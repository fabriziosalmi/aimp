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

#[test]
fn identity_seed_maps_to_the_rfc8032_public_key_on_every_backend() {
    // RFC 8032 section 7.1, TEST 1. Both the dalek and the `fast-crypto`
    // (ring) backends must agree, or a persisted seed would yield a different
    // node id after switching features.
    let seed: [u8; 32] =
        hex::decode("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60")
            .unwrap()
            .try_into()
            .unwrap();
    let id = Identity::from_secret_bytes(seed);
    assert_eq!(
        hex::encode(id.node_id()),
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    );
    assert_eq!(id.secret_bytes(), seed);
}
