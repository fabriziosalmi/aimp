use aimp_node::crdt::merkle_dag::MerkleCrdtEngine;
use aimp_node::crypto::{Identity, SecurityFirewall};
use aimp_node::protocol::{AimpData, OpCode, ProtocolParser};
use criterion::{criterion_group, criterion_main, Criterion};
use std::collections::BTreeMap;

fn bench_append_mutation(c: &mut Criterion) {
    c.bench_function("append_mutation", |b| {
        let identity = Identity::new();
        let sig = identity
            .sign(AimpData {
                v: 1,
                op: OpCode::Ping,
                ttl: 3,
                origin_pubkey: identity.node_id(),
                vclock: BTreeMap::new(),
                payload: b"bench".to_vec(),
            })
            .unwrap()
            .signature;

        b.iter(|| {
            let mut engine = MerkleCrdtEngine::default();
            let mut vc = BTreeMap::new();
            for i in 0u64..100 {
                vc.insert("n".to_string(), i);
                let hash = SecurityFirewall::hash(&i.to_le_bytes());
                engine.append_mutation(hash, sig, vc.clone(), None);
            }
        });
    });
}

fn bench_merkle_root(c: &mut Criterion) {
    let mut engine = MerkleCrdtEngine::default();
    let sig = [0u8; 64];
    for i in 0u64..50 {
        let mut vc = BTreeMap::new();
        vc.insert(format!("n{}", i), 1);
        engine.append_mutation(SecurityFirewall::hash(&i.to_le_bytes()), sig, vc, None);
    }

    c.bench_function("merkle_root_50_heads", |b| {
        b.iter(|| engine.get_merkle_root());
    });
}

fn bench_blake3_hash(c: &mut Criterion) {
    let data = vec![0u8; 1024];
    c.bench_function("blake3_hash_1kb", |b| {
        b.iter(|| SecurityFirewall::hash(&data));
    });
}

fn bench_serialize_envelope(c: &mut Criterion) {
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
        payload: vec![0u8; 256],
    };
    let envelope = identity.sign(data).unwrap();

    c.bench_function("serialize_envelope", |b| {
        b.iter(|| ProtocolParser::to_bytes(&envelope).unwrap());
    });

    let bytes = ProtocolParser::to_bytes(&envelope).unwrap();
    c.bench_function("deserialize_envelope", |b| {
        b.iter(|| ProtocolParser::from_bytes(&bytes).unwrap());
    });
}

fn bench_ed25519_sign_verify(c: &mut Criterion) {
    let identity = Identity::new();
    let data = AimpData {
        v: 1,
        op: OpCode::Ping,
        ttl: 3,
        origin_pubkey: identity.node_id(),
        vclock: BTreeMap::new(),
        payload: b"benchmark-payload".to_vec(),
    };

    c.bench_function("ed25519_sign", |b| {
        b.iter(|| identity.sign(data.clone()).unwrap());
    });

    let envelope = identity.sign(data).unwrap();
    c.bench_function("ed25519_verify", |b| {
        b.iter(|| SecurityFirewall::verify(&envelope));
    });
}

criterion_group!(
    benches,
    bench_append_mutation,
    bench_merkle_root,
    bench_blake3_hash,
    bench_serialize_envelope,
    bench_ed25519_sign_verify,
);
criterion_main!(benches);
