# For ChatGPT — AIMP v0.2.0 Cognitive Layer Review

You previously reviewed AIMP and correctly identified the missing L3 (Meaning) layer.
Your insights on Epistemic Layer, Intent CRDT, and Semantic Compaction were spot-on.

We've now designed and prototyped the v0.2.0 Cognitive Layer based on your suggestions.

## What we built (in `epistemic.rs`):

1. **Claim type** — Every payload becomes a typed knowledge claim with:
   - `origin` (Ed25519 pubkey), `confidence` (f32), `evidence_hash` (BLAKE3)
   - `ClaimKind`: Observation, Inference, Intent, or Summary

2. **SemanticReducer trait** — Compacts redundant claims into summaries
   - Default impl: ExactMatchReducer with Bayesian confidence aggregation
   - `group_key()` for deterministic grouping

3. **IntentResolver trait** — Resolves conflicting agent intents
   - Default impl: PriorityIntentResolver (highest confidence wins, tick tiebreak)

4. **RelevanceScorer trait** — Semantic GC based on relevance, not just age
   - Default impl: TimeDecayScorer with exponential half-life
   - Referenced claims are always protected (score = 1.0)

5. **8 unit tests** — All passing

## Architecture invariants we enforce:
- L3 NEVER blocks L2 (CRDT merges at full speed regardless)
- L3 is OPTIONAL (v0.1.0 compatibility preserved)
- All semantic ops must be DETERMINISTIC (for BFT consensus)
- Zero new crypto primitives (reuse Ed25519 + BLAKE3)
- All components are pluggable traits

## Open questions for you:

1. **Floating-point determinism**: Cosine similarity of embeddings is non-deterministic
   across ARM/x86. How do you propose handling this for BFT consensus?
   Our current thinking: quantized integer embeddings or Wasm sandbox.

2. **Reducer trigger strategy**: Should reduction run (a) on GC epoch,
   (b) lazily on read, or (c) on a background thread? Trade-offs?

3. **Intent conflict model**: We implemented priority-based. Would you design
   a game-theoretic or voting-based resolver instead? How?

4. **The Epistemic Layer you envisioned had `context` in Claims**.
   How would you represent context without making Claims too heavy?
   (Current Claim is ~150 bytes; v0.1.0 raw payload was ~64 bytes)

5. **Semantic GC vs Epoch GC**: When should semantic GC take over?
   Should they coexist (semantic first, epoch as fallback)?

Please review `epistemic.rs` and `DESIGN.md`, then:
- Tell us what's wrong or missing
- Propose concrete Rust code improvements
- Help us solve the determinism problem (#1 above)
