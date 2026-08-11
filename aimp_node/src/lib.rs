pub mod config;
pub mod crdt;
pub mod crypto;
// The terminal dashboard is part of the node application, not the protocol.
// It is the sole consumer of ratatui + crossterm (68 crates between them), so
// library consumers embedding AIMP do not pay for a TUI they never render.
#[cfg(feature = "cli")]
pub mod dashboard;
pub mod decision_engine;
pub mod epistemic;
pub mod error;
pub mod event;
pub mod network;
pub mod protocol;
pub mod semantic_topology;

pub use error::{AimpError, AimpResult};
