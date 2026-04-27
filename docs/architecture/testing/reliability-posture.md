# Reliability Posture

This document complements [ARCHITECTURE.md](../../ARCHITECTURE.md) and
[verification-architecture.md](verification-architecture.md) with the proof
discipline Neovex now expects for reliability-sensitive changes. The
architecture docs explain what the system guarantees; this doc explains how we
write and maintain proofs that deserve trust.

## Core Principles

- Prefer explicit invariants over incidental timing. A test should state the
  condition it is waiting for, not merely sleep and hope the condition has
  happened.
- Use bounded waits with clear failure messages. Every async proof should make
  it obvious what state failed to arrive and within what budget.
- Treat time budgets as a tool, not the contract. The real contract is the
  state transition, queue boundary, cancellation point, or lifecycle event the
  budget is guarding.
- Prefer deterministic hardship over ambient flakiness. Named fault gates,
  pause handles, seeded workloads, and deterministic harness cases are better
  than “run it enough times and hope”.
- Keep proof ownership close to concept ownership. Scenario-owned modules and
  local support seams are easier to maintain than one flat crate-root test
  file.
- Centralize helpers only when the contract is truly shared. Avoid helper piles
  that erase which invariant a proof is actually asserting.

## Assertions And Waits

Use the narrowest helper that matches the invariant:

- Use direct assertions for immediate, single-threaded state.
- Use semantic wait helpers when a state transition is eventually consistent or
  crosses task, queue, or process boundaries.
- Use helpers that report the awaited condition, elapsed time, poll interval,
  and attempt count whenever a failure would otherwise be opaque.
- Prefer “wait until X is true” helpers over raw `sleep`, repeated
  `yield_now`, or open-coded timeout loops unless the loop itself is the thing
  under test.

When a proof uses a wait, the failure message should explain:

- what state was expected
- what budget was allowed
- which queue, lifecycle, or visibility boundary was involved

## Time Budgets

Neovex proofs should use bounded budgets that stay stable across local runs and
CI contention:

- Use CI-aware helper functions when a crate can share `neovex-testing`.
- Mirror the same contract locally when architecture rules prevent a direct
  dependency, as `neovex-runtime` does.
- Prefer named budgets tied to the proof surface, such as progress windows,
  pending windows, or catch-up timeouts, rather than anonymous literals
  repeated through the file.
- Do not increase a timeout as the first response to flakiness. First ask which
  invariant is implicit, which state boundary lacks a semantic wait, and which
  diagnostics are missing.

Widen a budget only when:

- the underlying invariant is already explicit
- the slower path is legitimate and expected under CI load
- the failure output remains actionable

## Deterministic Hardship

Reliability-sensitive proofs should prefer deterministic control points over
ambient race reproduction:

- use `BlockingFaultInjector` or other named fault seams when the code already
  exposes them
- use pause handles to stop journal, publish, or scheduler work at the exact
  lifecycle boundary being asserted
- use seeded workloads and named harness cases when cross-crate replay matters
- isolate V8-sensitive runtime cases with the existing subprocess harnesses
  instead of forcing broader serialization

If a bug only reproduces with unlucky timing and cannot be pinned to an
explicit boundary, the proof surface likely needs a better seam before the fix
is trustworthy.

## Helper Ownership

Use these ownership rules by default:

- `neovex-testing` owns shared eventual assertions, CI-aware timing helpers,
  deterministic case metadata, and reusable fault-gate primitives for crates
  that can depend on it.
- `neovex-runtime` mirrors the same timing-helper contract locally to preserve
  the zero-workspace-dependency invariant.
- Crate-local support modules should own proof helpers that are specific to one
  concept family, such as Postgres activity helpers, SQLite snapshot fixtures,
  or container-runtime sample specs.
- Production modules should not keep large inline proof blocks when a sibling
  proof tree makes ownership clearer.

## Packaging Guidance

Packaging work should group proofs by behavior, not by arbitrary length:

- keep thin proof roots as composition surfaces
- group scenarios by one behavior family per module
- create a small local `support.rs` only when repeated builders or helpers
  improve clarity
- avoid reopening already-thin production roots unless the production ownership
  is still mixed

## Reliability Change Checklist

Before closing a reliability-sensitive change, check that:

1. The target invariant is stated explicitly in the test or helper name.
2. Bounded waits describe state transitions rather than elapsed time alone.
3. CI-aware budgets are centralized where appropriate.
4. New helpers live at the narrowest ownership level that makes sense.
5. Focused verification covers the touched proof surface before broader
   workspace verification.

Use [ci-failure-investigation.md](ci-failure-investigation.md) when a proof
fails in CI and the next step is investigation rather than implementation.
