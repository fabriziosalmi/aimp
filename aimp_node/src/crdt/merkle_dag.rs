use crate::config;
use crate::crdt::arena::DagArena;
use crate::protocol::Hash32;
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::BTreeMap;

/// Cryptographic evidence of an AI inference decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiEvidence {
    pub prompt: String,     // Original input prompt
    pub decision: String,   // Deterministic output
    pub model_hash: Hash32, // Hash of the logic/model used
    pub timestamp: u64,     // Local observation time
}

/// A single node in the Merkle-DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DagNode {
    pub parents: SmallVec<[Hash32; 2]>, // Causal links (stack-allocated for ≤2 parents)
    #[serde(with = "serde_bytes")]
    pub signature: [u8; 64], // Cryptographic proof of origin
    #[serde(with = "serde_bytes")]
    pub data_hash: Hash32, // Deterministic payload hash
    pub vclock: BTreeMap<String, u64>, // Structural causality
    pub evidence: Option<AiEvidence>, // Optional decision audit trail
}

impl DagNode {
    /// Compute the content hash using streaming BLAKE3 (zero Vec allocation).
    pub fn compute_hash(&self) -> Hash32 {
        let mut hasher = blake3::Hasher::new();

        // Parents (sorted for determinism is not needed — they come from
        // the heads set which is order-independent, and the hash covers
        // the full parent list as-is for this specific node's identity)
        for p in &self.parents {
            hasher.update(p);
        }
        hasher.update(&self.signature);
        hasher.update(&self.data_hash);

        // Deterministic vclock: iterate BTreeMap (already sorted by key)
        for (key, val) in &self.vclock {
            hasher.update(key.as_bytes());
            hasher.update(&val.to_le_bytes());
        }

        // Include evidence if present
        if let Some(ref evidence) = self.evidence {
            hasher.update(evidence.prompt.as_bytes());
            hasher.update(evidence.decision.as_bytes());
            hasher.update(&evidence.model_hash);
            hasher.update(&evidence.timestamp.to_le_bytes());
        }

        *hasher.finalize().as_bytes()
    }
}

use crate::crdt::store::PersistentStore;

/// The state container for the Merkle-DAG.
///
/// The merkle root is cached and only recomputed when the frontier (heads) changes,
/// avoiding redundant sort+hash on every health check or dashboard poll.
pub struct MerkleCrdtEngine {
    pub arena: DagArena,
    pub heads: FxHashSet<Hash32>,
    pub heads_soa: Vec<Hash32>,
    pub mutations_since_gc: u64,
    pub gc_threshold: u64,
    pub store: Option<PersistentStore>,
    /// Cached merkle root — `None` means heads changed and root needs recomputation.
    cached_root: Option<Hash32>,
}

impl MerkleCrdtEngine {
    pub fn new(store: Option<PersistentStore>) -> Self {
        Self::with_gc_threshold(store, config::GC_MUTATION_THRESHOLD)
    }

    pub fn with_gc_threshold(store: Option<PersistentStore>, gc_threshold: u64) -> Self {
        Self {
            arena: DagArena::new(),
            heads: FxHashSet::default(),
            heads_soa: Vec::with_capacity(64),
            mutations_since_gc: 0,
            gc_threshold,
            store,
            cached_root: None,
        }
    }

    /// Invalidate the cached merkle root. Must be called whenever heads change.
    pub fn invalidate_root(&mut self) {
        self.cached_root = None;
        self.heads_soa = self.heads.iter().copied().collect();
    }

    pub fn append_mutation(
        &mut self,
        data_hash: Hash32,
        signature: [u8; 64],
        vclock: BTreeMap<String, u64>,
        evidence: Option<AiEvidence>,
    ) -> Hash32 {
        let parents: SmallVec<[Hash32; 2]> = self.heads.iter().copied().collect();

        let node = DagNode {
            parents,
            signature,
            data_hash,
            vclock,
            evidence,
        };

        let hash = node.compute_hash();

        // Update heads: remove parents, add new node
        for p in &node.parents {
            self.heads.remove(p);
        }
        self.heads.insert(hash);

        // Persist to disk if store is active (before moving node into arena)
        if let Some(ref store) = self.store {
            let _ = store.save_node(&hash, &node);
        }

        self.arena.insert(hash, node);

        // Invalidate cached root since heads changed
        self.invalidate_root();

        // Epoch GC Trigger
        self.mutations_since_gc += 1;
        if self.mutations_since_gc >= self.gc_threshold {
            self.compact_history();
            self.mutations_since_gc = 0;
        }

        hash
    }

