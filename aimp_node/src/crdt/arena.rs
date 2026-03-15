use slab::Slab;
use crate::protocol::Hash32;
use crate::crdt::merkle_dag::DagNode;
use std::collections::HashMap;

/// A high-performance Arena for DagNodes using Slab allocation.
/// This provides O(1) insertion and stable references/indices.
pub struct DagArena {
    slab: Slab<DagNode>,
    hash_to_index: HashMap<Hash32, usize>,
}

impl DagArena {
    pub fn new() -> Self {
        Self {
            slab: Slab::new(),
            hash_to_index: HashMap::with_capacity(1024),
        }
    }

    /// Insert a node into the arena. Returns the index and whether it was newly inserted.
    pub fn insert(&mut self, hash: Hash32, node: DagNode) -> (usize, bool) {
        if let Some(&index) = self.hash_to_index.get(&hash) {
            return (index, false);
        }

        let index = self.slab.insert(node);
        self.hash_to_index.insert(hash, index);
        (index, true)
    }

    pub fn get_by_hash(&self, hash: &Hash32) -> Option<&DagNode> {
        self.hash_to_index.get(hash).map(|&idx| &self.slab[idx])
    }

    pub fn get_by_index(&self, index: usize) -> Option<&DagNode> {
        self.slab.get(index)
    }

    pub fn contains(&self, hash: &Hash32) -> bool {
        self.hash_to_index.contains_key(hash)
    }

    pub fn len(&self) -> usize {
        self.slab.len()
    }

    pub fn get_all_iter(&self) -> impl Iterator<Item = (&Hash32, &DagNode)> {
        self.hash_to_index.iter().map(move |(hash, &idx)| (hash, &self.slab[idx]))
    }

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
