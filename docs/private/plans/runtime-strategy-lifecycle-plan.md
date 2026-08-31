# Nimbus Runtime Strategy Lifecycle

Status: `proposed`.
Owner: this plan.
Created: 2026-08-29.
Baseline: `codex/release-readiness-2026-08` @ `304a2e677293fec7d150e12ffc0ba98960917753`.
Proof root: `proof/runtime-strategy-lifecycle/`.
Next action: wait for RRC8 U6 to record its exact commits and runtime-strategy evidence, then request owner activation.

## Outcome

> Nimbus keeps only measured and safe runtime strategies in product code. It
> preserves rejected strategies as reproducible lab definitions, raw evidence,
> and patches. Every runtime change passes hard gates before product promotion.

## Architecture

Before:

```text
[product default]
  -> exact-key WarmPool + CooperativeLocker
  -> NodeFull startup snapshot | WebStandard unsnapshotted cold construction
  -> exact owner and reuse authority + heap limits + reset-or-destroy
[rejected realm candidate]
  -> public policy value + product branches + Deno replay carries
[benchmark labels] -> requested policy can obscure actual construction
```

After:

```text
                              +-> [product winners: small supported set]
[strategy lifecycle contract] -> [promotion gate]
                              +-> [runtime lab: schema + raw data + verdicts]
                              +-> [experiment archive: exact patch + trigger]

[Deno | V8 | Bun/JSC | Wasmtime change]
  -> [admissibility] -> [hard gates] -> [measurement] -> [decision record]
```

## Scope

- Owns: the runtime-strategy lifecycle, decision criteria, benchmark evidence
  schema, experiment archive, and promotion or removal workflow.
- Owns: separate product runtime choices and diagnostic choices in
  `nimbus-runtime` and its benchmark harness.
- Owns: the post-RRC8 audit that confirms `WarmContextRecycle` is absent from
  product policy and execution, and closes any post-handoff residual.
- Owns: consume the RRC8 realm-carry disposition, audit its durable archive,
  and classify future Deno or V8 strategy carries.
- Owns: durable contributor documentation, a project runtime-strategy skill,
  and narrow source comments at stable concept seams.
- Coordinates with: `release-readiness-2026-08-plan.md`, which owns the active
  Deno 2.9.6 and V8 150.4 uplift and the release verdict.
- Consumes: `profile-aware-isolate-runtime-final-architecture-plan.md` and the
  completed `nimbus-runtime-tenant-isolation-plan.md`.
- Does not own: the current RRC8 fork release or product publication.
- Does not own: tenant identity, workload admission, a Bun product backend, or
  a Wasmtime redesign.
- Non-goal: keep an executable copy of every rejected strategy on the product
  dependency path.
- Non-goal: test the full Cartesian product of invalid or unsafe permutations.
- Non-goal: weaken exact reuse authority, the shared read-only heap, heap
  limits, or egress policy.
- Non-goal: weaken cancellation or reset-or-destroy behavior to improve a
  benchmark.

## Coordination And Precedence

1. RRC8 owns all edits in the active Deno 2.9.6 and V8 150.4 uplift worktrees.
   This plan cannot change, pause, or redirect that work.
2. The runtime tenant-isolation contract wins on owner identity, retirement,
   reuse authority, and retained-state separation.
3. The profile-aware runtime decision record remains historical evidence. This
   plan can replace its future workflow, but it cannot rewrite its measurements.
4. Source and tests win when a historical label conflicts with current
   construction behavior.
5. The owner must resolve any overlap before this plan becomes `active`.
6. Use only Sol for review while the owner's Opus 5 and Fable restriction is active.

## Activation Gate

This plan passed its independent scope, source, ordering, and acceptance review.
It remains `proposed` while RRC8 owns the active fork work. Activate it only
when every item holds:

- RRC8 U6 is terminal and records exact Nimbus, Deno, and rusty_v8 commits.
- RRC8 records the realm-carry disposition and the exact controlled A/B result.
- The plans index still names one owner for each affected fork and product seam.
- The owner approves activation and the proposed project skill.

At activation, RSL0 replaces the draft baseline and initializes task proofs. An
unrelated release blocker does not prevent the handoff after terminal U6.

