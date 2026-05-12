# CI Failure Investigation

This playbook complements [reliability-posture.md](reliability-posture.md) and
[verification-architecture.md](verification-architecture.md). It is the
evidence-first path for understanding a CI failure before changing code,
budgets, or retry strategy.

## Goals

- identify the exact failing invariant
- separate environmental noise from real correctness risk
- reproduce with the narrowest faithful command
- avoid cargo-cult timeout increases or blanket retries

## 1. Capture The Exact Failure

Before changing code, gather:

- the exact failing command
- the exact failing test, harness case, or binary target
- the failing commit SHA and date
- the CI environment details that matter: OS, runner class, containerized or
  host, and whether the lane was `pr`, `nightly`, or ordinary workspace
  coverage
- the raw stderr/stdout and any linked artifacts

Do not paraphrase away the useful detail. Keep the original failing line, panic
message, timeout text, and any repro command the failure already printed.

## 2. Build A Timeline

For async, scheduler, queueing, or lifecycle failures, reconstruct the order of
events:

- what operation started first
- what state transition was expected next
- which wait or timeout fired
- whether the failure happened before commit, after commit, during catch-up,
  during shutdown, or during replay

If the timeline is unclear, improve diagnostics before widening timeouts.

## 3. Reproduce Narrowly

Prefer the narrowest faithful repro:

- a focused crate lane such as `cargo test -p nimbus-engine postgres_provider`
- a focused surface such as `cargo test -p nimbus-storage sqlite_foundation`
- a harness repro such as
  `make verify-harness-repro SURFACE=<surface> MODE=<pr|nightly> CASE=<case-id>`
- a single ignored subprocess lane when the failure output already provides the
  exact command

Only fall back to full-workspace reruns when the failure cannot be narrowed
further.

## 4. Classify The Failure

Most CI failures fall into one of these buckets:

- missing explicit invariant: the proof waits on time instead of state
- insufficient diagnostics: the failure does not say which state boundary was
  missed
- real correctness bug: the code violated the intended contract
- legitimate slow path under CI load: the invariant is right, but the bounded
  budget is unrealistically narrow
- environmental blocker: network access, cargo lock contention, missing
  provider fixture, or host capability mismatch

Write down which category you believe you are in before changing code.

## 5. Fix In The Right Order

Use this order:

1. Make the invariant explicit.
2. Improve diagnostics so the next failure is more actionable.
3. Add or reuse the right semantic wait or deterministic fault seam.
4. Only then reconsider the time budget if the slower path is real.

Avoid these default reactions:

- increasing every timeout in the file
- adding raw sleeps
- broad test serialization when one proof surface needs a better seam
- treating a one-off local pass as proof that CI was “just flaky”

## 6. Check Ownership

When a failure lands in a concept-mixed file, the fix may need packaging work
as well as logic changes. Ask:

- does the failing proof live beside the code it is protecting?
- does the helper that controls the wait or fault seam belong to the proof
  family, the crate, or `nimbus-testing`?
- would a scenario-owned module make future failures easier to investigate?

If yes, update the owning control plan before refactoring.

## 7. Record What You Learned

When the failure leads to a real fix:

- note the invariant that failed
- note the focused verification used to prove the fix
- update the active control plan or execution log if the work is plan-owned
- keep any new repro command stable and copyable

When the failure turns out to be environmental:

- record the blocker precisely
- record the best focused verification you were still able to run
- avoid presenting the lane as fully verified

## Quick Triage Prompts

Use these questions when a failure is noisy:

- What exact state was supposed to become visible?
- Which code path owns that state transition?
- Which helper or budget guarded it?
- Did the operation commit, remain pending, or get canceled?
- Is there already a narrower proof surface or harness case for this?
- Would a deterministic pause or fault seam make this failure obvious?

The right fix should leave the next failure easier to explain, not merely less
likely to trigger.
