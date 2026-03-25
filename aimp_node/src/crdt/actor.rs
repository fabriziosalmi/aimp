use crate::crdt::gc::EpochManager;
use crate::crdt::merkle_dag::{DagNode, MerkleCrdtEngine};
use crate::crdt::PersistentStore;
use crate::event::metrics::GLOBAL_METRICS;
use crate::protocol::Hash32;
use std::collections::BTreeMap;
use tokio::sync::{mpsc, oneshot};

/// Messages handled by the CrdtActor
#[derive(Debug)]
pub enum CrdtMsg {
    /// Append a new mutation to the DAG
    Append {
        data_hash: Hash32,
        signature: [u8; 64],
        vclock: BTreeMap<String, u64>,
        evidence: Option<crate::crdt::merkle_dag::AiEvidence>,
        resp: oneshot::Sender<Hash32>,
    },
    /// Merge external nodes into the local state
    Merge {
        nodes: Vec<DagNode>,
        resp: oneshot::Sender<usize>, // Returns number of new nodes added
    },
    /// Request the current Merkle Root
    GetRoot { resp: oneshot::Sender<Hash32> },
    /// Get nodes for synchronization (Delta-Sync)
    GetDiff {
        remote_heads: Vec<Hash32>,
        resp: oneshot::Sender<Vec<DagNode>>,
    },
}

pub struct CrdtActor {
    engine: MerkleCrdtEngine,
    quorum: crate::crdt::QuorumManager,
    epochs: EpochManager,
    receiver: mpsc::Receiver<CrdtMsg>,
    log_tx: Option<mpsc::Sender<crate::event::SystemEvent>>,
}

