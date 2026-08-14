# NNC8.2 Provider-Command Live Claims

Status: `complete; K1-K18 green`

## Outcome

Every provider-command producer must retain current durable authority from
claim through provider result publication. An exact inspection must serialize
with that interval. After inspection authorizes epoch N+1, an epoch-N claimant
must fail before provider I/O.

This item closes provision and restart gaps. It preserves the completed
teardown substrate and does not move provider effects into compute, network, or
the journal.

## Source census

The audit uses NNC8.1 commit
`c3b8eec8f51ab44ced291ce155b6b9b8d00b7e18` as its clean base.

| Producer family | Operations or effects | Current claim interval | NNC8.2 action |
| --- | --- | --- | --- |
| Compute provision phase adapter | All eight provision operations, including reserve, attach, activation, publication, and observation | Four effect or inspection branches discard `ExecuteClaimed`; provider work and result publication occur after the claim lock closes. | Retain the exact execution claim through effects. Serialize nonterminal inspection and publication against that claim. |
| Compute restart phase adapter | All nine restart operations, including withdrawal, retained attachment, activation, publication, and observation | The same discarded-claim pattern exists. | Apply the same current-claim state machine. |
| Guest-node provision dispatcher | Forwarded guest execution and network phases | Execute drops the token before awaited provider work. Inspect performs provider reads and later records them outside a current interval. | Use the asynchronous current-claim seam for execute and exact inspection. |
| Guest-node restart dispatcher | Forwarded guest restart phases | The same awaited execute and inspection gap exists. | Use the same asynchronous state machine. |
| Local Container/Krun execution and attachment teardown | Container execution, Krun execution, Container attachment, and Krun attachment | Each backend receives the exact token and calls `execute_current_claim`; provider inspection uses the current observation. | Preserve. Run regression tests only. |
| Guest-node teardown | Composite execution plus attachment teardown | Both execute paths atomically persist prepared requests and call `execute_started_claim_async`. Inspection reports `EffectCanStillStart` for `Claimed`. | Preserve. Run regression tests only. |
| Parent forwarded-machine teardown | Parent withdrawal, guest phases, and forwarding cleanup | The adapter uses an atomically started claim and current inspection with publication under one lock. | Preserve. Run regression tests only. |

The four incomplete producer families cover every provision and restart
provider operation. The seven completed teardown effect owners remain the
comparison contract. No listener, route, forwarding, network attachment, or
provider implementation changes ownership.

## Frozen state machine

| Durable decision | Execute behavior | Inspect behavior |
| --- | --- | --- |
| New `ExecuteClaimed` | The owner enters `execute_current_claim` before provider work and publishes the result before lock release. | An inspection-only owner performs the read and publishes its result under the claimed interval. |
| Adopted `Claimed` | No second effect starts. The exact owner can resume only through the journal seam and must reacquire the stream lock before provider I/O. | Acquire the exact stream lock, inspect, and publish atomically. A live executor publishes first; an orphaned durable claim is recovered. Every delayed token fails before provider I/O after the inspection publishes. |
| Adopted `InProgress` or `Ambiguous` | Do not retry the effect. | Inspect and publish under the exact current stream lock. |
| Adopted terminal durable result | Adopt it without an effect. | Adopt it without another durable-provider read. A process-bound success may use its existing live-absence reconciliation rule. |
| Stale, skipped, crossed, corrupt, or store-ambiguous claim | Fail before provider work. | Fail closed without retry authority. |

Provider effects remain callbacks owned by their concrete adapters. The journal
owns only exact claim authentication, serialization, and durable result
publication. Compute remains the sole saga coordinator.

## Frozen ownership

| Owner | Paths |
| --- | --- |
| Current-claim protocol | `crates/nimbus-sandbox/src/provider_command/current_claim.rs` and concept-owned provider-command tests |
| Portable provision and restart composition | `crates/nimbus-compute/src/workload_saga/provision_provider.rs`, `restart_provider_command.rs`, and their tests |
| Guest provider composition | `crates/nimbus-cli/src/machine/api/service_workloads.rs`, its `provision.rs` and `restart.rs` children, and their tests. The parent change can only make the already-Arc-backed exact phase owner cloneable for a detached current-claim worker. |
| Static closeout | One aggregate verifier condition, this proof, and the canonical plan |

