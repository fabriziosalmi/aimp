use serde::{Deserialize, Serialize};

pub mod metrics;

/// SOTA Structured Logging: Algebraic Data Type for system-wide events.
/// This replaces "vibe" strings with machine-queryable variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemEvent {
    /// Networking: Incoming packet security check
    SecurityDrop { peer: String, reason: String },
    
    /// CRDT: Merkle-DAG mutation committed
    MutationCommitted { hash: String, author: String },
    
    /// CRDT: Anti-entropy state merge
    StateMerged { nodes_added: usize },
    
    /// Epoch GC: History compaction event
    GarbageCollection { nodes_pruned: usize, remaining: usize },
    
    /// AI Engine: Deterministic inference decision
    AiInference { prompt: String, decision: String },
    
    /// System: General operational status
    Status(String),
}

impl SystemEvent {
    /// Format for the Network Log UI
    pub fn to_display(&self) -> String {
        match self {
            SystemEvent::SecurityDrop { peer, reason } => 
                format!("[SECURITY] Blocked {}: {}", peer, reason),
            SystemEvent::MutationCommitted { hash, author } => 
                format!("[AE] Committed {} by {}", &hash[..8], author),
            SystemEvent::StateMerged { nodes_added } => 
                format!("[SYNC] Merged {} nodes", nodes_added),
            SystemEvent::GarbageCollection { nodes_pruned, .. } => 
                format!("[GC] Pruned {} orphan nodes", nodes_pruned),
            SystemEvent::AiInference { decision, .. } => 
                format!("[AI] Decision: {}", decision),
            SystemEvent::Status(msg) => 
                format!("[SYS] {}", msg),
        }
    }
}