impl CrdtActor {
    pub fn new(
        receiver: mpsc::Receiver<CrdtMsg>,
        store: Option<PersistentStore>,
        log_tx: Option<mpsc::Sender<crate::event::SystemEvent>>,
        threshold: usize,
        gc_threshold: u64,
    ) -> Self {
        let mut engine = MerkleCrdtEngine::with_gc_threshold(store, gc_threshold);

        // RECOVERY: If store is present, load nodes in batches to avoid OOM
        if let Some(ref s) = engine.store {
            s.load_batched(1024, |batch| {
                for (hash, node) in batch {
                    engine.arena.insert(hash, node);
                    engine.heads.insert(hash);
                }
            });
            // Recalculate frontier (remove parents that are in the store)
            let mut all_parents = std::collections::HashSet::new();
            for (_, node) in engine.arena.get_all_iter() {
                for p in &node.parents {
                    all_parents.insert(*p);
                }
            }
            engine.heads.retain(|h| !all_parents.contains(h));
            engine.invalidate_root();
        }

        Self {
            engine,
            quorum: crate::crdt::QuorumManager::new(threshold),
            epochs: EpochManager::new(),
            receiver,
            log_tx,
        }
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.receiver.recv().await {
            match msg {
                CrdtMsg::Append {
                    data_hash,
                    signature,
                    vclock,
                    evidence,
                    resp,
                } => {
                    let gc_was_pending = self.engine.mutations_since_gc
                        >= self.engine.gc_threshold.saturating_sub(1);
                    let hash =
                        self.engine
                            .append_mutation(data_hash, signature, vclock, evidence.clone());

                    // If GC just ran, finalize the epoch with the new root
                    if gc_was_pending && self.engine.mutations_since_gc == 0 {
                        let root = self.engine.get_merkle_root();
                        self.epochs.finalize_epoch(root);
                        if let Some(ref tx) = self.log_tx {
                            let _ = tx.try_send(crate::event::SystemEvent::GarbageCollection {
                                nodes_pruned: 0, // actual count tracked in compact_history
                                remaining: self.engine.arena.len(),
                            });
                        }
                    }

                    GLOBAL_METRICS.mutation_count.inc();
                    GLOBAL_METRICS.dag_size.set(self.engine.arena.len() as f64);
                    let _ = resp.send(hash);
                }
                CrdtMsg::Merge { nodes, resp } => {
                    let mut added = 0;
                    for node in nodes {
                        let hash = node.compute_hash();
                        if !self.engine.arena.contains(&hash) {
                            // Equivocation detection: check for conflicting mutations
                            if let Some(proof) = self.quorum.check_equivocation(&node, hash) {
                                if let Some(ref tx) = self.log_tx {
                                    let _ = tx.try_send(crate::event::SystemEvent::SecurityDrop {
                                        peer: proof.origin.clone(),
                                        reason: format!(
                                            "EQUIVOCATION at tick {}: nodes {} vs {}",
                                            proof.tick,
                                            hex::encode(&proof.hash_a[..4]),
                                            hex::encode(&proof.hash_b[..4]),
                                        ),
                                    });
                                }
                                // Still merge the node (CRDT must accept all data)
                                // but the origin is now denied from quorum voting
                            }

                            // Process Evidence for Quorum (rejected if origin is denied)
                            if let Some(ref evidence) = node.evidence {
                                if self
                                    .quorum
                                    .observe(node.signature[..32].try_into().unwrap(), evidence)
                                {
                                    if let Some(ref tx) = self.log_tx {
                                        let _ =
                                            tx.try_send(crate::event::SystemEvent::AiInference {
                                                prompt: format!("[VERIFIED] {}", evidence.prompt),
                                                decision: evidence.decision.clone(),
                                            });
                                    }
                                }
                            }

                            self.engine.arena.insert(hash, node.clone());

                            // Persist merged node
                            if let Some(ref store) = self.engine.store {
                                let _ = store.save_node(&hash, &node);
                            }

                            added += 1;
                        }
                    }

                    if added > 0 {
                        // Recompute heads from the full arena to handle
                        // out-of-order message delivery correctly.
                        // A node is a head iff no other node lists it as a parent.
                        let mut has_children = rustc_hash::FxHashSet::default();
                        for (_, node) in self.engine.arena.get_all_iter() {
                            for p in &node.parents {
                                has_children.insert(*p);
                            }
                        }
                        self.engine.heads.clear();
                        for (hash, _) in self.engine.arena.get_all_iter() {
                            if !has_children.contains(hash) {
                                self.engine.heads.insert(*hash);
                            }
                        }
                        self.engine.invalidate_root();
                    }

                    GLOBAL_METRICS.mutation_count.inc();
                    GLOBAL_METRICS.dag_size.set(self.engine.arena.len() as f64);
                    let _ = resp.send(added);
                }
                CrdtMsg::GetRoot { resp } => {
                    let _ = resp.send(self.engine.get_merkle_root());
                }
                CrdtMsg::GetDiff { remote_heads, resp } => {
                    let _ = resp.send(self.engine.get_vdiff(remote_heads));
                }
            }
        }
    }
}

/// Handle to communicate with the CrdtActor
#[derive(Clone)]
pub struct CrdtHandle {
    tx: mpsc::Sender<CrdtMsg>,
}

impl CrdtHandle {
    pub fn new(tx: mpsc::Sender<CrdtMsg>) -> Self {
        Self { tx }
    }

    pub async fn append_mutation(
        &self,
        data_hash: Hash32,
        signature: [u8; 64],
        vclock: BTreeMap<String, u64>,
        evidence: Option<crate::crdt::merkle_dag::AiEvidence>,
    ) -> Hash32 {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(CrdtMsg::Append {
                data_hash,
                signature,
                vclock,
                evidence,
                resp: tx,
            })
            .await;
        rx.await.expect("Actor died")
    }

    pub async fn merge_nodes(&self, nodes: Vec<DagNode>) -> usize {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(CrdtMsg::Merge { nodes, resp: tx }).await;
        rx.await.expect("Actor died")
    }

    pub async fn get_merkle_root(&self) -> Hash32 {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(CrdtMsg::GetRoot { resp: tx }).await;
        rx.await.expect("Actor died")
    }
}
