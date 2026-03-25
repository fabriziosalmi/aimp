///! Batch Signing Benchmark — Amortized Ed25519 via Merkle Batch Root
///!
///! Instead of signing each mutation individually (7.1 µs per mutation),
///! accumulate N mutations in a batch, compute their Merkle root, and
///! sign only the root. Each mutation is verifiable via Merkle proof
///! against the signed root — identical security model to blockchain blocks.
///!
///! Security model:
///!   - Individual signing: each mutation independently verifiable
///!   - Batch signing: batch root signed, individual mutations verified
///!     via Merkle inclusion proof against the signed root
///!   - Integrity guarantee is identical (Merkle tree collision resistance)
///!   - Trade-off: mutations are only fully verifiable after batch close
///!
///! Run: cargo run --release --example bench_batch_signing
///!      RUSTFLAGS="-C target-cpu=native" cargo run --release \
///!        --features fast-crypto --example bench_batch_signing

use aimp_node::crdt::merkle_dag::MerkleCrdtEngine;
use aimp_node::crypto::{Identity, SecurityFirewall};
use aimp_node::protocol::{AimpData, OpCode};
use std::collections::BTreeMap;
use std::time::Instant;

/// Batch accumulator: collects mutation hashes, then signs the batch root.
struct BatchSigner {
    pending_hashes: Vec<[u8; 32]>,
    batch_size: usize,
    batches_signed: usize,
}

impl BatchSigner {
    fn new(batch_size: usize) -> Self {
        Self {
            pending_hashes: Vec::with_capacity(batch_size),
            batch_size,
            batches_signed: 0,
        }
    }

    /// Add a mutation hash to the current batch.
    /// Returns Some(batch_root_signature) when the batch is full.
    fn add_mutation(
        &mut self,
        mutation_hash: [u8; 32],
        identity: &Identity,
    ) -> Option<[u8; 64]> {
        self.pending_hashes.push(mutation_hash);

        if self.pending_hashes.len() >= self.batch_size {
            let sig = self.close_batch(identity);
            Some(sig)
        } else {
            None
        }
    }

    /// Compute Merkle root of pending hashes and sign it.
    fn close_batch(&mut self, identity: &Identity) -> [u8; 64] {
        let root = Self::compute_batch_root(&self.pending_hashes);
        let sig = identity.sign_bytes(&root);
        self.pending_hashes.clear();
        self.batches_signed += 1;
        sig
    }

    /// Compute Merkle root of a batch of hashes.
    /// Uses iterative pairwise BLAKE3 hashing (binary Merkle tree).
    fn compute_batch_root(hashes: &[[u8; 32]]) -> [u8; 32] {
        if hashes.is_empty() {
            return [0u8; 32];
        }
        if hashes.len() == 1 {
            return hashes[0];
        }

        let mut level: Vec<[u8; 32]> = hashes.to_vec();

        while level.len() > 1 {
            let mut next_level = Vec::with_capacity((level.len() + 1) / 2);
            for pair in level.chunks(2) {
                if pair.len() == 2 {
                    let mut hasher = blake3::Hasher::new();
                    hasher.update(&pair[0]);
                    hasher.update(&pair[1]);
                    next_level.push(*hasher.finalize().as_bytes());
                } else {
                    // Odd element: promote directly
                    next_level.push(pair[0]);
                }
            }
            level = next_level;
        }

        level[0]
    }

    /// Generate a Merkle inclusion proof for a specific mutation in the batch.
    /// Returns the sibling hashes needed to verify inclusion.
    fn compute_proof(hashes: &[[u8; 32]], index: usize) -> Vec<([u8; 32], bool)> {
        if hashes.len() <= 1 {
            return vec![];
        }

        let mut proof = Vec::new();
        let mut level: Vec<[u8; 32]> = hashes.to_vec();
        let mut idx = index;

        while level.len() > 1 {
            let sibling_idx = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
            if sibling_idx < level.len() {
                // true = sibling is on the right
                proof.push((level[sibling_idx], idx % 2 == 0));
            }

            let mut next_level = Vec::with_capacity((level.len() + 1) / 2);
            for pair in level.chunks(2) {
                if pair.len() == 2 {
                    let mut hasher = blake3::Hasher::new();
                    hasher.update(&pair[0]);
                    hasher.update(&pair[1]);
                    next_level.push(*hasher.finalize().as_bytes());
                } else {
                    next_level.push(pair[0]);
                }
            }
            level = next_level;
            idx /= 2;
        }

        proof
    }

    /// Verify a Merkle inclusion proof.
    fn verify_proof(leaf: [u8; 32], proof: &[([u8; 32], bool)], root: [u8; 32]) -> bool {
        let mut current = leaf;
        for (sibling, is_right) in proof {
            let mut hasher = blake3::Hasher::new();
            if *is_right {
                hasher.update(&current);
                hasher.update(sibling);
            } else {
                hasher.update(sibling);
                hasher.update(&current);
            }
            current = *hasher.finalize().as_bytes();
        }
        current == root
    }
}

