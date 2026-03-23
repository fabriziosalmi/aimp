use crate::crdt::merkle_dag::AiEvidence;
use crate::protocol::Hash32;
use std::collections::{HashMap, HashSet};

/// Tracks consensus for AI decisions across the mesh.
///
/// A decision is considered "Verified" when at least `threshold` independent nodes
/// publish identical decision hashes for the same prompt. This implements a
/// simple Byzantine fault-tolerant voting mechanism for deterministic AI output.
pub struct QuorumManager {
    /// Minimum number of agreeing nodes required for verification.
    pub threshold: usize,
    /// Nested map: prompt_hash -> (decision_hash -> set of voting node IDs).
    #[allow(clippy::type_complexity)]
    decisions: HashMap<[u8; 32], HashMap<[u8; 32], HashSet<[u8; 32]>>>,
    /// Set of prompt hashes that have already reached quorum.
    verified: HashSet<[u8; 32]>,
}

impl QuorumManager {
    /// Create a new quorum manager with the given consensus threshold.
    pub fn new(threshold: usize) -> Self {
        Self {
            threshold,
            decisions: HashMap::new(),
            verified: HashSet::new(),
        }
    }

    /// Record a piece of evidence from a peer node.
    ///
    /// Returns `true` if this evidence caused the prompt to reach consensus threshold.
    /// Returns `false` if the prompt was already verified or threshold not yet met.
    pub fn observe(&mut self, origin: [u8; 32], evidence: &AiEvidence) -> bool {
        let prompt_hash = crate::crypto::SecurityFirewall::hash(evidence.prompt.as_bytes());
        let decision_hash = crate::crypto::SecurityFirewall::hash(evidence.decision.as_bytes());

        if self.verified.contains(&prompt_hash) {
            return false;
        }

        let prompt_entry = self.decisions.entry(prompt_hash).or_default();
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
