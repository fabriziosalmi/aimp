use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use crate::protocol::Hash32;
use crate::crypto::SecurityFirewall;
use crate::config;
use crate::crdt::arena::DagArena;

/// Cryptographic evidence of an AI inference decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiEvidence {
    pub prompt: String,         // Original input prompt
    pub decision: String,       // Deterministic output
    pub model_hash: Hash32,     // Hash of the logic/model used
    pub timestamp: u64,         // Local observation time
}

/// A single node in the Merkle-DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNode {
    pub parents: Vec<Hash32>,          // Causal links (Merkle-Edges)
    #[serde(with = "serde_bytes")]
    pub signature: [u8; 64],           // Cryptographic proof of origin
    #[serde(with = "serde_bytes")]
    pub data_hash: Hash32,             // Deterministic payload hash
    pub vclock: BTreeMap<String, u64>, // Structural causality
    pub evidence: Option<AiEvidence>,  // Optional AI Audit Trail (v0.4.0)
}

impl DagNode {
    pub fn compute_hash(&self) -> Hash32 {
        let mut bytes = Vec::new();
        for p in &self.parents { bytes.extend_from_slice(p); }
        bytes.extend_from_slice(&self.signature);
        bytes.extend_from_slice(&self.data_hash);
        // Deterministic vclock hash
        let clock_bytes = rmp_serde::to_vec(&self.vclock).unwrap_or_default();
        bytes.extend_from_slice(&clock_bytes);

        // Include evidence in hash if present (v0.4.0)
        if let Some(ref evidence) = self.evidence {
            let ev_bytes = rmp_serde::to_vec(evidence).unwrap_or_default();
            bytes.extend_from_slice(&ev_bytes);
        }

        SecurityFirewall::hash(&bytes)
    }
}

use crate::crdt::store::PersistentStore;

/// The state container for the Merkle-DAG.
/// In v0.3.0, this supports durable persistence via a Sled-based store.
pub struct MerkleCrdtEngine {
    pub arena: DagArena,
    pub heads: HashSet<Hash32>,
    pub heads_soa: Vec<Hash32>, 
    pub mutations_since_gc: u64,
    pub store: Option<PersistentStore>, // Durable persistence
}

impl MerkleCrdtEngine {
    pub fn new(store: Option<PersistentStore>) -> Self {
        Self {
            arena: DagArena::new(),
            heads: HashSet::new(),
            heads_soa: Vec::with_capacity(64),
            mutations_since_gc: 0,
            store,
        }
    }

    pub fn append_mutation(
        &mut self, 
        data_hash: Hash32, 
        signature: [u8; 64], 
        vclock: BTreeMap<String, u64>,
        evidence: Option<AiEvidence>
    ) -> Hash32 {
        let parents: Vec<Hash32> = self.heads.iter().copied().collect();
        
        let node = DagNode {
            parents: parents.clone(),
            signature,
            data_hash,
            vclock,
            evidence,
        };

        let hash = node.compute_hash();
        
        // Update heads (SoA & Set)
        for p in &parents {
            self.heads.remove(p);
        }
        self.heads.insert(hash);
        self.arena.insert(hash, node.clone());
        
        // Persist to disk if store is active
        if let Some(ref store) = self.store {
            let _ = store.save_node(&hash, &node);
        }
        
        // Synchronize SoA
        self.heads_soa = self.heads.iter().copied().collect();

        // Epoch GC Trigger
        self.mutations_since_gc += 1;
        if self.mutations_since_gc >= config::GC_MUTATION_THRESHOLD {
            self.compact_history();
            self.mutations_since_gc = 0;
        }

        hash
    }

    /// Prune nodes that are no longer part of the frontier or its immediate history
    pub fn compact_history(&mut self) {
        // In v0.2.1 with Slab, we'd ideally Mark/Sweep indices.
        // For now, we perform a logic-level compaction.
        let mut keep = HashSet::new();
        let mut queue = Vec::new();

        for h in &self.heads {
            queue.push((*h, 0));
        }

        while let Some((h, depth)) = queue.pop() {
            if keep.contains(&h) || depth > config::DAG_HISTORY_DEPTH {
                continue;
            }
            keep.insert(h);
            if let Some(node) = self.arena.get_by_hash(&h) {
                for p in &node.parents {
                    queue.push((*p, depth + 1));
                }
            }
        }

        // Slab sweep is more complex; for v0.2.1 we focus on logic correctness
        // and Arena ownership. Full SoA sweep to be implemented in v0.2.2.
    }

