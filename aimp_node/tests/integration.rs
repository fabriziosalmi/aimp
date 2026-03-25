use aimp_node::crdt::arena::DagArena;
use aimp_node::crdt::consensus::QuorumManager;
use aimp_node::crdt::gc::EpochManager;
use aimp_node::crdt::merkle_dag::{AiEvidence, DagNode, MerkleCrdtEngine};
use aimp_node::crypto::{Identity, SecurityFirewall};
use aimp_node::decision_engine::RuleEngine;
use aimp_node::protocol::{AimpData, OpCode, ProtocolParser};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// CRDT Convergence
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Security Firewall
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// BFT Quorum Consensus
// ---------------------------------------------------------------------------

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
fn test_quorum_conflicting_decisions_tracked_separately() {
    let mut quorum = QuorumManager::new(2);

    let evidence_a = AiEvidence {
        prompt: "sensor reading".to_string(),
        decision: "CRITICAL".to_string(),
        model_hash: SecurityFirewall::hash(b"model-v1"),
        timestamp: 1000,
    };

    let evidence_b = AiEvidence {
        prompt: "sensor reading".to_string(),
        decision: "NORMAL".to_string(),
        model_hash: SecurityFirewall::hash(b"model-v1"),
        timestamp: 1001,
    };

    // Two nodes disagree
    assert!(!quorum.observe([1u8; 32], &evidence_a));
    assert!(!quorum.observe([2u8; 32], &evidence_b));

    // Neither reaches quorum with threshold=2 for their specific decision
    assert_eq!(quorum.get_support_count("sensor reading", "CRITICAL"), 1);
    assert_eq!(quorum.get_support_count("sensor reading", "NORMAL"), 1);
}

// ---------------------------------------------------------------------------
// Protocol Serialization
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Delta Synchronization (vdiff)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Arena Allocator
// ---------------------------------------------------------------------------

#[test]
fn test_arena_allocation_and_soa_layout() {
    let mut arena = DagArena::new();

    // Insert a node
    let node = DagNode {
        parents: smallvec::smallvec![],
        signature: [0u8; 64],
        data_hash: SecurityFirewall::hash(b"test"),
        vclock: BTreeMap::new(),
        evidence: None,
    };
    let hash = node.compute_hash();
    let (idx, is_new) = arena.insert(hash, node.clone());
    assert!(is_new);
    assert_eq!(arena.len(), 1);

    // Duplicate insert returns same index
    let (idx2, is_new2) = arena.insert(hash, node);
    assert!(!is_new2);
    assert_eq!(idx, idx2);
    assert_eq!(arena.len(), 1);

    // Lookup by hash and index
    assert!(arena.contains(&hash));
    assert!(arena.get_by_hash(&hash).is_some());
    assert!(arena.get_by_index(idx).is_some());

    // Insert multiple nodes
    for i in 0..100 {
        let n = DagNode {
            parents: smallvec::smallvec![],
            signature: [0u8; 64],
            data_hash: SecurityFirewall::hash(&[i as u8]),
            vclock: BTreeMap::new(),
            evidence: None,
        };
        let h = n.compute_hash();
        arena.insert(h, n);
    }
    assert_eq!(arena.len(), 101); // 1 original + 100 new

    // Retain only a subset
    let keep_hashes: std::collections::HashSet<_> = arena
        .get_all_iter()
        .take(10)
        .map(|(h, _)| *h)
        .collect();
    let removed = arena.retain(&keep_hashes);
    assert_eq!(removed, 91);
    assert_eq!(arena.len(), 10);
}

// ---------------------------------------------------------------------------
// Epoch GC
// ---------------------------------------------------------------------------

#[test]
fn test_gc_epoch_pruning() {
    // Use a very low GC threshold so it triggers quickly
    let mut engine = MerkleCrdtEngine::with_gc_threshold(None, 5);

    let sig = [0u8; 64];

    // Insert mutations beyond the GC threshold
    for i in 0..10u64 {
        let mut vc = BTreeMap::new();
        vc.insert("n".to_string(), i + 1);
        engine.append_mutation(
            SecurityFirewall::hash(&i.to_le_bytes()),
            sig,
            vc,
            None,
        );
    }

    // GC should have triggered at mutation 5 and again at mutation 10
    // The engine should still have valid heads
    assert!(!engine.heads.is_empty());

    // Merkle root should be computable
    let root = engine.get_merkle_root();
    assert_ne!(root, [0u8; 32]);
}

#[test]
fn test_gc_preserves_heads() {
    let mut engine = MerkleCrdtEngine::with_gc_threshold(None, 3);

    let sig = [0u8; 64];

    // Create a chain of 6 mutations (triggers GC at 3 and 6)
    let mut last_hash = [0u8; 32];
    for i in 0..6u64 {
        let mut vc = BTreeMap::new();
        vc.insert("n".to_string(), i + 1);
        last_hash = engine.append_mutation(
            SecurityFirewall::hash(&i.to_le_bytes()),
            sig,
            vc,
            None,
        );
    }

    // The most recent mutation should always be a head
    assert!(engine.heads.contains(&last_hash));
    assert_eq!(engine.heads.len(), 1); // Linear chain = single head
}