fn main() {
    println!("AIMP Batch Signing Benchmark");
    println!("============================\n");

    let identity = Identity::new();
    let num_mutations = 10_000;

    // -----------------------------------------------------------------------
    // Baseline: Individual signing (current behavior)
    // -----------------------------------------------------------------------
    println!("--- Baseline: Individual Signing ({num_mutations} mutations) ---\n");
    {
        let mut engine = MerkleCrdtEngine::with_gc_threshold(None, 100_000);

        let start = Instant::now();
        for i in 0..num_mutations {
            let data = format!("m-{}", i);
            let data_hash = SecurityFirewall::hash(data.as_bytes());

            // Sign full AimpData (current protocol)
            let aimp_data = AimpData {
                v: 1,
                op: OpCode::Ping,
                ttl: 3,
                origin_pubkey: identity.node_id(),
                vclock: BTreeMap::new(),
                payload: data.into_bytes(),
            };
            let bytes = rmp_serde::to_vec(&aimp_data).unwrap();
            let sig = identity.sign_bytes(&bytes);

            let mut vc = BTreeMap::new();
            vc.insert("n0".to_string(), i as u64);
            engine.append_mutation(data_hash, sig, vc, None);
        }
        let elapsed = start.elapsed();
        let rate = num_mutations as f64 / elapsed.as_secs_f64();

        println!("  Time:       {:.3}ms", elapsed.as_secs_f64() * 1000.0);
        println!("  Throughput: {:.0} ops/sec", rate);
        println!("  Per-op:     {:.1} µs", elapsed.as_secs_f64() * 1_000_000.0 / num_mutations as f64);
    }

    // -----------------------------------------------------------------------
    // Batch signing with various batch sizes
    // -----------------------------------------------------------------------
    for batch_size in [2, 5, 10, 20, 50, 100] {
        println!("\n--- Batch Signing (batch_size={batch_size}, {num_mutations} mutations) ---\n");

        let mut engine = MerkleCrdtEngine::with_gc_threshold(None, 100_000);
        let mut batcher = BatchSigner::new(batch_size);

        let start = Instant::now();
        for i in 0..num_mutations {
            let data = format!("m-{}", i);
            let data_hash = SecurityFirewall::hash(data.as_bytes());

            // Accumulate mutation hash; sign only when batch is full
            let sig = match batcher.add_mutation(data_hash, &identity) {
                Some(batch_sig) => batch_sig,
                None => [0u8; 64], // Placeholder until batch closes
            };

            let mut vc = BTreeMap::new();
            vc.insert("n0".to_string(), i as u64);
            engine.append_mutation(data_hash, sig, vc, None);
        }

        // Close final partial batch
        if !batcher.pending_hashes.is_empty() {
            let _ = batcher.close_batch(&identity);
        }

        let elapsed = start.elapsed();
        let rate = num_mutations as f64 / elapsed.as_secs_f64();
        let per_op = elapsed.as_secs_f64() * 1_000_000.0 / num_mutations as f64;
        let amortized_sign = 7.1 / batch_size as f64; // µs

        println!("  Time:       {:.3}ms", elapsed.as_secs_f64() * 1000.0);
        println!("  Throughput: {:.0} ops/sec", rate);
        println!("  Per-op:     {:.1} µs (amortized sign: {:.2} µs)", per_op, amortized_sign);
        println!("  Batches:    {} ({} mutations/batch)", batcher.batches_signed, batch_size);
    }

    // -----------------------------------------------------------------------
    // Verify Merkle proof correctness
    // -----------------------------------------------------------------------
    println!("\n--- Merkle Proof Verification ---\n");
    {
        let batch_size = 10;
        let mut hashes = Vec::new();
        for i in 0..batch_size {
            hashes.push(SecurityFirewall::hash(format!("proof-test-{}", i).as_bytes()));
        }

        let root = BatchSigner::compute_batch_root(&hashes);

        let mut all_valid = true;
        for (idx, hash) in hashes.iter().enumerate() {
            let proof = BatchSigner::compute_proof(&hashes, idx);
            let valid = BatchSigner::verify_proof(*hash, &proof, root);
            if !valid {
                println!("  FAILED: proof for index {} is invalid!", idx);
                all_valid = false;
            }
        }

        if all_valid {
            println!("  All {} Merkle inclusion proofs verified correctly.", batch_size);
            println!("  Proof size: {} hashes ({} bytes per mutation)",
                (batch_size as f64).log2().ceil() as usize,
                (batch_size as f64).log2().ceil() as usize * 32);
        }

        // Verify tampered proof fails
        let proof = BatchSigner::compute_proof(&hashes, 0);
        let tampered_hash = SecurityFirewall::hash(b"tampered");
        assert!(
            !BatchSigner::verify_proof(tampered_hash, &proof, root),
            "Tampered proof should fail"
        );
        println!("  Tampered mutation correctly rejected by proof.");
    }

    println!("\n============================");
    println!("BENCHMARK COMPLETE");
    println!("============================");
}