## Invariants

1. Product code contains only supported strategy winners and the code needed to
   select them.
2. The lab preserves enough information to reproduce a result without product
   branches for a rejected strategy.
3. A benchmark row records the requested pool kind and the actual construction
   mode. A label cannot imply a startup snapshot that the runtime did not use.
4. Safety gates precede performance comparison. A failed safety gate rejects a
   candidate before scoring.
5. Mutable retained state always uses the exact runtime-owner and reuse-authority
   contract.
6. The shared read-only heap stays enabled. A controlled density study and an
   architecture review must accept any replacement.
7. WebStandard stays unsnapshotted while the NodeFull superset anchor owns the
   process cage and the cross-profile snapshot crash remains reproducible.
8. NodeFull startup snapshots stay enabled unless a controlled cold-start study
   rejects them without breaking Node compatibility.
9. Nimbus destroys a dirty, unquiesced, over-limit, cancelled, or ambiguous
   runtime. It never returns that runtime to a retained pool.
10. A Deno or V8 carry has a current consumer, a disposition, proof, and a
    removal trigger. No fork tag or repin precedes its required A/B verdict.
11. Raw measurement data and the decision verdict are different files.
12. An archived experiment records its exact source commit, patch, runtime
    version, build features, machine data, workload, repetitions, and result.

## Optimization Contract

Nimbus uses this objective:

> Minimize total CPU and resident-memory cost per successful invocation while
> meeting tenant-isolation, semantic-correctness, recovery, and tail-latency
> requirements across bursty and sustained multi-tenant workloads.

### Hard gates

A candidate must prove all these properties before a performance verdict:

- exact runtime-owner and reuse-authority isolation.
- guest and host semantic correctness.
- no permission, capability, request-state, or service-state leakage.
- bounded cancellation and termination.
- reset-or-destroy behavior after each invocation.
- bounded heap and process resident memory.
- correct failure replacement and recovery.
- no unacceptable process-wide crash or cage conflict.

### Measured objectives

The scorecard records these measurements:

- latency: cold p50, p95, and p99, plus warm p50 and p99.
- efficiency: throughput per core, construction time, and teardown time.
- state: snapshot size, parse time, RSS, heap carryover, and warm-hit rate.
- reliability: contention tails, connection reuse, and failure replacement cost.

The review also records fork burden, upstream rebase cost, test-matrix size,
observability, crash blast radius, backend portability, and proof repeatability.

### Decision rule

The review uses a Pareto frontier. It selects the smallest winner set that
covers the supported workload classes. It does not select one global winner
from one microbenchmark. A candidate that loses or fails a hard gate moves to
the experiment archive with a re-open trigger.

The lab records backend, construction mode, reuse model, scheduler, profile,
pointer compression, authority fanout, workload, and pressure. Its
admissibility table removes invalid combinations before execution. Backend
fixtures map these concepts without copying V8-only API names.

## Audit Baseline Facts

