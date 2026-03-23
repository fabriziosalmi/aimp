use crate::protocol::Hash32;

/// Epoch-based garbage collection manager for the Merkle-DAG.
///
/// Tracks the current epoch number and the most recently finalized root hash.
/// When a quorum acknowledges a state as finalized, the epoch advances and
/// nodes older than the finalized frontier can be pruned.
pub struct EpochManager {
    /// Current epoch number, incremented on each finalization.
    pub current_epoch: u64,
    /// Root hash of the most recently finalized state.
    pub finalized_root: Option<Hash32>,
}

impl EpochManager {
    /// Create a new epoch manager starting at epoch 0.
    pub fn new() -> Self {
        Self {
            current_epoch: 0,
            finalized_root: None,
        }
    }

    /// Finalize the current epoch with the given root hash and advance to the next.
    pub fn finalize_epoch(&mut self, root: Hash32) {
        self.finalized_root = Some(root);
        self.current_epoch += 1;
    }
}

impl Default for EpochManager {
    fn default() -> Self {
        Self::new()
    }
}
