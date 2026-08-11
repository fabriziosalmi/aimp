//! Acceptance test for library consumers embedding AIMP with
//! `default-features = false` (e.g. zion's `sovereign-aimp` feature).
//!
//! Pins the exact surface such a consumer imports. If a future refactor moves
//! any of it behind `cli`, this fails here instead of in a downstream repo.

use aimp_node::crypto::Identity;
use aimp_node::crypto::SecurityFirewall;
use aimp_node::protocol::envelope::{AimpData, AimpEnvelope, OpCode};

#[test]
fn protocol_surface_is_reachable_without_default_features() {
    // Identity: the Ed25519 seed round-trip an embedder needs to persist a
    // stable node id across restarts.
    let id = Identity::new();
    let restored = Identity::from_secret_bytes(id.secret_bytes());
    assert_eq!(
        id.node_id(),
        restored.node_id(),
        "identity must survive a seed round-trip"
    );

    // Envelope types must stay nameable, and the firewall entry point callable.
    let _: fn(&AimpEnvelope) -> bool = SecurityFirewall::verify;
    let _ = OpCode::Infer;
    let _ = std::mem::size_of::<AimpEnvelope>();
    let _ = std::mem::size_of::<AimpData>();
}
