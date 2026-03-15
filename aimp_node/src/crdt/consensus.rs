use std::collections::{HashMap, HashSet};
use crate::crdt::merkle_dag::AiEvidence;

/// Tracks consensus for AI decisions across the mesh.
/// In v0.4.0, a decision is "Verified" if K independent nodes publish the same data.
pub struct QuorumManager {
    pub threshold: usize,
    // Map: Prompt Hash -> (Decision Hash -> Set of Node IDs)
    decisions: HashMap<[u8; 32], HashMap<[u8; 32], HashSet<[u8; 32]>>>,
    // Track verified prompts to avoid redundant events
    verified: HashSet<[u8; 32]>,
}

impl QuorumManager {
    pub fn new(threshold: usize) -> Self {
        Self {
            threshold,
            decisions: HashMap::new(),
            verified: HashSet::new(),
        }
    }

    /// Process a new piece of evidence from a peer.
    /// Returns true if this evidence reached the consensus threshold.
    pub fn observe(&mut self, origin: [u8; 32], evidence: &AiEvidence) -> bool {
        let prompt_hash = crate::crypto::SecurityFirewall::hash(evidence.prompt.as_bytes());
        let decision_hash = crate::crypto::SecurityFirewall::hash(evidence.decision.as_bytes());

        if self.verified.contains(&prompt_hash) {
            return false;
        }

        let prompt_entry = self.decisions.entry(prompt_hash).or_insert_with(HashMap::new);
        let decision_entry = prompt_entry.entry(decision_hash).or_insert_with(HashSet::new);
        
        decision_entry.insert(origin);

        if decision_entry.len() >= self.threshold {
            self.verified.insert(prompt_hash);
            return true;
        }

        false
    }

    pub fn is_verified(&self, prompt: &str) -> bool {
        let prompt_hash = crate::crypto::SecurityFirewall::hash(prompt.as_bytes());
        self.verified.contains(&prompt_hash)
    }

    pub fn get_support_count(&self, prompt: &str, decision: &str) -> usize {
        let prompt_hash = crate::crypto::SecurityFirewall::hash(prompt.as_bytes());
        let decision_hash = crate::crypto::SecurityFirewall::hash(decision.as_bytes());
        
        self.decisions.get(&prompt_hash)
            .and_then(|d| d.get(&decision_hash))
            .map(|s| s.len())
            .unwrap_or(0)
    }
}