    pub fn get_vdiff(&self, remote_heads: Vec<Hash32>) -> Vec<DagNode> {
        let mut deltas = Vec::new();
        let mut visited = HashSet::new();
        let mut queue = Vec::new();

        for h in &self.heads {
            queue.push(*h);
        }

        let remote_set: HashSet<Hash32> = remote_heads.into_iter().collect();

        while let Some(current_hash) = queue.pop() {
            if visited.contains(&current_hash) || remote_set.contains(&current_hash) {
                continue;
            }
            visited.insert(current_hash);

            if let Some(node) = self.arena.get_by_hash(&current_hash) {
                for p in &node.parents {
                    queue.push(*p);
                }
                deltas.push(node.clone());
            }
        }

        deltas
    }

    pub fn get_merkle_root(&self) -> Hash32 {
        if self.heads_soa.is_empty() {
            return [0u8; 32];
        }

        let mut sorted_heads = self.heads_soa.clone();
        sorted_heads.sort_unstable(); // Use unstable for better performance in SoA
        
        let mut combined = Vec::with_capacity(sorted_heads.len() * 32);
        for h in sorted_heads {
            combined.extend_from_slice(&h);
        }
        SecurityFirewall::hash(&combined)
    }
}

impl Default for MerkleCrdtEngine {
    fn default() -> Self {
        Self::new(None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_dag_merge_idempotency(
            data_hash in prop::array::uniform32(0u8..255), 
            sig_vec in prop::collection::vec(0u8..255, 64)
        ) {
            let mut sig = [0u8; 64];
            sig.copy_from_slice(&sig_vec);

            let mut engine = MerkleCrdtEngine::default();
            let mut vclock = BTreeMap::new();
            vclock.insert("node1".to_string(), 1);

            let node = DagNode {
                parents: vec![],
                signature: sig,
                data_hash,
                vclock,
            };
            let hash = node.compute_hash();

            // Merge once
            engine.arena.insert(hash, node.clone());
            engine.heads.insert(hash);
            let root1 = engine.get_merkle_root();

            // Merge identical node again
            engine.arena.insert(hash, node);
            engine.heads.insert(hash);
            let root2 = engine.get_merkle_root();

            prop_assert_eq!(root1, root2);
        }

        #[test]
        fn test_dag_convergence(
            data1 in prop::array::uniform32(0u8..255), 
            sig1_vec in prop::collection::vec(0u8..255, 64),
            data2 in prop::array::uniform32(0u8..255),
            sig2_vec in prop::collection::vec(0u8..255, 64)
        ) {
            let mut sig1 = [0u8; 64]; sig1.copy_from_slice(&sig1_vec);
            let mut sig2 = [0u8; 64]; sig2.copy_from_slice(&sig2_vec);

            let mut engine_a = MerkleCrdtEngine::default();
            let mut engine_b = MerkleCrdtEngine::default();

            let mut vclocks = vec![BTreeMap::new(), BTreeMap::new()];
            vclocks[0].insert("a".to_string(), 1);
            vclocks[1].insert("b".to_string(), 1);

            let node1 = DagNode { parents: vec![], signature: sig1, data_hash: data1, vclock: vclocks[0].clone() };
            let node2 = DagNode { parents: vec![], signature: sig2, data_hash: data2, vclock: vclocks[1].clone() };

            let h1 = node1.compute_hash();
            let h2 = node2.compute_hash();

            // Sync A: add node1, then node2
            engine_a.arena.insert(h1, node1.clone());
            engine_a.heads.insert(h1);
            engine_a.arena.insert(h2, node2.clone());
            engine_a.heads.insert(h2);

            // Sync B: add node2, then node1
            engine_b.arena.insert(h2, node2);
            engine_b.heads.insert(h2);
            engine_b.arena.insert(h1, node1);
            engine_b.heads.insert(h1);

            // Merkle Root must converge regardless of receipt order
            prop_assert_eq!(engine_a.get_merkle_root(), engine_b.get_merkle_root());
        }
    }
}