| ID | Observed fact | Evidence and consequence |
|---|---|---|
| RSF1 | The user-provided commit `f5f81336688dbb6e1994b76ab256f72d4f1362c7` is not a pre-PIR runtime baseline. | It is dated 2026-08-21. `WarmContextRecycle` entered in `0d934c5b9` on 2026-06-21. The Web snapshot restriction entered in `53ad5e25c` on 2026-06-27. RSL0 must use `3ce860f01`, `0d934c5b9`, `53ad5e25c`, `f5f813366`, and the activated candidate as distinct anchors. |
| RSF2 | The principal runtime-strategy files have no diff from `f5f813366` to this draft baseline. | `git diff --quiet f5f813366..304a2e677 --` over `limits/axes.rs`, runtime construction and invocation, and `runtime_pool_modes.rs` exits 0. RSL0 must not attribute an unmeasured post-`f5f813366` regression to those files. |
| RSF3 | At the draft baseline, `RuntimePoolKind::WarmContextRecycle` is public, serialize-only Rust policy data with product execution branches. No CLI, package, server, or compute selector was found. | RRC8 U3 completed the caller audit and removes the value and product branches in its paired candidate. RSL4 must verify the final post-U6 graph instead of repeating the removal. |
| RSF4 | WebStandard currently builds without a startup snapshot. Node targets use a startup snapshot. | `V8RuntimeConstructionMode::for_compatibility_target` in `backends/v8/startup.rs` owns the mapping. Benchmark output must record this actual mode. |
| RSF5 | Current proof rejects `WarmContextRecycle` as a default. | PIR2 records Web rows at 2.23 to 2.50 times its labelled startup-snapshot lane. NFR6 records Node rows at 5.38 to 10.01 times startup snapshots and 13.35 to 16.13 times exact warm pools. The Web label does not prove snapshot use. |
| RSF6 | Realm replay affects Nimbus snapshot companion data and Deno runtime construction. | RRC8's exact old-graph A/B found no detected Web construction change, a small favorable Node replay-off result after counterbalancing, and 16 encoded replay-table bytes in the Node22 blob. RSL8 must reproduce this study through the permanent lab before it treats the result as a cross-host performance claim. |
| RSF7 | The generic lazy-ESM termination repair can outlive realm recycle. | The Deno carry `core: avoid lazy esm abort on termination` has a separate failure contract. RSL7 must audit it independently and must not remove it with realm-only carries. |
| RSF8 | The Deno bump ledger is stale relative to this draft baseline. | The ledger names Deno 2.9.1 while `Cargo.toml` names `v2.9.3-nimbus.2`. RRC8 is already replacing both. This plan must consume RRC8's final ledger instead of repairing the active uplift in parallel. |

## Decisions To Ratify At Activation

| ID | Candidate decision | Evidence | Re-open condition |
|---|---|---|---|
| RSL-D1 | Keep exact-key `WarmPool` as the retained V8 product strategy. | PIR warm-hit, fanout, and isolation evidence. | A runtime-version or workload change moves it off the Pareto frontier. |
| RSL-D2 | Keep NodeFull startup snapshots and WebStandard unsnapshotted construction under the shared cage. | Node cold-start evidence and the cross-profile cage-crash proof. | A separate cage design or upstream heap change passes all hard gates. |
| RSL-D3 | Keep the shared read-only heap and pointer compression on proven targets. | Existing density evidence and current anchor design. | Controlled density evidence shows that cost exceeds the memory benefit. |
| RSL-D4 | Accept RRC8's removal of `WarmContextRecycle` from product policy and execution, and preserve its exact old graph in the experiment archive. | It is non-default, measured slower, and adds realm-specific fork and snapshot seams. | New Deno/V8 behavior or a new workload predicts a Pareto win. |
| RSL-D5 | Preserve rejected strategies as lab metadata and exact patches, not dormant product branches. | This keeps future comparison possible without permanent product cost. | A strategy becomes a supported product winner through the promotion gate. |

RRC8 timing decision: U3 omits realm-only carries during the untouched 2.9.6
replay. RRC8 removes paired Nimbus consumers before U4. It archives the patch
and completes the A/B before U5. A supported caller reopens this decision.

## Status Ledger

| ID | Task | Status | Evidence |
|---|---|---|---|
| RSL0 | Pin the post-RRC8 baseline, preserve the draft audit, and author the 24-condition verifier red. No product behavior changes. | `todo` | |
| RSL1 | Define the canonical runtime-strategy lifecycle, terms, gates, admissibility rules, and decision record. | `todo` | |
| RSL2 | Build the permanent benchmark-lab schema, runner contract, raw-data format, and experiment archive. | `todo` | |
| RSL3 | Add the project skill and narrow contributor routing for the runtime-strategy workflow. | `todo` | |
| RSL4 | Audit current product strategies, callers, serialized surfaces, snapshots, and every Deno and V8 carry. | `todo` | |
| RSL5 | Separate diagnostic strategy definitions from product runtime policy and preserve exact experiment recipes. | `todo` | |
| RSL6 | Audit the RRC8 Nimbus realm-recycle cleanup and close any post-handoff residual. | `todo` | |
| RSL7 | Audit the immutable post-RRC8 fork carries, removal triggers, and rejected-experiment archive. | `todo` | |
| RSL8 | Reproduce the controlled construction studies in the permanent lab and accept a lifecycle verdict. | `todo` | |
| RSL9 | Run final repository gates, independent reviews, and the completion audit. | `todo` | |
| RSL99 | Clean up this plan after the final pull request merges. | `todo` | |

