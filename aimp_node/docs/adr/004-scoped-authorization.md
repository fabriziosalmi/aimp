# ADR 004: Authorization Is a Gate, Bound in the Epoch Certificate

## Status
Proposed — draft for review before any implementation. Target: v0.6.0 (breaking). Issues #8, #10, #19.

## Context

A signature proves *who* produced a claim, never *what they were entitled to assert*. Issue #8 raised this; ADR 003 arrived at the same place from the opposite direction. After every evidence-weighting defect was closed, one case remained that aggregation cannot separate: K independently-delegated identities with distinct evidence sources and no embeddings are indistinguishable from K genuinely independent sources (#10). Nothing is left to measure. The missing fact is not another confidence adjustment but whether each identity was entitled to originate that class of assertion at all.

Today nothing in the protocol carries that fact:

1. **Delegation transfers reputation and nothing else.** `ReputationTracker::delegate(from, to, rep)` caps the grant at the delegator's reputation and charges half of it. There is no notion of *what* the delegate may do. A credential obtained to report one sensor carries the same standing as one obtained to aggregate the network's beliefs. This is reputation manufacturing authority, which is the thing this ADR exists to prevent.
2. **The only epoch L3 knows is `claim.tick / grid_size`, and `tick` is self-declared.** Any authorization window keyed on it lets a claimant choose its own epoch, for example by back-dating into a certificate that has since expired. ADR 003 and #21 established the rule this would violate: *a self-declared field may not exempt a claim from a check.*
3. **Edges have no author.** `RawEpistemicEdge` is `{from, to, relation, strength}`. A `Supports` edge is a corroboration, but who corroborated is not recorded, so a corroborate capability could not be checked even if it existed.
4. **Summaries have no author.** `SemanticReducer` emits `Summary` claims with `origin = [0; 32]`. That is sound while every node recomputes them deterministically from the same inputs. A Summary *received* over L2 is unattributable.

The epoch certificate is the natural place to bind authorization: it is master-signed, time-bounded, and a protocol object, so revocation becomes non-renewal with no separate revocation mechanism. It is described in the design notes but **not implemented**. Its layout is therefore still free, and this ADR fixes it.

## Decision

### 1. The certificate binds capability, scope and constraints, not a domain bitmap

```text
EpochCertificate {
    subject:          ephemeral public key
    issuer:           public key (master, or a delegate holding `delegate`)
    epoch:            first epoch of validity
    valid_epochs:     number of epochs, ≥ 1
    capabilities:     set of { Originate, Corroborate, Aggregate, Delegate }
    scope:            set of resource identifiers (indexed table)
    constraints:      closed list of typed constraints (see §6)
    delegation_depth: remaining depth; 0 = may not delegate
    parent:           hash of the issuer's own certificate, or none for master-issued
    policy_version:   u16
    signature:        issuer's signature over all of the above
}
```

The three dimensions are kept separate because they fail separately. A node authorized to *report* one sensor must not thereby gain authority to *corroborate* other sensors, to *aggregate*, or to *delegate*, even when all of those acts fall under the same resource. Encoding may be compact (bitsets, indexed tables), but these semantics are the contract.

### 2. Authorization is evaluated first, and it is a gate, not a weight

```text
authenticate signature
→ resolve and validate the certificate chain for the act's epoch
→ check capability and scope for this act
→ validate claim and evidence structure
→ correlation handling (ADR 003)
→ reputation
→ aggregation
```

A claim that fails authorization is **absent** from belief formation. It is not discounted, and it is not counted at weight zero in a way that still shapes grouping or ranking. Reputation modulates confidence *within* an authorized scope. It can never create authority, and a high-reputation identity acting outside its scope is treated exactly like an unknown one.

### 3. Every act has a capability, and every act has an author

| act | capability | checked against |
|---|---|---|
| a claim (`Observation`, `Inference`, `Intent`) | `Originate` | the claim's resource |
| a `Supports` / `Contradicts` edge | `Corroborate` | the target claim's resource (but see §7) |
| a `Summary` received over L2 | `Aggregate` | the resource of every input |
| a certificate issued to another key | `Delegate` | §4 |

This requires two wire changes: **edges gain an `author` and a signature**, and **received Summaries gain an author**. Edges and Summaries a node derives *locally* (`AutoEdgeGenerator`, `reduce_epoch_aligned`) need no capability. They are deterministic functions of inputs that each passed the gate, and every node recomputes the same result. `DerivedFrom` and `SharedSource` are structural, not assertions, and are not gated.

### 4. Delegation can only attenuate

A certificate issued by a delegate is valid only if, against its parent:

- `capabilities ⊆ parent.capabilities`, and `Delegate ∈ parent.capabilities`;
- `scope ⊆ parent.scope`;
- every parent constraint still holds (constraints only accumulate);
- `delegation_depth < parent.delegation_depth`;
- its validity window lies inside the parent's.

Chains are verified to the master key. Reputation delegation (`ReputationTracker::delegate`) remains as the confidence mechanism, but it is no longer sufficient to participate: a delegate with reputation and no certificate is outside every scope.