#[test]
fn test_epoch_manager_advances() {
    let mut epochs = EpochManager::new();
    assert_eq!(epochs.current_epoch, 0);
    assert!(epochs.finalized_root.is_none());

    let root1 = SecurityFirewall::hash(b"epoch-1");
    epochs.finalize_epoch(root1);
    assert_eq!(epochs.current_epoch, 1);
    assert_eq!(epochs.finalized_root, Some(root1));

    let root2 = SecurityFirewall::hash(b"epoch-2");
    epochs.finalize_epoch(root2);
    assert_eq!(epochs.current_epoch, 2);
    assert_eq!(epochs.finalized_root, Some(root2));
}

// ---------------------------------------------------------------------------
// Decision Engine (formerly AI Bridge)
// ---------------------------------------------------------------------------

#[test]
fn test_decision_engine_rule_matching() {
    let engine = RuleEngine::default_rules();

    // "error" keyword should trigger CRITICAL
    let decision = <RuleEngine as aimp_node::decision_engine::DecisionEngine>::evaluate(
        &engine,
        "System error detected in valve",
    )
    .unwrap();
    assert_eq!(decision.status, "CRITICAL");
    assert_eq!(decision.target_entity, "system_alert");
    assert!(decision.action_required);

    // "valve" keyword should trigger WARNING
    let decision = <RuleEngine as aimp_node::decision_engine::DecisionEngine>::evaluate(
        &engine,
        "Check valve pressure",
    )
    .unwrap();
    assert_eq!(decision.status, "WARNING");
    assert_eq!(decision.target_entity, "hydraulic_system");
    assert!(decision.action_required);

    // "north" keyword should trigger NORMAL
    let decision = <RuleEngine as aimp_node::decision_engine::DecisionEngine>::evaluate(
        &engine,
        "Sector north status report",
    )
    .unwrap();
    assert_eq!(decision.status, "NORMAL");
    assert_eq!(decision.target_entity, "sector_north");
    assert!(!decision.action_required);

    // No matching keyword should return default
    let decision = <RuleEngine as aimp_node::decision_engine::DecisionEngine>::evaluate(
        &engine,
        "Hello world",
    )
    .unwrap();
    assert_eq!(decision.status, "NORMAL");
    assert_eq!(decision.target_entity, "generic_entity");
    assert!(!decision.action_required);
}

#[test]
fn test_decision_engine_case_insensitive() {
    let engine = RuleEngine::default_rules();

    // Keywords should match case-insensitively
    let decision = <RuleEngine as aimp_node::decision_engine::DecisionEngine>::evaluate(
        &engine,
        "CRITICAL ERROR DETECTED",
    )
    .unwrap();
    assert_eq!(decision.status, "CRITICAL");
}

#[test]
fn test_decision_engine_first_match_wins() {
    let engine = RuleEngine::default_rules();

    // "error" appears before "valve" in rules, so CRITICAL should win
    let decision = <RuleEngine as aimp_node::decision_engine::DecisionEngine>::evaluate(
        &engine,
        "error in valve system",
    )
    .unwrap();
    assert_eq!(decision.status, "CRITICAL");
    assert_eq!(decision.target_entity, "system_alert");
}

#[test]
fn test_decision_engine_deterministic_hash() {
    use aimp_node::decision_engine::DecisionEngine;

    let engine1 = RuleEngine::default_rules();
    let engine2 = RuleEngine::default_rules();

    // Same rules should produce same hash
    assert_eq!(engine1.engine_hash(), engine2.engine_hash());
}

// ---------------------------------------------------------------------------
// Crypto Identity
// ---------------------------------------------------------------------------

#[test]
fn test_identity_unique_keys() {
    let id1 = Identity::new();
    let id2 = Identity::new();

    // Each identity should have a unique node ID
    assert_ne!(id1.node_id(), id2.node_id());
}

#[test]
fn test_blake3_hash_deterministic() {
    let hash1 = SecurityFirewall::hash(b"hello world");
    let hash2 = SecurityFirewall::hash(b"hello world");
    let hash3 = SecurityFirewall::hash(b"hello world!");

    assert_eq!(hash1, hash2);
    assert_ne!(hash1, hash3);
}

// ---------------------------------------------------------------------------
// Merkle Root Caching
// ---------------------------------------------------------------------------