## Tasks

### RSL0 Baseline And Red Verifier

- Problem: the active uplift can change the code and dependency baseline before this plan starts.
- Owning seam and paths: this plan and `proof/runtime-strategy-lifecycle/`.
- Steps:
  1. Wait for the activation gate.
  2. Pin Nimbus, Deno, rusty_v8, build features, and machine data.
  3. Compare the five historical anchors in RSF1 with the activated baseline.
  4. Preserve `draft-plan-audit.md` and initialize the task proof files.
  5. Add `scripts/verify-runtime-strategy-lifecycle.sh` with 24 fixed checks.
  6. Capture the failing summary before an implementation task.
- Acceptance: the proof records exact commits and confirms which strategy
  files did or did not change between each anchor.
- Acceptance: the verifier prints `Summary: N passed, M failed` with `N + M =
  24`, exits nonzero, and changes no product behavior.
- Fail-before: at least one lifecycle, lab, or product-separation condition is
  red.
- Verification: run `bash -n scripts/verify-runtime-strategy-lifecycle.sh` and
  `bash scripts/verify-runtime-strategy-lifecycle.sh`.

### RSL1 Canonical Lifecycle Contract

- Problem: historical plans record decisions, but no short current document
  owns proposal, evaluation, promotion, archive, removal, and re-open steps.
- Owning seam and paths:
  `docs/private/architecture/runtime/runtime-strategy-lifecycle.md` and the
  runtime architecture routing index.
- Steps:
  1. Define the candidate states from proposal through product or archive.
  2. Define admissibility, hard gates, scorecard fields, and Pareto review.
  3. Define change triggers for Deno, V8, Bun/JSC, Wasmtime, workloads, and host
     constraints.
  4. Ratify or revise RSL-D1 through RSL-D5.
- Acceptance: transition fixtures reach both product and archive verdicts.
  They reject a promotion, rejection, removal, or re-open transition that lacks
  its required proof.
- Fail-before: the RSL0 verifier reports the missing lifecycle contract.
- Verification: run the technical-writing linter in strict mode on the new
  document, `bash scripts/check-docs.sh`, and the RSL verifier.

### RSL2 Permanent Benchmark Lab

- Problem: current benchmark names can mix requested policy with actual runtime
  construction. Archived prose is not a reusable experiment schema.
- Owning seam and paths: `crates/nimbus-runtime/benches/`, a concept-owned lab
  manifest, and `proof/runtime-strategy-lifecycle/experiments/`.
- Steps:
  1. Define admissible axes without creating the full Cartesian product.
  2. Record requested strategy, actual construction mode, runtime versions,
     features, machine data, workload, pressure, repetitions, and units.
  3. Split raw samples from the verdict and decision record.
  4. Add an archive format for exact patches or source tags that main no longer
     compiles.
  5. Add schema and mutation tests.
- Acceptance: a fixture rejects a false snapshot label, missing authority
  fanout, missing units, and a verdict embedded in raw data.
- Acceptance: one archived `WarmContextRecycle` recipe identifies exact source
  commits and can be rebuilt in a detached experiment worktree.
- Fail-before: the schema test rejects at least one current or synthetic
  ambiguous row.
- Verification: run `cargo test -p nimbus-runtime runtime_strategy_lab`, `cargo
  bench -p nimbus-runtime --bench runtime_pool_modes --no-run`, and the RSL
  verifier.

### RSL3 Agent Workflow And Routing

- Problem: future agents need one discoverable workflow before they change a
  product runtime strategy or a runtime fork carry.
- Owning seam and paths: `.agents/skills/runtime-strategy-lifecycle/SKILL.md`,
  runtime architecture routing, Deno fork workflow, and approved routing text.
