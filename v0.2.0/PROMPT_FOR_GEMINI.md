# For Gemini — AIMP v0.2.0 Critical Review

You were the academic conscience of v0.1.0. We need you again.

## Context
Based on ChatGPT's L3 vision and your endorsement of the Epistemic/Intent/Semantic
direction, we've built a prototype Cognitive Layer.

## Your job: Destroy it.

Please review `epistemic.rs` and `DESIGN.md` with these specific lenses:

### 1. Academic Rigor
- Are we claiming novelty where there is none?
- What existing papers cover this ground? (Kleppmann, Shapiro, Bailis, etc.)
- Is "Cognitive Layer for CRDT" a publishable concept or engineering fluff?

### 2. Formal Correctness
- Does the Bayesian confidence aggregation in ExactMatchReducer hold mathematically?
  (Assumption: claim independence. Is this valid for correlated sensors?)
- Does TimeDecayScorer's integer approximation of 2^(-age/half_life) introduce
  consensus-breaking rounding errors?
- Can the PriorityIntentResolver be gamed by a Byzantine node that always
  claims confidence=1.0?

### 3. Architectural Critique
- Is the Claim struct too heavy? (150 bytes vs 64 bytes raw payload)
- Should `confidence` be protocol-level or application-level?
- Is SmallVec<[[u8; 32]; 4]> for input_claims sufficient?

### 4. Paper Positioning
- If we write a v0.2.0 addendum, what venue would accept it?
- Is this closer to AAMAS (multi-agent systems) or SIGCOMM (networking)?
- What's the minimum empirical evaluation needed?

### 5. The Hard Question
- Does adding L3 dilute the clean, focused contribution of v0.1.0?
- Should v0.2.0 be a separate paper entirely?

Be merciless. We'd rather kill a bad idea now than defend it poorly later.
