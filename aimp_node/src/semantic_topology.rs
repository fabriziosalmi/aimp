//! AIMP v0.4.0 — Deterministic Semantic Topologies
//!
//! Automatic epistemic edge generation from quantized embeddings.
//! Transforms the knowledge graph from manual to autonomous topology.
//!
//! ## Design Rules
//! 1. **NO FLOATS in protocol core** — SimHash is pre-computed application-side;
//!    protocol only stores and compares the resulting `[u64; 4]` via Hamming distance.
//! 2. **Epoch-batch generation** — edges are generated at epoch boundaries on the
//!    complete, sorted claim set. Deterministic across all nodes.
//! 3. **max_k_nearest cap** — bounds edge count per claim, preventing O(N²) explosion
//!    in the knowledge graph.
//! 4. **Complementary to SemanticFingerprint** — fingerprint = identity (Reducer),
//!    embedding = relation (edge generation). Different pipeline stages.

use serde::{Deserialize, Serialize};

use crate::epistemic::{Claim, ClaimHash, RawEpistemicEdge, Relation, Reputation};

// ─── Quantized Embedding (SimHash 256-bit) ──────────────────

/// 256-bit SimHash of claim content in a canonical latent space.
///
/// Stored as 4 × u64 (32 bytes, same size as ClaimHash).
/// Hamming distance between two embeddings approximates the angular
/// distance in the original dense embedding space:
///   cosine_similarity ≈ (256 - hamming_distance) / 256
///
/// Construction is application-side: the agent passes claim text through
/// a protocol-mandated canonical embedding model, then applies SimHash
/// with the protocol's fixed hyperplane matrix. The protocol core never
/// touches floats — it only receives and compares the pre-computed hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QuantizedEmbedding(pub [u64; 4]);

impl QuantizedEmbedding {
    /// Hamming distance: number of differing bits (0 to 256).
    /// 0 = identical meaning, 256 = perfectly opposite.
    ///
    /// Uses hardware popcount (count_ones) — ~1 ns on modern CPUs.
    /// Deterministic, integer-only, SIMD-friendly, ZK-circuit-compatible.
    pub fn hamming_distance(&self, other: &Self) -> u32 {
        self.0
            .iter()
            .zip(other.0.iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum()
    }

    /// Create from raw u64 quadruple (for testing and deserialization).
    pub fn new(bits: [u64; 4]) -> Self {
        Self(bits)
    }

    /// All zeros (neutral embedding — maximum similarity to itself).
    pub const ZERO: Self = Self([0; 4]);

    /// All ones (bitwise opposite of ZERO — maximum distance).
    pub const ONES: Self = Self([u64::MAX; 4]);
}

// ─── Auto Edge Configuration ────────────────────────────────

/// Default Hamming distance threshold for Supports edges.
/// 30 out of 256 bits differ → ~88% cosine similarity.
pub const DEFAULT_SUPPORT_THRESHOLD: u32 = 30;

/// Default Hamming distance threshold for Contradicts edges.
/// 200 out of 256 bits differ → ~22% cosine similarity (mostly opposite).
pub const DEFAULT_CONTRADICT_THRESHOLD: u32 = 200;

/// Default maximum edges generated per claim (prevents O(N²) graph explosion).
pub const DEFAULT_MAX_K_NEAREST: usize = 10;

/// Configuration for automatic edge generation.
#[derive(Clone, Debug)]
pub struct AutoEdgeConfig {
    /// Max Hamming distance to generate a Supports edge (inclusive).
    pub support_threshold_bits: u32,
    /// Min Hamming distance to generate a Contradicts edge (inclusive).
    pub contradict_threshold_bits: u32,
    /// Max edges a single claim can generate (cap for graph density).
    pub max_k_nearest: usize,
}

impl Default for AutoEdgeConfig {
    fn default() -> Self {
        Self {
            support_threshold_bits: DEFAULT_SUPPORT_THRESHOLD,
            contradict_threshold_bits: DEFAULT_CONTRADICT_THRESHOLD,
            max_k_nearest: DEFAULT_MAX_K_NEAREST,
        }
    }
}

// ─── Auto Edge Generator ────────────────────────────────────

/// Generates epistemic edges automatically from quantized embeddings.
///
/// Called at epoch boundaries on the complete, sorted claim set.
/// Two nodes with the same claims produce byte-identical edge sets
/// (deterministic: sorted by claim ID, Hamming is commutative).
pub struct AutoEdgeGenerator {
    pub config: AutoEdgeConfig,
}

impl AutoEdgeGenerator {
    pub fn new(config: AutoEdgeConfig) -> Self {
        Self { config }
    }