- Steps:
  1. Add a project skill that reads the canonical contract and current fork
     ledger before runtime-strategy work.
  2. Add one routing entry to the nearest contributor index after owner review.
  3. Put source comments only on stable strategy-selection, construction-mode,
     and experiment-manifest seams.
  4. Make each comment state an invariant or removal trigger. Do not add comments
     that only tell an agent to read documentation.
  5. Add skill fixtures for proposal, promotion, archive, and re-open states.
  6. Map Bun/JSC and Wasmtime fixtures to backend-neutral lifecycle concepts.
- Acceptance: skill tests route a Deno uplift, a Bun strategy, and a Wasmtime
  strategy. They also route a `RuntimePoolKind` edit to its lifecycle stage.
- Acceptance: no composition root or ordinary branch gets an agent-only
  comment.
- Fail-before: the RSL0 verifier cannot find a project-owned workflow entry.
- Verification: run the technical-writing linter in strict mode on the skill
  and changed documents, `bash scripts/check-docs.sh`, and the RSL verifier.

### RSL4 Current Strategy And Fork Audit

- Problem: cleanup must use current callers and fork commits, not historical
  assumptions.
- Owning seam and paths: `crates/nimbus-runtime`, `nimbus-compute`, the Deno and
  rusty_v8 worktrees, the fork ledger, and RSL4 proof files.
- Steps:
  1. Trace every runtime strategy from public data through all selectors and
     execution branches.
  2. Trace startup-snapshot bytes, residual lazy sources, replay sources,
     shared-heap setup, heap limits, and teardown.
  3. Classify each Deno and V8 carry as product-required, experiment-only,
     upstreamed, replaced, or removable.
  4. Measure source, binary, snapshot, and ordinary construction cost where the
     classification depends on cost.
  5. Route every finding to RSL5 through RSL9 or record `no-action` evidence.
- Acceptance: the audit has no unowned strategy, branch, serialized value,
  snapshot field, carry, test, verifier condition, or benchmark label.
- Acceptance: the audit proves whether any non-test caller can select
  `WarmContextRecycle`.
- Fail-before: retain the pre-audit inventory and each initial mismatch.
- Verification: run `cargo test -p nimbus-runtime`,
  `make verify-profile-aware-runtime-crossover`, `bash
  scripts/verify-profile-aware-isolate-runtime.sh`, `bash
  scripts/verify-runtime-execution-classification.sh`, `bash
  scripts/verify-runtime-tenant-isolation.sh`, and the RSL verifier. The
  compatibility-named profile verifier aggregates current contracts. The
  removed NodeFull realm verifier remains archived evidence, not a post-U6
  gate.

### RSL5 Product And Lab Type Separation

- Problem: a rejected diagnostic strategy should not remain a public product
  policy value only to keep a benchmark executable.
- Owning seam and paths: runtime limit types, execution planning, benchmark lab
  types, experiment manifests, and serialization tests.
- Steps:
  1. Define the minimal product strategy enum from RSL4 evidence.
  2. Move diagnostic labels and recipes to lab-owned types.
  3. Make archived experiments use an exact patch or tag when product source no
     longer implements the strategy.
  4. Reject diagnostic strategy names in product configuration and diagnostics.
- Acceptance: product policy cannot select a lab-only candidate. The lab can
  still locate and reproduce its exact historical implementation.
- Fail-before: an old-graph fixture proves the public Rust policy selected or
  serialized the rejected candidate before RRC8 removed it.
- Verification: run `cargo test -p nimbus-runtime limits`,
  `cargo test -p nimbus-compute`, `cargo bench -p nimbus-runtime --bench
  runtime_pool_modes --no-run`, and the RSL verifier.

### RSL6 Nimbus Realm-Recycle Cleanup Closure

- Problem: RRC8 removes realm-recycle product branches before activation. The
  lifecycle needs a closure audit so no residual or reintroduction stays live.
- Owning seam and paths: `nimbus-runtime` invocation, worker, V8 snapshot,
  metrics, tests, benches, and runtime verifier scripts.
- Steps:
  1. Compare the activated source with the exact RRC8 cleanup and archive.
  2. Confirm no realm-recycle residual remains in product policy, invocation,
     replay data, metrics, tests, or verifier obligations.
  3. Close a post-handoff residual in Nimbus if RSL4 finds one.
  4. Keep retained-lazy-source behavior and all independent termination fixes.
  5. Keep structural tests that reject reintroduction on the product path.