    /// Prune nodes that are no longer reachable from the frontier within `DAG_HISTORY_DEPTH`.
    ///
    /// Performs a mark-and-sweep: BFS from heads up to the configured depth,
    /// then removes all unmarked nodes from the slab arena to reclaim memory.
    pub fn compact_history(&mut self) {
        let mut keep = FxHashSet::default();
        let mut queue = Vec::new();

        for h in &self.heads {
            queue.push((*h, 0u32));
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

        // Sweep: remove nodes not in the keep set from the slab
        let removed = self.arena.retain(&keep);
        if removed > 0 {
            self.heads.retain(|h| keep.contains(h));
            self.invalidate_root();
        }
    }

    /// Compute the delta between local state and a remote peer's heads.
    ///
    /// Traversal is bounded by `DAG_HISTORY_DEPTH` to avoid walking the entire
    /// DAG history on every sync request. Nodes already known to the remote
    /// (present in `remote_heads`) act as BFS stop points.
    pub fn get_vdiff(&self, remote_heads: Vec<Hash32>) -> Vec<DagNode> {
        let mut deltas = Vec::new();
        let mut visited = FxHashSet::default();
        let mut queue: Vec<(Hash32, u32)> = Vec::new();

        for h in &self.heads {
            queue.push((*h, 0));
        }

        let remote_set: FxHashSet<Hash32> = remote_heads.into_iter().collect();

        while let Some((current_hash, depth)) = queue.pop() {
            if visited.contains(&current_hash)
                || remote_set.contains(&current_hash)
                || depth > config::DAG_HISTORY_DEPTH
            {
                continue;
            }
            visited.insert(current_hash);

            if let Some(node) = self.arena.get_by_hash(&current_hash) {
                for p in &node.parents {
                    queue.push((*p, depth + 1));
                }
                deltas.push(node.clone());
            }
        }

        deltas
    }

    /// Return the merkle root of the current frontier.
    ///
    /// Uses a cached value that is only recomputed when heads change,
    /// making repeated calls (e.g. health checks, dashboard polls) O(1).
    pub fn get_merkle_root(&mut self) -> Hash32 {
        if let Some(root) = self.cached_root {
            return root;
        }

        if self.heads_soa.is_empty() {
            let root = [0u8; 32];
            self.cached_root = Some(root);
            return root;
        }

        // Sort in-place (heads_soa is rebuilt on every invalidate anyway)
        self.heads_soa.sort_unstable();

        // Streaming hash — no Vec allocation
        let mut hasher = blake3::Hasher::new();
        for h in &self.heads_soa {
            hasher.update(h);
        }
        let root = *hasher.finalize().as_bytes();
        self.cached_root = Some(root);
        root
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
                parents: SmallVec::new(),
                signature: sig,
                data_hash,
                vclock,
                evidence: None,
            };
            let hash = node.compute_hash();

            // Merge once
            engine.arena.insert(hash, node.clone());
            engine.heads.insert(hash);
            engine.invalidate_root();
            let root1 = engine.get_merkle_root();

            // Merge identical node again (no-op, same hash)
            engine.arena.insert(hash, node);
            engine.heads.insert(hash);
            engine.invalidate_root();
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

            let node1 = DagNode { parents: SmallVec::new(), signature: sig1, data_hash: data1, vclock: vclocks[0].clone(), evidence: None };
            let node2 = DagNode { parents: SmallVec::new(), signature: sig2, data_hash: data2, vclock: vclocks[1].clone(), evidence: None };

            let h1 = node1.compute_hash();
            let h2 = node2.compute_hash();

            // Sync A: add node1, then node2
            engine_a.arena.insert(h1, node1.clone());
            engine_a.heads.insert(h1);
            engine_a.arena.insert(h2, node2.clone());
            engine_a.heads.insert(h2);
            engine_a.invalidate_root();

            // Sync B: add node2, then node1
            engine_b.arena.insert(h2, node2);
            engine_b.heads.insert(h2);
            engine_b.arena.insert(h1, node1);
            engine_b.heads.insert(h1);
            engine_b.invalidate_root();

            // Merkle Root must converge regardless of receipt order
            prop_assert_eq!(engine_a.get_merkle_root(), engine_b.get_merkle_root());
        }
    }
}