Forbidden edits include concrete Container, Krun, Netavark, nftables, gvproxy,
socket, proxy, policy, service naming, system projection, cluster, and
`nimbus-network` source. Completed teardown product paths change only if a
regression proves that their retained-claim contract is false.

## Fail-before evidence

| ID | Result at the clean base |
| --- | --- |
| F1 | Source census finds six `ExecuteClaimed(_)` matches in the four incomplete files. Four are effect-producing branches; two are combined inspection branches. |
| F2 | Compute provision calls `effect()` before `record_observation` and never calls `execute_current_claim`. |
| F3 | Compute restart has the same effect-before-record gap. |
| F4 | Guest provision and restart await `execute_phase`, then call `record_effect`; neither file uses an asynchronous current-claim seam. |
| F5 | The synchronous `execute_current_claim` method is `pub(crate)`, so the compute-owned adapters cannot use the existing contract. |
| F6 | Existing delayed-token tests cover stop, detach, release, and prepared remote teardown. No test proves an unprepared provision or restart token fails before I/O after inspection advances authority. |
| F7 | The aggregate verifier has no condition that enumerates all provider-command producer families or rejects a discarded execute token. |

## Acceptance ledger

| ID | Verifiable success criterion | Status |
| --- | --- | --- |
| K1 | The producer census, state machine, owned paths, forbidden paths, and F1-F7 are frozen before product edits. | `pass` |
| K2 | Every product claim site is classified. Four incomplete provision/restart families and seven protected teardown effect owners reconcile exactly. | `pass` |
| K3 | One journal-owned synchronous current-claim seam is usable by upper adapters without moving effects or adding a dependency. | `pass` |
| K4 | Compute provision retains the exact current claim through each effect and result publication. | `pass` |
| K5 | Compute restart retains the exact current claim through each effect and result publication. | `pass` |
| K6 | Guest provision retains the exact current claim through awaited provider work and publication. | `pass` |
| K7 | Guest restart retains the exact current claim through awaited provider work and publication. | `pass` |
| K8 | Fresh inspection, adopted `Claimed`, adopted nonterminal, terminal replay, and process-bound live absence follow the frozen state machine in all four families. | `pass` |
| K9 | Deterministic provision, restart, and teardown races prove that epoch N performs zero I/O after exact inspection authorizes epoch N+1. | `pass` |
| K10 | A live effect blocks exact inspection and successor authority until its result is durable. | `pass` |
| K11 | Caller cancellation cannot cancel an awaited current effect or lose its result publication. | `pass` |
| K12 | Ambiguous, stale, skipped, crossed, corrupt, and publication-failure outcomes remain fenced and create no duplicate effect. | `pass` |
| K13 | Existing local, guest, and parent teardown behavior remains green without a second journal or effect owner. | `pass` |
| K14 | Provision and restart behavior tests cover all 17 operation variants and the concrete local, server-ingress, parent-machine, and guest consumers. | `pass` |
| K15 | One fail-closed verifier condition proves all producer families use a current or atomically started claim and finds no discarded product token. | `pass` |
| K16 | Focused and full affected tests, strict Clippy and Rustdoc, format, dependency/effect scans, aggregate verifier, docs, site, and proof lint pass with exact counts. | `pass` |
| K17 | After K1-K16 pass, one GPT-5.6 Sol/xhigh/fast item review runs. Only an accepted executable correction can authorize one narrow review. | `pass` |
| K18 | The completed ledger, proof, and exact owned diff commit as one NNC8.2 item. | `pass` |

## Implementation order

1. Extend only the journal's current-claim protocol and add fail-before races
   for unprepared provision, restart, and preserved teardown operations.
2. Migrate the two compute phase adapters and prove their closed decision
   matrix.
3. Migrate the two guest dispatchers and prove awaited execution,
   cancellation, exact inspection, and retry fencing.
4. Run preserved local, guest, and parent teardown proofs.
5. Add the source-derived verifier condition, run K1-K16, freeze the candidate,
   and run the one item review.

## Candidate-convergence evidence

- `cargo check -p nimbus-compute` and `cargo check -p nimbus-cli` pass.
- The compute provision adapter passes `6/6`. The restart provider-journal
  slice passes `5/5`, including adopted `Claimed`, adopted ambiguity, terminal
  replay, and exact single-effect behavior.