- Acceptance: `rg WarmContextRecycle crates/nimbus-runtime/src` finds only an
  approved historical or rejection reference, or finds no match.
- Acceptance: WebStandard unsnapshotted construction, Node startup snapshots,
  warm-pool reuse, cancellation, teardown, authority isolation, and cage tests
  stay green.
- Fail-before: the RRC8 archive and pre-cleanup source retain the removed
  product branches. A structural mutation proves the current gate rejects them.
- Verification: run `cargo test -p nimbus-runtime`, `make
  test-rust-runtime-cage`, the three current runtime gates named in RSL4, and
  the RSL verifier.

### RSL7 Post-RRC8 Fork Carry And Archive Audit

- Problem: RRC8 publishes Deno 2.9.6 without realm-only carries. This plan must
  verify that graph without a second release or loss of the rejected experiment.
- Owning seam and paths: the post-RRC8 fork ledger, immutable fork references,
  and the exact archived experiment recipe.
- Steps:
  1. Pin the exact post-RRC8 upstream and Nimbus fork tags.
  2. Confirm that the released fork has no Nimbus realm-only API or replay
     construction state.
  3. Audit lazy-ESM termination, Locker, egress, heap, TCP, and CI carries
     independently.
  4. Verify that each retained carry has a consumer, proof, and removal trigger.
  5. Reconstruct the rejected strategy in a detached experiment worktree.
- Acceptance: Deno exposes no Nimbus-only realm API without a current consumer.
  Every retained carry has a proof and a removal trigger.
- Acceptance: RSL7 does not edit, tag, release, or repin either product graph.
  RRC8's immutable graph stays the product source of truth.
- Fail-before: the archive identifies each pre-cleanup realm-only commit and its
  Nimbus consumer.
- Verification: run the immutable-reference and fork-policy commands from the
  terminal RRC8 ledger, `cargo test -p nimbus-runtime`, `make
  test-rust-runtime-cage`, and the RSL verifier.

### RSL8 Construction Truth And Controlled Lab Reproduction

- Problem: an architectural decision needs measurements that distinguish pool
  policy, actual construction, runtime version, and shared-heap behavior.
- Owning seam and paths: runtime traces, benchmark schema, uplift proof, and
  architecture decision records.
- Steps:
  1. Reproduce replay-on and replay-off on the archived exact graph and controls.
  2. Run the same admissible workloads on the exact post-RRC8 graph.
  3. Compare shared-heap on and off only on supported isolated builds.
  4. Compare Node snapshot and unsnapshotted cold construction.
  5. Measure ordinary construction and snapshot size with and without removable
     replay scaffolding.
  6. Write raw data, statistical summaries, environmental variance, and a
     separate verdict.
  7. Ratify or revise the lifecycle decision. Do not change the RRC8 product
     fork as part of this reproduction.
- Acceptance: every result identifies actual construction mode. No Web row
  calls itself snapshotted when the selector used `Unsnapshotted`.
- Acceptance: the final product set satisfies every hard gate and stays on the
  measured Pareto frontier for its workload class.
- Acceptance: the consumed RRC8 graph resolves published tags and exact
  lockfile SHAs. It has no local path or unpublished revision.
- Fail-before: schema fixtures reject the old ambiguous label and incomplete
  environment data.
- Verification: run the lab command with its fixed manifest, both fork policy
  scripts, `make verify-profile-aware-runtime-crossover`, and the RSL verifier.

### RSL9 Completion Audit

- Problem: removal can hide a behavior loss, stale proof, or experiment that
  nobody can reproduce. One final review checks all repositories and evidence.
- Owning seam and paths: all changed Nimbus, Deno, and rusty_v8 paths and the
  complete proof root.
- Steps:
  1. Run focused and full repository gates.
  2. Run the configured Sol-only Nimbus pre-PR autoreview after final commits.
  3. Run an independent allowed-model review of code, docs, results, and fork
     carries. Do not use Opus 5 or Fable unless the owner lifts the restriction.
  4. Reproduce one product winner and one archived rejected experiment from
     their manifests.
  5. Issue the completion verdict and route all residual work.