    /// Generate edges for a batch of claims (epoch-aligned).
    ///
    /// Claims are sorted by ID internally for BFT determinism.
    /// Only claims with `embedding != None` participate.
    /// Returns edges in deterministic order (by from_hash, then to_hash).
    pub fn generate_edges(&self, claims: &[Claim]) -> Vec<RawEpistemicEdge> {
        // Collect claims with embeddings, sorted by ID for determinism
        let mut with_embedding: Vec<&Claim> =
            claims.iter().filter(|c| c.embedding.is_some()).collect();
        with_embedding.sort_by_key(|c| c.id);

        if with_embedding.len() < 2 {
            return Vec::new();
        }

        let mut edges = Vec::new();
        // Track per-claim edge count for max_k_nearest cap
        let mut edge_counts: std::collections::HashMap<ClaimHash, usize> =
            std::collections::HashMap::new();

        for i in 0..with_embedding.len() {
            let c1 = with_embedding[i];
            let e1 = c1.embedding.unwrap(); // safe: filtered above

            for c2 in with_embedding.iter().skip(i + 1).copied() {
                // v0.4.0: Only compare claims with the same embedding version.
                // Different versions use different canonical models / hyperplanes
                // and produce incomparable SimHash values.
                if c1.embedding_version != c2.embedding_version {
                    continue;
                }

                let e2 = c2.embedding.unwrap();

                // Check if both claims have hit their edge cap
                let count1 = edge_counts.get(&c1.id).copied().unwrap_or(0);
                let count2 = edge_counts.get(&c2.id).copied().unwrap_or(0);
                if count1 >= self.config.max_k_nearest || count2 >= self.config.max_k_nearest {
                    continue;
                }

                let distance = e1.hamming_distance(&e2);

                let relation = if distance <= self.config.support_threshold_bits {
                    Some(Relation::Supports)
                } else if distance >= self.config.contradict_threshold_bits {
                    Some(Relation::Contradicts)
                } else {
                    None // Dead zone: insufficient signal
                };

                if let Some(rel) = relation {
                    // Edge strength: inversely proportional to distance for Supports,
                    // proportional for Contradicts. Clamped to [1000, 10000] bps.
                    let strength_bps = match rel {
                        Relation::Supports => {
                            // distance=0 → 10000, distance=threshold → 1000
                            let t = self.config.support_threshold_bits.max(1);
                            let raw = 10000u32.saturating_sub(distance * 9000 / t);
                            raw.clamp(1000, 10000) as u16
                        }
                        Relation::Contradicts => {
                            // distance=256 → 10000, distance=threshold → 1000
                            let range = 256u32
                                .saturating_sub(self.config.contradict_threshold_bits)
                                .max(1);
                            let above =
                                distance.saturating_sub(self.config.contradict_threshold_bits);
                            let raw = 1000 + above * 9000 / range;
                            raw.clamp(1000, 10000) as u16
                        }
                        _ => 5000,
                    };

                    edges.push(RawEpistemicEdge {
                        from_hash: c1.id,
                        from_fingerprint: c1.fingerprint,
                        to_hash: c2.id,
                        to_fingerprint: c2.fingerprint,
                        relation: rel,
                        strength: Reputation::from_bps(strength_bps),
                    });

                    *edge_counts.entry(c1.id).or_insert(0) += 1;
                    *edge_counts.entry(c2.id).or_insert(0) += 1;
                }
            }
        }

        edges
    }
}

// ─── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epistemic::{ClaimKind, CorrelationCell, LogOdds, SemanticFingerprint};

    fn make_embedding(seed: u64) -> QuantizedEmbedding {
        // Deterministic embedding from seed using BLAKE3
        let hash = blake3::hash(&seed.to_le_bytes());
        let bytes = hash.as_bytes();
        QuantizedEmbedding([
            u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
            u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
            u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        ])
    }

    /// Create an embedding that is `flipped_bits` away from `base`.
    fn make_nearby_embedding(base: &QuantizedEmbedding, flipped_bits: u32) -> QuantizedEmbedding {
        let mut result = *base;
        let mut remaining = flipped_bits;
        for word in result.0.iter_mut() {
            let to_flip = remaining.min(64);
            // Flip the lowest `to_flip` bits
            let mask = if to_flip >= 64 {
                u64::MAX
            } else {
                (1u64 << to_flip) - 1
            };
            *word ^= mask;
            remaining = remaining.saturating_sub(64);
            if remaining == 0 {
                break;
            }
        }
        result
    }

    fn make_fingerprint(data: &[u8]) -> SemanticFingerprint {
        let hash = blake3::hash(data);
        let mut primary = [0u8; 16];
        primary.copy_from_slice(&hash.as_bytes()[..16]);
        SemanticFingerprint {
            primary,
            secondary: u64::from_le_bytes(hash.as_bytes()[16..24].try_into().unwrap()),
        }
    }

