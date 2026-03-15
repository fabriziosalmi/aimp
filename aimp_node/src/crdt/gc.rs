use crate::protocol::Hash32;

/// Implementation of Cryptographic Epochs for Memory Safety.
/// Prunes the Merkle-DAG when a Quorum acknowledges a state finalized.
pub struct EpochManager {
    pub current_epoch: u64,
    pub finalized_root: Option<Hash32>,
}

impl EpochManager {
    pub fn new() -> Self {
        Self {
            current_epoch: 0,
            finalized_root: None,
        }
    }

    pub fn finalize_epoch(&mut self, root: Hash32) {
        self.finalized_root = Some(root);
        self.current_epoch += 1;
        // Logic for O(1) pruning would be implemented here in a production switch.
    }
}

impl Default for EpochManager {
    fn default() -> Self {
        Self::new()
    }
}