- Acceptance: all 24 verifier checks pass. No accepted P0 through P3 finding
  remains. All ledger rows except RSL99 are terminal.
- Acceptance: the closeout proof names exact commits, pull requests, commands,
  counts, raw data, verdicts, reviewer results, and remaining uncertainty.
- Fail-before: retain every initial review finding and failed reproduction.
- Verification: run `cargo fmt --all --check`, `make ci`, `bash
  scripts/check-docs.sh`, `bash scripts/verify-nimbus-docs-site.sh`, the RSL
  verifier, and the configured Nimbus pre-PR autoreview gate.

### RSL99 Cleanup

- Problem: a merged plan must not remain as a stale control plane.
- Owning seam and paths: this plan, its proof root, and the plans index.
- Steps:
  1. Confirm that the final pull request merged.
  2. Archive or delete the plan under the repository plan policy.
  3. Preserve experiment records in their canonical non-plan archive.
  4. Remove the plans-index entry or replace it with the approved retrospective.
- Acceptance: no active routing points to this plan. Reproducible benchmark and
  experiment evidence remains discoverable from the runtime architecture index.
- Fail-before: not applicable because the ledger records the final merge.
- Verification: run `rg -n 'runtime-strategy-lifecycle-plan' docs/private
  .agents/skills` and confirm that every remaining match routes to the approved
  terminal layout.

## Goal

```text
Execute docs/private/plans/runtime-strategy-lifecycle-plan.md to completion.
This is a whole-plan goal, not a single-task goal. Use worktree
`<set at activation>` and branch `<set at activation>`. Read the plan fully,
then read AGENTS.md, docs/private/plans/README.md,
docs/private/plans/release-readiness-2026-08-plan.md,
docs/private/plans/profile-aware-isolate-runtime-final-architecture-plan.md,
docs/private/plans/nimbus-runtime-tenant-isolation-plan.md,
docs/private/operating/deno-fork-workflow.md, and
docs/private/architecture/runtime/isolate-glossary.md. Chat history is not
progress state. Resume from the status ledger, the execution log, and git state.
If compaction happens, continue from the plan and git state. Loop: keep one task in_progress, implement at the owning seam,
capture fail-before evidence, run the verification commands, commit the work
per the commit policy, write the proof file, append the execution log with the
work commit, mark the task terminal with evidence, commit the plan update, then
advance to the next task. Decide rather than ask. Mark a wrong or already
satisfied task no-action with a one-line reason. Record a blocker and continue
with the next eligible task. Binding constraints: preserve all invariants and
non-goals; do not edit an active RRC8 worktree; do not weaken isolation,
semantics, cancellation, heap, egress, shared-heap, or reset-or-destroy gates;
keep rejected strategies reproducible outside product code. Commit policy: use
one pull request per task unless the task names a cross-repository sequence;
commit intended files only; run the repository pre-PR autoreview after final
checks. Stop only at a valid stop state from the plans skill. Before stopping,
update the ledger and log, and record the next action in the status line. The
goal is met when RSL0 through RSL9 are terminal. The 24-condition verifier must be
green, the completion audit accepts the product and lab split, and RSL99 waits
only for the merge trigger.
```

The activation update must replace both placeholders before this goal can run.

## Execution Log

Append rows at the end. This section stays last.

| Date | Item | Action | Evidence |
|---|---|---|---|
| 2026-08-29 | meta | Authored the deferred draft from a source and history audit. | Baseline `304a2e677`; no implementation started and no active-plan routing changed. |
| 2026-08-29 | meta | Completed an independent review and promoted the corrected plan to proposed. | `proof/runtime-strategy-lifecycle/draft-plan-audit.md`; RRC8 keeps fork ownership and no RSL implementation started. |
| 2026-08-30 | meta | Corrected the post-U6 fork ownership and verification workflow. | RRC8 owns realm omission, publication, and repin. RSL7 and RSL8 only audit and reproduce the immutable result. Sol-only review restriction recorded; no RSL implementation started. |
