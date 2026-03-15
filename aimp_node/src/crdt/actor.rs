use std::collections::BTreeMap;
use tokio::sync::{mpsc, oneshot};
use crate::protocol::Hash32;
use crate::crdt::merkle_dag::{DagNode, MerkleCrdtEngine};
use crate::crdt::PersistentStore;
use crate::event::metrics::GLOBAL_METRICS;

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
    GetRoot {
        resp: oneshot::Sender<Hash32>,
    },
    /// Get nodes for synchronization (Delta-Sync)
    GetDiff {
        remote_heads: Vec<Hash32>,
        resp: oneshot::Sender<Vec<DagNode>>,
    },
}

pub struct CrdtActor {
    engine: MerkleCrdtEngine,
    quorum: crate::crdt::QuorumManager,
    receiver: mpsc::Receiver<CrdtMsg>,
    log_tx: Option<mpsc::Sender<crate::event::SystemEvent>>,
}

impl CrdtActor {
    pub fn new(receiver: mpsc::Receiver<CrdtMsg>, store: Option<PersistentStore>, log_tx: Option<mpsc::Sender<crate::event::SystemEvent>>, threshold: usize) -> Self {
        let mut engine = MerkleCrdtEngine::new(store);
        
        // RECOVERY: If store is present, load all existing nodes
        if let Some(ref s) = engine.store {
            for (hash, node) in s.load_all() {
                engine.arena.insert(hash, node);
                engine.heads.insert(hash); 
            }
            // Recalculate frontier (remove parents that are in the store)
            let mut all_parents = std::collections::HashSet::new();
            for (_, node) in engine.arena.get_all_iter() {
                for p in &node.parents { all_parents.insert(*p); }
            }
            engine.heads.retain(|h| !all_parents.contains(h));
            engine.heads_soa = engine.heads.iter().copied().collect();
        }

        Self {
            engine,
            quorum: crate::crdt::QuorumManager::new(threshold), 
            receiver,
            log_tx,
        }
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.receiver.recv().await {
            match msg {
                CrdtMsg::Append { data_hash, signature, vclock, evidence, resp } => {
                    let hash = self.engine.append_mutation(data_hash, signature, vclock, evidence.clone());
                    
                    // Observe own evidence
                    if let Some(ref _ev) = evidence {
                         // NodeId is usually handled by Networking, here we use placeholder or actual identity
                         // For v0.4.0 we'll track locally published evidence as part of the quorum
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
                            // Update heads: remove parents of the new node
                            for p in &node.parents {
                                self.engine.heads.remove(p);
                            }
                            // Process Evidence for Quorum (v0.4.0)
                            if let Some(ref evidence) = node.evidence {
                                if self.quorum.observe(node.signature[..32].try_into().unwrap(), evidence) {
                                    if let Some(ref tx) = self.log_tx {
                                        let _ = tx.try_send(crate::event::SystemEvent::AiInference { 
                                            prompt: format!("[VERIFIED] {}", evidence.prompt),
                                            decision: evidence.decision.clone(),
                                        });
                                    }
                                }
                            }

                            self.engine.heads.insert(hash);
                            self.engine.arena.insert(hash, node.clone());
                            
                            // Persist merged node
                            if let Some(ref store) = self.engine.store {
                                let _ = store.save_node(&hash, &node);
                            }

                            added += 1;
                        }
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

    pub async fn append_mutation(&self, data_hash: Hash32, signature: [u8; 64], vclock: BTreeMap<String, u64>, evidence: Option<crate::crdt::merkle_dag::AiEvidence>) -> Hash32 {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(CrdtMsg::Append { data_hash, signature, vclock, evidence, resp: tx }).await;
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