- The provider-command journal passes `46` tests with two subprocess-entry
  ignores. It proves delayed provision/restart/teardown exclusion, live-lock
  ordering, cancellation-safe publication, exact ambiguity inspection,
  process contention, corruption, stale/skipped/crossed rejection, and retry
  lineage.
- The guest workload-service slice passes `32` tests with one subprocess-entry
  ignore. Its provision and restart mappings cover all `17` operation
  variants. Its retained teardown tests remain green. The parent forwarded
  teardown slice passes `3/3`.
- NNCV037 passes its green source plus seven sole-diagnostic mutations and one
  exclusive aggregate mutation: `9/9`. The additional mutation removes
  adopted-`Claimed` recovery from one producer decision matrix.
- A small contract wrapper owns the NNCV037 mutation group. The
  pre-existing aggregate verifier remains a routing switchboard rather than
  gaining another embedded test group.
- The first full server candidate failed the NNC6.4 `after-owner-claim` cut.
  An adopted durable `Claimed` record returned `InProgress` after its process
  died. Locked sync and async inspection recover that orphaned claim and
  invalidate delayed tokens before provider I/O. The exact fresh-process
  proof passes `1/1`. The two compute adapter regressions pass `2/2`. The
  journal regression is part of the `46` passing tests.
- Corrected full sandbox behavior passes `1,176` tests with `46` declared
  ignores.
- Corrected full compute behavior passes `476` tests with one ignore.
- Corrected full CLI behavior passes `1,010` tests, including two subprocess
  proofs, with four ignores.
- Corrected full server behavior passes `753` tests with `35` ignores.
- The first parallel sandbox rerun had one unrelated readiness-test timing
  failure. The exact test then passed five consecutive runs. The authoritative
  serialized full sandbox suite also passed.
- Strict all-target and all-feature Clippy passes for all four affected crates
  with warnings denied. The successful command uses the isolated
  `target/ptrcomp` target. The first shared-target command stopped at the
  repository V8 pointer-compression guard before Nimbus linting.
- Warning-denied Rustdoc passes for all four crates with all features and no
  dependencies in the same isolated target.
- Rustfmt, diff, Node syntax, Bash syntax, scoped ShellCheck, and Prettier pass.
  The aggregate script retains only its recorded dynamic-source and
  unused-helper ShellCheck exclusions.
- The live architecture verifier passes `38/38`. NNCV004 and NNCV012 prove the
  exact `nimbus-network -> nimbus-core` edge and reject forbidden effects or
  dependencies.
- The public docs gate passes `108` pages. The docs-site gate passes `17/17`.
  Strict proof lint passes this file with zero diagnostics.
- The full GPT-5.6 Sol/xhigh/fast review completed at confidence `0.99`.
  TruffleHog was clean. The reviewer found one P1 stale NNCV037 self-test count
  and one P3 stale recovery action. The owner accepts both findings.
- The P1 correction updates the focused count to `9/9` and the full aggregate
  count to `576/576`. Both mutation gates pass. The live verifier remains
  `38/38`.
- The P3 correction routes the recovery action to this correction proof and
  the one permitted narrow review. It does not change product code.

The accepted P1 changed executable verifier logic. It authorizes one narrow
correction review. The review cadence permits no other review.
- The narrow GPT-5.6 Sol/xhigh/fast correction review is clean at confidence
  `0.99`. TruffleHog is clean. The review cadence is complete.
- The executable and static-proof patch has SHA-256
  `62c600c8fe438b2e036bac04011ca5079bb9ec6411f54415d1950493991b2740`
  across `15` product and verifier paths. The commit that contains this proof
  and the completed ledger row is the durable NNC8.2 item checkpoint.

## Review dispositions

| Finding | Disposition | Proof |
| --- | --- | --- |
| P1: the aggregate expected NNCV037 `8/8` after the wrapper advanced to `9/9` | Accepted. Update the focused summary and the aggregate total. | `--self-test-nnc82` passes `9/9`. The full aggregate self-test passes `576/576`. |
| P3: the recovery `Next` row sent the frozen candidate back to implementation | Accepted. Route the row to correction closeout and the narrow review. | The canonical recovery header names the correction review as its next action. |
