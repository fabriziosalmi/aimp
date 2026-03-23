use aimp_node::crdt::consensus::QuorumManager;
use aimp_node::crdt::merkle_dag::{AiEvidence, MerkleCrdtEngine};
use aimp_node::crypto::{Identity, SecurityFirewall};
use aimp_node::protocol::{AimpData, OpCode, ProtocolParser};
use std::collections::BTreeMap;

#[test]
fn test_crdt_convergence_two_engines() {
    let mut engine_a = MerkleCrdtEngine::default();
    let mut engine_b = MerkleCrdtEngine::default();

    let identity = Identity::new();

    // Engine A: create mutation 1
    let data1 = b"mutation-from-a";
    let data_hash1 = SecurityFirewall::hash(data1);
    let sig1 = identity
        .sign(AimpData {
            v: 1,
            op: OpCode::Ping,
            ttl: 3,
            origin_pubkey: identity.node_id(),
            vclock: BTreeMap::new(),
            payload: data1.to_vec(),
        })
        .unwrap()
        .signature;
    let mut vclock1 = BTreeMap::new();
    vclock1.insert("a".to_string(), 1);

    let h1 = engine_a.append_mutation(data_hash1, sig1, vclock1.clone(), None);

    // Engine B: create mutation 2
    let data2 = b"mutation-from-b";
    let data_hash2 = SecurityFirewall::hash(data2);
    let sig2 = identity
        .sign(AimpData {
            v: 1,
            op: OpCode::Ping,
            ttl: 3,
            origin_pubkey: identity.node_id(),
            vclock: BTreeMap::new(),
            payload: data2.to_vec(),
        })
        .unwrap()
        .signature;
    let mut vclock2 = BTreeMap::new();
    vclock2.insert("b".to_string(), 1);

    let h2 = engine_b.append_mutation(data_hash2, sig2, vclock2.clone(), None);

    // Cross-merge: A gets B's node, B gets A's node
    let node1 = engine_a.arena.get_by_hash(&h1).unwrap().clone();
    let node2 = engine_b.arena.get_by_hash(&h2).unwrap().clone();

    engine_a.arena.insert(h2, node2.clone());
    engine_a.heads.insert(h2);
    engine_a.invalidate_root();

    engine_b.arena.insert(h1, node1.clone());
    engine_b.heads.insert(h1);
    engine_b.invalidate_root();

    // Both engines must have the same Merkle root
    assert_eq!(engine_a.get_merkle_root(), engine_b.get_merkle_root());
}

#[test]
fn test_security_firewall_rejects_invalid_signature() {
    let identity = Identity::new();

    let data = AimpData {
        v: 1,
        op: OpCode::Ping,
        ttl: 3,
        origin_pubkey: identity.node_id(),
        vclock: BTreeMap::new(),
        payload: b"hello".to_vec(),
    };

    let mut envelope = identity.sign(data).unwrap();

    // Valid signature should pass
    assert!(SecurityFirewall::verify(&envelope));

    // Tamper with signature
    envelope.signature[0] ^= 0xff;
    assert!(!SecurityFirewall::verify(&envelope));
}

#[test]
fn test_quorum_consensus_reaches_threshold() {
    let mut quorum = QuorumManager::new(2);

    let evidence = AiEvidence {
        prompt: "check valve status".to_string(),
        decision: "NORMAL".to_string(),
        model_hash: SecurityFirewall::hash(b"test-model"),
        timestamp: 1000,
    };

    // First node votes — threshold not yet met
    let node1_id = [1u8; 32];
    assert!(!quorum.observe(node1_id, &evidence));
    assert!(!quorum.is_verified("check valve status"));

    // Second node votes — threshold met
    let node2_id = [2u8; 32];
    assert!(quorum.observe(node2_id, &evidence));
    assert!(quorum.is_verified("check valve status"));

    // Third vote should return false (already verified)
    let node3_id = [3u8; 32];
    assert!(!quorum.observe(node3_id, &evidence));

    assert_eq!(quorum.get_support_count("check valve status", "NORMAL"), 2);
}

#[test]
fn test_protocol_roundtrip_serialize_deserialize() {
    let identity = Identity::new();

    let data = AimpData {
        v: 1,
        op: OpCode::SyncRes,
        ttl: 5,
        origin_pubkey: identity.node_id(),
        vclock: {
            let mut m = BTreeMap::new();
            m.insert("node1".to_string(), 42);
            m
        },
        payload: b"test-payload-data".to_vec(),
    };

    let envelope = identity.sign(data).unwrap();

    // Serialize
    let bytes = ProtocolParser::to_bytes(&envelope).unwrap();
    assert!(!bytes.is_empty());

    // Deserialize
    let restored = ProtocolParser::from_bytes(&bytes).unwrap();

    assert_eq!(restored.data.v, envelope.data.v);
    assert_eq!(restored.data.op, envelope.data.op);
    assert_eq!(restored.data.ttl, envelope.data.ttl);
    assert_eq!(restored.data.origin_pubkey, envelope.data.origin_pubkey);
    assert_eq!(restored.data.payload, envelope.data.payload);
    assert_eq!(restored.signature, envelope.signature);
}

#[test]
fn test_vdiff_returns_missing_nodes() {
    let mut engine = MerkleCrdtEngine::default();

    let sig = [0u8; 64];
    let mut vc = BTreeMap::new();
    vc.insert("n".to_string(), 1);

    let h1 = engine.append_mutation(SecurityFirewall::hash(b"d1"), sig, vc.clone(), None);
    vc.insert("n".to_string(), 2);
    let _h2 = engine.append_mutation(SecurityFirewall::hash(b"d2"), sig, vc, None);

    // Remote has only h1 — should get h2 as delta
    let diff = engine.get_vdiff(vec![h1]);
    assert!(!diff.is_empty());
}
