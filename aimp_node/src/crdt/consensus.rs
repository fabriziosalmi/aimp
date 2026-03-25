use crate::crdt::merkle_dag::{AiEvidence, DagNode};
use crate::protocol::Hash32;
use std::collections::{HashMap, HashSet};

/// Cryptographic proof that a node signed two conflicting mutations
/// with the same vector clock tick — irrefutable evidence of Byzantine behavior.
#[derive(Debug, Clone)]
pub struct EquivocationProof {
    /// The origin node ID (vclock key) that equivocated.
    pub origin: String,
    /// The vector clock tick at which equivocation occurred.
    pub tick: u64,
    /// Hash of the first conflicting DagNode.
    pub hash_a: Hash32,
    /// Hash of the second conflicting DagNode.
    pub hash_b: Hash32,
}

/// Tracks consensus for decisions across the mesh, with equivocation detection.
///
/// A decision is considered "Verified" when at least `threshold` independent nodes
/// publish identical decision hashes for the same prompt. Nodes that produce
/// conflicting mutations at the same vector clock tick are detected and denied.
pub struct QuorumManager {
    /// Minimum number of agreeing nodes required for verification.
    pub threshold: usize,
    /// Nested map: prompt_hash -> (decision_hash -> set of voting node IDs).
    #[allow(clippy::type_complexity)]
    decisions: HashMap<[u8; 32], HashMap<[u8; 32], HashSet<[u8; 32]>>>,
    /// Set of prompt hashes that have already reached quorum.
    verified: HashSet<[u8; 32]>,
    /// Deny list: origin node IDs proven to be Byzantine via equivocation.
    denied: HashSet<String>,
    /// Equivocation proofs collected (for gossip propagation).
    equivocation_proofs: Vec<EquivocationProof>,
    /// Observed mutations: origin+tick -> (data_hash, dag_node_hash) for equivocation detection.
    observed_ticks: HashMap<(String, u64), (Hash32, Hash32)>,
}

impl QuorumManager {
    /// Create a new quorum manager with the given consensus threshold.
    pub fn new(threshold: usize) -> Self {
        Self {
            threshold,
            decisions: HashMap::new(),
            verified: HashSet::new(),
            denied: HashSet::new(),
            equivocation_proofs: Vec::new(),
            observed_ticks: HashMap::new(),
        }
    }

    /// Check a DagNode for equivocation before merging.
    ///
    /// Equivocation: two DagNodes from the same origin at the same vclock tick
    /// but with different data_hash. This is cryptographic proof of Byzantine
    /// behavior — the node intentionally forked its causal history.
    ///
    /// Returns `Some(proof)` if equivocation is detected, `None` if clean.
    pub fn check_equivocation(&mut self, node: &DagNode, node_hash: Hash32) -> Option<EquivocationProof> {
        for (origin, &tick) in &node.vclock {
            // Skip already-denied origins
            if self.denied.contains(origin) {
                continue;
            }

            let key = (origin.clone(), tick);
            if let Some((prev_data_hash, prev_node_hash)) = self.observed_ticks.get(&key) {
                // Same origin + same tick: check if data differs
                if *prev_data_hash != node.data_hash {
                    let proof = EquivocationProof {
                        origin: origin.clone(),
                        tick,
                        hash_a: *prev_node_hash,
                        hash_b: node_hash,
                    };
                    self.denied.insert(origin.clone());
                    self.equivocation_proofs.push(proof.clone());
                    return Some(proof);
                }
            } else {
                self.observed_ticks.insert(key, (node.data_hash, node_hash));
            }
        }
        None
    }

    /// Check if an origin is on the deny list.
    pub fn is_denied(&self, origin: &str) -> bool {
        self.denied.contains(origin)
    }

    /// Return the deny list (for protocol-level isolation).
    pub fn denied_origins(&self) -> &HashSet<String> {
        &self.denied
    }

    /// Return collected equivocation proofs (for gossip propagation as PoM).
    pub fn equivocation_proofs(&self) -> &[EquivocationProof] {
        &self.equivocation_proofs
    }

    /// Record a piece of evidence from a peer node.
    ///
    /// Returns `true` if this evidence caused the prompt to reach consensus threshold.
    /// Returns `false` if the prompt was already verified or threshold not yet met.
    /// Rejects votes from denied (Byzantine) origins.
    pub fn observe(&mut self, origin: [u8; 32], evidence: &AiEvidence) -> bool {
        let prompt_hash = crate::crypto::SecurityFirewall::hash(evidence.prompt.as_bytes());
        let decision_hash = crate::crypto::SecurityFirewall::hash(evidence.decision.as_bytes());

        if self.verified.contains(&prompt_hash) {
            return false;
        }

        // Reject votes from denied origins
        let origin_hex = hex::encode(origin);
        if self.denied.contains(&origin_hex) {
            return false;
        }

        let prompt_entry = self.decisions.entry(prompt_hash).or_default();

        // A node may vote only once per prompt (not per decision).
        for existing_voters in prompt_entry.values() {
            if existing_voters.contains(&origin) {
                return false;
            }
        }

        let decision_entry = prompt_entry.entry(decision_hash).or_default();
        decision_entry.insert(origin);

        if decision_entry.len() >= self.threshold {
            self.verified.insert(prompt_hash);
            return true;
        }

        false
    }

    /// Check whether a given prompt has been verified by quorum.
    pub fn is_verified(&self, prompt: &str) -> bool {
        let prompt_hash = crate::crypto::SecurityFirewall::hash(prompt.as_bytes());
        self.verified.contains(&prompt_hash)
    }

    /// Return the set of verified prompt hashes (for persistence).
    pub fn verified_hashes(&self) -> &HashSet<Hash32> {
        &self.verified
    }

    /// Restore previously verified prompt hashes (loaded from persistent store).
    pub fn restore_verified(&mut self, hashes: HashSet<Hash32>) {
        self.verified.extend(hashes);
    }

    /// Return the number of nodes that agree on a specific (prompt, decision) pair.
    pub fn get_support_count(&self, prompt: &str, decision: &str) -> usize {
        let prompt_hash = crate::crypto::SecurityFirewall::hash(prompt.as_bytes());
        let decision_hash = crate::crypto::SecurityFirewall::hash(decision.as_bytes());

        self.decisions
            .get(&prompt_hash)
            .and_then(|d| d.get(&decision_hash))
            .map(|s| s.len())
            .unwrap_or(0)
    }
}