### 5. The epoch is not chosen by the claimant

The authorization set for epoch N is a pure function of the certificates finalized before the boundary of N. Every node evaluates the same set at the same boundary, using the bucketing that already makes correlation discounting order-independent. A renewal or delegation that arrives late takes effect at N+1 for every node, never at N for some.

The epoch an *act* is evaluated in must not come from `tick`. The proposal: an act names the certificate it was made under, and is evaluated in the epoch of the most recent **epoch boundary record** in its causal past in the DAG. Boundary records are master-signed beacons materialized like any other mutation. See Unresolved 1 for the weakness this leaves.

### 6. Constraints are a closed set

Proposed: a closed, enumerated set of typed constraints, each with a deterministic evaluator, for example epoch sub-window, per-epoch act budget, and a bound on topological distance from the issuer. Unknown constraint types make the certificate **invalid** (fail closed), and `policy_version` gates additions. An expression language was considered and not proposed: every evaluator must be bit-identical across nodes, and the attack surface of an interpreter on the authorization path is hard to justify before a real deployment needs one. *Open to review* (Questions for review, Q1).

### 7. Corroboration scope

Proposed default: to corroborate a claim, the author must hold `Corroborate` over the **claim's** resource. Origination rights over it are *not* required, so independent cross-domain verifiers remain possible. The stricter alternative is to require origination rights as well. *Open to review* (Questions for review, Q2).

### 8. Type-level enforcement

The gate must not be a convention. Today all ten `Claim` fields are `pub` (#19), so the defensive accessors added by ADR 003 are enforced by comment. In the same release:

- the `Claim` wire fields become private;
- the belief-formation entry points (`propagate_trust_*`, `LogOddsBeliefEngine::compute`, the reducer) accept only an `AuthorizedClaim` that can be constructed solely by the gate.

An unauthorized claim then cannot reach aggregation without a compile error.

## Consequences

**Pros**
- Closes the case #10 leaves open. It does not do so by detecting it: K delegated identities can now act only within what their certificates grant, and each grant is attributable to an issuer.
- **Bounds the consequence of compromise.** A correctly authorized node that is compromised can still lie, and scoping does not detect that. What changes is the damage: from "anything, indefinitely" to "these acts, over these resources, until the certificate is not renewed". It is a large reduction, and it is not compromise detection.
- Revocation is non-renewal: no revocation lists, no revocation gossip.
- Separating capabilities closes the escalation path from a low-level credential to aggregate- or delegate-level authority.

**Cons**
- **BREAKING**, three times over: certificates are required to participate, edges and received Summaries carry authors, and `Claim` fields go private. Deployments need a master key ceremony and a certificate issuer.
- Wire cost: a signature and an author on every gossiped edge, plus the certificate chain per subject per epoch (O(1) per epoch, cacheable).
- Liveness depends on certificate renewal. A node whose renewal is late drops out at the boundary. This is fail-closed, but it is a new way to lose availability.
- Validation cost: chain verification per subject per epoch, cached; scope checks per act.

## Unresolved

1. **Stale causal past.** Evaluating an act in the epoch of the latest boundary in its causal past (§5) lets a node *pretend not to have seen* recent boundaries: it builds on stale heads and keeps acting under an expired certificate. Rejecting stale acts by comparison with the receiver's own view would make acceptance depend on local state, which reintroduces divergence. Candidates: require each act to reference a boundary no older than a fixed number of epochs *before the boundary in which the act is first materialized*; or make finality of an act depend on a boundary that post-dates it. Neither has been worked through yet. This is the hardest open problem in this ADR.
2. **The master key** is a single root of authority. Threshold issuance and rotation are out of scope here but must exist before any real deployment.
3. **Migration**: whether v0.6.0 offers a transition mode that admits uncertified claims at reduced weight. Proposal: **no**. A transition mode that weights rather than gates is exactly the conflation §2 rejects.

## Questions for review

- **Q1.** Constraints: a closed enumerated set (§6), or is there a real case for an expression that needs an evaluator?
- **Q2.** Corroboration: is `Corroborate` over the claim's resource sufficient (§7), or should origination rights be required too?
- **Q3.** Unresolved 1: is there a known construction for epoch evaluation in a leaderless DAG that resists withholding without making acceptance local-state-dependent?

## Verification plan

Each of these is written as a test **before** the implementation, and is shown to fail against v0.5.0:

- K delegated identities with distinct sources and no embeddings (the #10 fixture): their claims are absent from aggregation unless each holds `Originate` over the resource.
- An out-of-scope claim from a maximal-reputation identity contributes nothing, and does not change the grouping or ranking of any other claim (absent, not zero-weighted).
- Back-dating `tick` into an expired certificate's window has no effect on the evaluated epoch.
- A delegate cannot issue a certificate wider than its own in any dimension (capabilities, scope, constraints, depth, window).
- An unattributed `Supports` edge received over L2 is rejected. A locally derived one is unaffected.
- The authorization set at boundary N is identical under every arrival order of the same certificates (the permutation test ADR 003 uses for claims).