    fn make_test_claim(data: &[u8], tick: u64, embedding: Option<QuantizedEmbedding>) -> Claim {
        let fp = make_fingerprint(data);
        let mut source = [0u8; 32];
        source[..data.len().min(32)].copy_from_slice(&data[..data.len().min(32)]);

        let mut hasher = blake3::Hasher::new();
        hasher.update(&fp.primary);
        hasher.update(&tick.to_le_bytes());
        hasher.update(&source);
        if let Some(e) = &embedding {
            for word in &e.0 {
                hasher.update(&word.to_le_bytes());
            }
        }
        let id = *hasher.finalize().as_bytes();

        Claim {
            id,
            fingerprint: fp,
            origin: [1u8; 32],
            kind: ClaimKind::Observation {
                sensor_type: 1,
                data: data.to_vec(),
            },
            confidence: LogOdds::new(2000),
            evidence_source: source,
            tick,
            correlation_cell: None,
            embedding,
            embedding_version: 1, // default test version
        }
    }

    // ── Hamming distance tests ──

    #[test]
    fn test_hamming_distance_identical() {
        let e = make_embedding(42);
        assert_eq!(e.hamming_distance(&e), 0);
    }

    #[test]
    fn test_hamming_distance_opposite() {
        let a = QuantizedEmbedding::ZERO;
        let b = QuantizedEmbedding::ONES;
        assert_eq!(a.hamming_distance(&b), 256);
    }

    #[test]
    fn test_hamming_distance_single_bit() {
        let a = QuantizedEmbedding::new([0, 0, 0, 0]);
        let b = QuantizedEmbedding::new([1, 0, 0, 0]);
        assert_eq!(a.hamming_distance(&b), 1);
    }

    #[test]
    fn test_hamming_distance_symmetric() {
        let a = make_embedding(1);
        let b = make_embedding(2);
        assert_eq!(a.hamming_distance(&b), b.hamming_distance(&a));
    }

    #[test]
    fn test_hamming_distance_known_flips() {
        let base = make_embedding(100);
        let nearby = make_nearby_embedding(&base, 15);
        assert_eq!(base.hamming_distance(&nearby), 15);
    }

    #[test]
    fn test_hamming_distance_range() {
        // Random embeddings should have ~128 bits different (binomial mean)
        let a = make_embedding(1);
        let b = make_embedding(2);
        let d = a.hamming_distance(&b);
        assert!(
            d > 80 && d < 180,
            "random embeddings should be ~128 apart, got {}",
            d
        );
    }

    // ── Auto edge generation tests ──