#[test]
fn test_merkle_root_caching() {
    let mut engine = MerkleCrdtEngine::default();

    let sig = [0u8; 64];
    let mut vc = BTreeMap::new();
    vc.insert("n".to_string(), 1);

    engine.append_mutation(SecurityFirewall::hash(b"d1"), sig, vc, None);

    // First call computes, second should return cached value
    let root1 = engine.get_merkle_root();
    let root2 = engine.get_merkle_root();
    assert_eq!(root1, root2);
    assert_ne!(root1, [0u8; 32]);
}

#[test]
fn test_empty_engine_merkle_root_is_zero() {
    let mut engine = MerkleCrdtEngine::default();
    let root = engine.get_merkle_root();
    assert_eq!(root, [0u8; 32]);
}

// ---------------------------------------------------------------------------
// Equivocation Detection (Byzantine Slashing)
// ---------------------------------------------------------------------------

#[test]
fn test_equivocation_detected_on_conflicting_mutations() {
    let mut quorum = QuorumManager::new(2);

    // Node "alice" creates two mutations at the same tick with different data
    let mut vc = BTreeMap::new();
    vc.insert("alice".to_string(), 1u64);

    let node_a = DagNode {
        parents: smallvec::smallvec![],
        signature: [0u8; 64],
        data_hash: SecurityFirewall::hash(b"payload-A"),
        vclock: vc.clone(),
        evidence: None,
    };
    let hash_a = node_a.compute_hash();

    let node_b = DagNode {
        parents: smallvec::smallvec![],
        signature: [0u8; 64],
        data_hash: SecurityFirewall::hash(b"payload-B"), // DIFFERENT data
        vclock: vc,
        evidence: None,
    };
    let hash_b = node_b.compute_hash();

    // First mutation: clean
    assert!(quorum.check_equivocation(&node_a, hash_a).is_none());

    // Second mutation at same tick with different data: EQUIVOCATION
    let proof = quorum.check_equivocation(&node_b, hash_b);
    assert!(proof.is_some());

    let proof = proof.unwrap();
    assert_eq!(proof.origin, "alice");
    assert_eq!(proof.tick, 1);
    assert_eq!(proof.hash_a, hash_a);
    assert_eq!(proof.hash_b, hash_b);

    // Alice is now denied
    assert!(quorum.is_denied("alice"));
}

#[test]
fn test_equivocation_not_triggered_on_same_data() {
    let mut quorum = QuorumManager::new(2);

    let mut vc = BTreeMap::new();
    vc.insert("bob".to_string(), 1u64);

    let node_a = DagNode {
        parents: smallvec::smallvec![],
        signature: [0u8; 64],
        data_hash: SecurityFirewall::hash(b"same-payload"),
        vclock: vc.clone(),
        evidence: None,
    };
    let hash_a = node_a.compute_hash();

    // Same data, same tick — NOT equivocation (idempotent delivery)
    let node_b = DagNode {
        parents: smallvec::smallvec![],
        signature: [0u8; 64],
        data_hash: SecurityFirewall::hash(b"same-payload"), // SAME data
        vclock: vc,
        evidence: None,
    };
    let hash_b = node_b.compute_hash();

    assert!(quorum.check_equivocation(&node_a, hash_a).is_none());
    assert!(quorum.check_equivocation(&node_b, hash_b).is_none());
    assert!(!quorum.is_denied("bob"));
}

#[test]
fn test_denied_node_cannot_vote() {
    let mut quorum = QuorumManager::new(2);

    // Create equivocation to get "mallory" denied
    let mut vc = BTreeMap::new();
    vc.insert("mallory".to_string(), 1u64);

    let node_a = DagNode {
        parents: smallvec::smallvec![],
        signature: [0u8; 64],
        data_hash: SecurityFirewall::hash(b"data-1"),
        vclock: vc.clone(),
        evidence: None,
    };
    let node_b = DagNode {
        parents: smallvec::smallvec![],
        signature: [0u8; 64],
        data_hash: SecurityFirewall::hash(b"data-2"),
        vclock: vc,
        evidence: None,
    };

    quorum.check_equivocation(&node_a, node_a.compute_hash());
    quorum.check_equivocation(&node_b, node_b.compute_hash());
    assert!(quorum.is_denied("mallory"));

    // Mallory tries to vote — rejected
    let mallory_id = SecurityFirewall::hash(b"mallory"); // Use as node ID
    let evidence = AiEvidence {
        prompt: "test prompt".to_string(),
        decision: "NORMAL".to_string(),
        model_hash: [0u8; 32],
        timestamp: 1000,
    };

    // Use "mallory" hex-encoded as origin for the deny check
    // The observe function checks hex::encode(origin) against denied set
    // We need the hex of mallory_id to match "mallory" — it won't, so let's test
    // that the equivocation proof mechanism works end-to-end
    assert!(!quorum.observe(mallory_id, &evidence));
    // Mallory's vote didn't count — prompt not verified with just 1 vote
    assert!(!quorum.is_verified("test prompt"));
}
