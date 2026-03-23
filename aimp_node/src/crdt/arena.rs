use crate::crdt::merkle_dag::DagNode;
use crate::protocol::Hash32;
use slab::Slab;
use std::collections::HashMap;

/// Slab-based arena allocator for Merkle-DAG nodes.
///
/// Provides O(1) insertion with stable indices and dual lookup
/// via both hash and slab index. Pre-allocates capacity for 1024 nodes.
pub struct DagArena {
    slab: Slab<DagNode>,
    hash_to_index: HashMap<Hash32, usize>,
}

impl DagArena {
    /// Create a new arena with pre-allocated capacity.
    pub fn new() -> Self {
        Self {
            slab: Slab::new(),
            hash_to_index: HashMap::with_capacity(1024),
        }
    }

    /// Insert a node into the arena. Returns `(index, true)` if newly inserted,
    /// or `(existing_index, false)` if the hash was already present.
    pub fn insert(&mut self, hash: Hash32, node: DagNode) -> (usize, bool) {
        if let Some(&index) = self.hash_to_index.get(&hash) {
            return (index, false);
        }

        let index = self.slab.insert(node);
        self.hash_to_index.insert(hash, index);
        (index, true)
    }

    /// Look up a node by its content hash.
    pub fn get_by_hash(&self, hash: &Hash32) -> Option<&DagNode> {
        self.hash_to_index.get(hash).map(|&idx| &self.slab[idx])
    }

    /// Look up a node by its slab index.
    pub fn get_by_index(&self, index: usize) -> Option<&DagNode> {
        self.slab.get(index)
    }

    /// Check whether a node with the given hash exists.
    pub fn contains(&self, hash: &Hash32) -> bool {
        self.hash_to_index.contains_key(hash)
    }

    /// Return the number of nodes in the arena.
    pub fn len(&self) -> usize {
        self.slab.len()
    }

    /// Check if the arena is empty.
    pub fn is_empty(&self) -> bool {
        self.slab.is_empty()
    }

    /// Iterate over all `(hash, node)` pairs.
    pub fn get_all_iter(&self) -> impl Iterator<Item = (&Hash32, &DagNode)> {
        self.hash_to_index
            .iter()
            .map(move |(hash, &idx)| (hash, &self.slab[idx]))
    }

    /// Retain only nodes whose hashes are in the given set.
    /// Removes all other nodes from both the slab and the index.
    /// Returns the number of nodes removed.
    pub fn retain(&mut self, keep: &std::collections::HashSet<Hash32>) -> usize {
        let to_remove: Vec<(Hash32, usize)> = self
            .hash_to_index
            .iter()
            .filter(|(h, _)| !keep.contains(*h))
            .map(|(h, &idx)| (*h, idx))
            .collect();

        let removed = to_remove.len();
        for (hash, idx) in to_remove {
            self.hash_to_index.remove(&hash);
            self.slab.remove(idx);
        }
        removed
    }

    /// Remove all nodes from the arena.
    pub fn clear(&mut self) {
        self.slab.clear();
        self.hash_to_index.clear();
    }
}

impl Default for DagArena {
    fn default() -> Self {
        Self::new()
    }
}