    #[test]
    fn test_auto_edge_supports() {
        let base = make_embedding(42);
        let nearby = make_nearby_embedding(&base, 10); // 10 bits apart < 30 threshold

        let c1 = make_test_claim(b"sensor_a", 0, Some(base));
        let c2 = make_test_claim(b"sensor_b", 1, Some(nearby));

        let gen = AutoEdgeGenerator::new(AutoEdgeConfig::default());
        let edges = gen.generate_edges(&[c1, c2]);

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].relation, Relation::Supports);
        assert!(
            edges[0].strength.bps() > 5000,
            "close distance should produce high strength"
        );
    }

    #[test]
    fn test_auto_edge_contradicts() {
        let base = make_embedding(42);
        let far = make_nearby_embedding(&base, 220); // 220 bits apart > 200 threshold

        let c1 = make_test_claim(b"sensor_a", 0, Some(base));
        let c2 = make_test_claim(b"sensor_b", 1, Some(far));

        let gen = AutoEdgeGenerator::new(AutoEdgeConfig::default());
        let edges = gen.generate_edges(&[c1, c2]);

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].relation, Relation::Contradicts);
    }

    #[test]
    fn test_auto_edge_dead_zone() {
        let base = make_embedding(42);
        let mid = make_nearby_embedding(&base, 100); // 100 bits: in dead zone (30 < 100 < 200)

        let c1 = make_test_claim(b"sensor_a", 0, Some(base));
        let c2 = make_test_claim(b"sensor_b", 1, Some(mid));

        let gen = AutoEdgeGenerator::new(AutoEdgeConfig::default());
        let edges = gen.generate_edges(&[c1, c2]);

        assert!(edges.is_empty(), "dead zone should produce no edges");
    }

    #[test]
    fn test_auto_edge_deterministic() {
        let base = make_embedding(42);
        let nearby = make_nearby_embedding(&base, 10);

        let c1 = make_test_claim(b"sensor_a", 0, Some(base));
        let c2 = make_test_claim(b"sensor_b", 1, Some(nearby));

        let gen = AutoEdgeGenerator::new(AutoEdgeConfig::default());

        // Forward order
        let edges_fwd = gen.generate_edges(&[c1.clone(), c2.clone()]);
        // Reverse order
        let edges_rev = gen.generate_edges(&[c2.clone(), c1.clone()]);

        assert_eq!(edges_fwd.len(), edges_rev.len());
        assert_eq!(edges_fwd[0].from_hash, edges_rev[0].from_hash);
        assert_eq!(edges_fwd[0].to_hash, edges_rev[0].to_hash);
        assert_eq!(edges_fwd[0].relation, edges_rev[0].relation);
    }

    #[test]
    fn test_auto_edge_no_embedding_skipped() {
        let c1 = make_test_claim(b"with_emb", 0, Some(make_embedding(1)));
        let c2 = make_test_claim(b"no_emb", 1, None);

        let gen = AutoEdgeGenerator::new(AutoEdgeConfig::default());
        let edges = gen.generate_edges(&[c1, c2]);

        assert!(
            edges.is_empty(),
            "claim without embedding should be skipped"
        );
    }

    #[test]
    fn test_auto_edge_max_k_cap() {
        let base = make_embedding(42);
        // Create 20 claims all very close to base (within support threshold)
        let claims: Vec<Claim> = (0..20u64)
            .map(|i| {
                let emb = make_nearby_embedding(&base, (i % 5) as u32); // 0-4 bits apart
                make_test_claim(format!("sensor_{}", i).as_bytes(), i, Some(emb))
            })
            .collect();

        let config = AutoEdgeConfig {
            max_k_nearest: 5,
            ..Default::default()
        };
        let gen = AutoEdgeGenerator::new(config);
        let edges = gen.generate_edges(&claims);

        // With max_k=5, no claim should have more than 5 edges
        let mut per_claim: std::collections::HashMap<ClaimHash, usize> =
            std::collections::HashMap::new();
        for e in &edges {
            *per_claim.entry(e.from_hash).or_insert(0) += 1;
            *per_claim.entry(e.to_hash).or_insert(0) += 1;
        }
        for (id, count) in &per_claim {
            assert!(
                *count <= 5,
                "claim {:?} has {} edges, max_k=5",
                &id[..4],
                count
            );
        }
    }

    #[test]
    fn test_auto_edge_strength_gradient() {
        let base = make_embedding(42);
        let very_close = make_nearby_embedding(&base, 2);
        let barely_supports = make_nearby_embedding(&base, 28);

        let c0 = make_test_claim(b"base", 0, Some(base));
        let c1 = make_test_claim(b"close", 1, Some(very_close));
        let c2 = make_test_claim(b"barely", 2, Some(barely_supports));

        let gen = AutoEdgeGenerator::new(AutoEdgeConfig::default());
        let edges_close = gen.generate_edges(&[c0.clone(), c1]);
        let edges_far = gen.generate_edges(&[c0, c2]);

        assert!(!edges_close.is_empty());
        assert!(!edges_far.is_empty());
        // Very close should have higher strength than barely-supports
        assert!(
            edges_close[0].strength.bps() > edges_far[0].strength.bps(),
            "closer distance should produce stronger edge: close={}, far={}",
            edges_close[0].strength.bps(),
            edges_far[0].strength.bps()
        );
    }

    #[test]
    fn test_auto_edge_empty_input() {
        let gen = AutoEdgeGenerator::new(AutoEdgeConfig::default());
        assert!(gen.generate_edges(&[]).is_empty());
    }

    #[test]
    fn test_auto_edge_single_claim() {
        let c = make_test_claim(b"alone", 0, Some(make_embedding(1)));
        let gen = AutoEdgeGenerator::new(AutoEdgeConfig::default());
        assert!(gen.generate_edges(&[c]).is_empty());
    }

    #[test]
    fn test_embedding_version_mismatch_no_edges() {
        let base = make_embedding(42);
        let nearby = make_nearby_embedding(&base, 5); // very close

        let mut c1 = make_test_claim(b"sensor_a", 0, Some(base));
        let mut c2 = make_test_claim(b"sensor_b", 1, Some(nearby));

        // Different embedding versions → incomparable
        c1.embedding_version = 1;
        c2.embedding_version = 2;

        let gen = AutoEdgeGenerator::new(AutoEdgeConfig::default());
        let edges = gen.generate_edges(&[c1, c2]);

        assert!(
            edges.is_empty(),
            "different embedding versions must not produce edges"
        );
    }

    #[test]
    fn test_embedding_version_same_produces_edges() {
        let base = make_embedding(42);
        let nearby = make_nearby_embedding(&base, 5);

        let mut c1 = make_test_claim(b"sensor_a", 0, Some(base));
        let mut c2 = make_test_claim(b"sensor_b", 1, Some(nearby));

        c1.embedding_version = 1;
        c2.embedding_version = 1;

        let gen = AutoEdgeGenerator::new(AutoEdgeConfig::default());
        let edges = gen.generate_edges(&[c1, c2]);

        assert_eq!(
            edges.len(),
            1,
            "same version + close distance → Supports edge"
        );
    }
}
