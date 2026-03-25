use serde::{Deserialize, Serialize};

pub mod metrics;

/// Algebraic data type representing all structured system events.
///
/// Replaces free-form string logging with machine-queryable variants,
/// enabling structured observability across the mesh node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemEvent {
    /// A packet was dropped by the security firewall.
    SecurityDrop { peer: String, reason: String },

    /// A new mutation was committed to the Merkle-DAG.
    MutationCommitted { hash: String, author: String },

    /// Remote state was merged via anti-entropy sync.
    StateMerged { nodes_added: usize },

    /// Epoch GC compacted the DAG history.
    GarbageCollection {
        nodes_pruned: usize,
        remaining: usize,
    },

    /// The deterministic AI engine produced a decision.
    AiInference { prompt: String, decision: String },

    /// General operational status message.
    Status(String),
}

impl SystemEvent {
    /// Format this event for display in the TUI network log.
    pub fn to_display(&self) -> String {
        match self {
            SystemEvent::SecurityDrop { peer, reason } => {
                format!("[SECURITY] Blocked {}: {}", peer, reason)
            }
            SystemEvent::MutationCommitted { hash, author } => {
                format!("[AE] Committed {} by {}", &hash[..8], author)
            }
            SystemEvent::StateMerged { nodes_added } => {
                format!("[SYNC] Merged {} nodes", nodes_added)
            }
            SystemEvent::GarbageCollection { nodes_pruned, .. } => {
                format!("[GC] Pruned {} orphan nodes", nodes_pruned)
            }
            SystemEvent::AiInference { decision, .. } => format!("[AI] Decision: {}", decision),
            SystemEvent::Status(msg) => format!("[SYS] {}", msg),
        }
    }
}
