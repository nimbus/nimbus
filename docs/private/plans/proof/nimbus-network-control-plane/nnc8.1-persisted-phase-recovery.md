# NNC8.1 Persisted-Phase Recovery

Status: `done; K1-K12 green; review cadence exhausted`

## Outcome

Every canonical durable workload and network phase must survive an exact
process cut. A fresh process must converge the same generation to its desired
state or retain it behind a `CleanupPending` fence. The common NNC0 process
harness owns boundary synchronization, timeout diagnostics, kill, reap, and
fresh-process launch.

## Source census

| Durable owner | Current coverage | Finding |
| --- | --- | --- |
| `WorkloadSagaPhase` | All 16 phases and 30 recovery decisions reopen through `SubprocessCrashCutHarness` in the server Engine adapter. | Green. Preserve this proof. |
| `NetworkResourcePhase` | Container and Krun each exercise all 11 phases against absent, present, and unknown provider observations. | Green. Preserve the 66-row decision matrix. |
| OCI attachment effects | Container and Krun each exercise 10 create and 10 delete boundaries in fresh processes, with exact generation, IPAM, segment, listener, provider, and replay checks. | Behavior is green, but a private marker-and-poll harness owns the process cut. |
| Network state store | The NNC0 harness cuts state sync, replacement, and parent-directory sync. | Green. Preserve old-or-new complete-state proof. |
| Port lease authority | The NNC0 harness proves dead active-listener recovery and abandoned never-bound reservation recovery. Local state-machine tests cover all seven phases. | Green. Preserve lifetime and no-premature-reuse proof. |
| Tenant retirement | Five phases resume through the existing compute/server startup owner. | Green. NNC8.1 does not create a second coordinator. |
| Provider-private journals | Existing creator, runner, Netavark, forwarding, execution teardown, and compensation crash matrices retain their current effect owners. | Link as lower-level evidence. NNC8.2, NNC8.3, and NNC8.6 own new ambiguity, orphan, and failure-row behavior. |

The canonical all-phase gap is process-harness substitution, not a missing
runtime recovery algorithm. The attachment matrix has `POLL_INTERVAL`, marker
files, `kill_after_marker`, and `park_forever`. It does not use the NNC0
protocol.

## Dependency decision

`nimbus-sandbox` cannot depend on `nimbus-testing`, even as a development
dependency. `nimbus-testing` depends on `nimbus-tenant`, and `nimbus-tenant`
depends on `nimbus-sandbox`. That edge would create a cycle.

Move the dependency-neutral process protocol to one small
`nimbus-process-harness` crate. It has only standard-library production code
and test-only `tempfile`. Existing CLI, KV, server, and `nimbus-testing`
consumers import it directly. `nimbus-sandbox` becomes a fifth direct consumer.
Delete the old `nimbus-testing` owner and do not add a compatibility re-export.

This extraction has multiple real consumers and removes duplicate process-cut
authority. It does not change `nimbus-network` or any provider effect.

## Frozen ownership

| Owner | Paths |
| --- | --- |
| Process protocol | `crates/nimbus-process-harness/**`, workspace manifest and lockfile |
| Existing consumers | Process-harness imports and development dependencies in CLI, KV, server, and `nimbus-testing` |
| Attachment process proof | `crates/nimbus-sandbox/Cargo.toml` and `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/crash_recovery.rs` |
| Static closeout | Aggregate network verifier, this proof, and the canonical plan |

Forbidden changes include provider implementations, attachment decisions,
workload coordination, policy, naming, proxy forwarding, system projection,
cluster transport, runtime source, and `nimbus-network` dependencies.

## Fail-before evidence

| ID | Baseline result |
| --- | --- |
| F1 | The attachment crash matrix contains no `SubprocessCrashCutHarness`, `run_crash_cut_child`, or `run_crash_recovery_child` reference. The absence command exits zero only because the expected search itself returns one. |
| F2 | The same file contains one polling interval, two `park_forever` calls, two `kill_after_marker` call sites, and bounded polling sleeps. |
| F3 | `cargo tree -p nimbus-sandbox -e dev --depth 1` lists only `futures` and `proptest`. No dependency-safe shared process harness is available. |
| F4 | The frozen source census counts 10 create cuts, 10 delete cuts, 11 network resource phases, and 16 workload saga phases. |

## Acceptance ledger

| ID | Verifiable success criterion | Status |
| --- | --- | --- |
| K1 | This source census, dependency decision, owned paths, forbidden seams, and F1-F4 are frozen before implementation. | `pass` |
| K2 | One low-dependency process-harness crate owns the existing semantic protocol and all 13 harness self-tests. It has no Nimbus or runtime dependency. | `pass` |
| K3 | CLI, KV, server, and `nimbus-testing` import the new owner directly. No old module or compatibility re-export remains. | `pass` |
| K4 | Sandbox uses the same owner without a cycle. `nimbus-network -> nimbus-core` remains its only workspace edge. | `pass` |
| K5 | The attachment parent runs every 10 create and 10 delete boundaries for both Container and Krun through the common exact-boundary protocol. | `pass` |
| K6 | The attachment proof has no marker-file synchronization, polling sleep, private kill/reap loop, or unbounded wait. Wrong boundaries and child failures retain common harness diagnostics. | `pass` |
| K7 | Create recovery converges to the exact same-generation `Active` state or `CleanupPending`; delete recovery converges to `Released`; replay executes no duplicate acknowledged effect. | `pass` |
| K8 | The 66-row network phase/observation matrix and the 30-row, 16-phase workload process matrix remain green. | `pass` |
| K9 | State-store durability and port-lifetime crash tests remain green through the extracted owner. | `pass` |
| K10 | Provider-private phase proofs remain green and no effect, policy, naming, forwarding, projection, or cluster owner moves. | `pass` |
| K11 | Focused and full affected tests, strict Clippy and Rustdoc, format, dependency/effect checks, aggregate verifier, docs, site, and proof lint pass with exact counts. | `pass` |
| K12 | After K1-K11 pass, one GPT-5.6 Sol/xhigh/fast item review runs. Only an accepted executable correction can authorize one narrow review. | `pass` |

## Immediate implementation order

1. Extract the process protocol without behavior changes and update direct
   consumers.
2. Run its self-tests plus the existing state-store, port-lease, CLI, KV, and
   server crash tests.
3. Replace only the attachment matrix process transport. Keep every lifecycle
   observer, durable witness, recovery assertion, and replay assertion.
4. Add one fail-closed aggregate verifier condition for the owner, dependency,
   and no-private-harness contract.
5. Run K1-K11, freeze the candidate, run one item review, and commit NNC8.1.

## Current evidence

- Process harness: `13 passed, 2 ignored` child entrypoints.
- State-store and port-lifetime consumers: `8 passed, 3 ignored` child entrypoints.
- Attachment matrix: `1 passed`. The parent executed 10 create and 10 delete
  cuts for Container and Krun, for 40 crash/recovery rows.
- Network phase matrix: `2 passed`. The tests evaluated 11 phases against
  three provider observations for both backends, for 66 decisions.
- Workload phase matrix: `1 passed`. The parent evaluated 30 fresh-process
  decisions across all 16 workload phases.
- Provider-private fresh-process set: `19 passed, 3 ignored` child
  entrypoints. The set covers creator, runner, Netavark, forwarding,
  Container/Krun execution and network teardown, orphan evidence, and the
  attachment parent.
- Full affected behavior: CLI `1,007 passed, 4 ignored`, sandbox `1,162
  passed, 30 ignored`, server library `659 passed, 35 ignored`, and KV listener
  `10 passed, 2 ignored`.
- Dependency checks: the process harness has no normal dependency and
  `nimbus-network` retains its sole workspace edge to `nimbus-core`.
- Static checks: NNCV020 mutations `9/9`, the NNC8.1 affected mutation lane
  `4/4`, and the live aggregate `37/37` pass. Bash and Node syntax also pass.
  The affected lane covers the changed NNCV035 summary and three dependency
  mutations for NNCV036. NNC7.6 retains the unchanged `564/564` baseline.
- Quality: format, diff, and strict all-target Clippy pass for all six affected
  crates. Warning-denied Rustdoc passes for the authored
  `nimbus-process-harness` crate. A broad multi-crate Rustdoc diagnostic found
  three pre-existing broken links in unchanged server/testing files. Those
  files are outside this item. This proof does not count those links green.
- Documentation: `check-docs` reports 108 link-clean pages and the docs-site
  verifier reports `17/17` conditions.
- The first workload command built unrelated integration tests and exhausted
  local storage before any test ran. The corrected `--lib` command passed.
  `cargo clean -p nimbus-server` removed only 53.1 GiB of reproducible build
  artifacts before the corrected run.

## Item review

The sole full GPT-5.6 Sol/xhigh/fast item review inspected staged tree
`55f30155aff056b1d8373b953ddfa8ed833fde1a` and patch SHA-256
`a837eaba17ac27c97bc36562447beabaec88d07c17fc8fe5ee8bfc68fc337748`.
The review returned two P2 findings at confidence `0.95`. We accepted both:

1. NNCV036 must reject every dependency except one unconditional `tempfile`
   development edge, including extra development, build, and normal edges.
2. The changed NNCV035 aggregate summary and new NNCV036 condition need an
   affected mutation campaign before K11 can pass.

The first finding changes executable verifier code. Both corrections and their
affected proofs pass. These changes authorize exactly one narrow correction
review.

## Correction evidence

- NNCV036 now permits exactly one unconditional `tempfile` development edge.
  It rejects all other normal, build, target-specific, optional, or additional
  development dependencies.
- The focused mutation lane passes `4/4`. It proves exclusive NNCV036 failure
  for an extra development dependency, a Nimbus build dependency, and a runtime
  normal dependency. It also proves the changed NNCV035 aggregate summary.
- The live aggregate passes `37/37`. Format, diff, docs `108`, site `17/17`,
  and proof lint pass after the correction.
- The narrow review inspected staged tree
  `4358b71c0f9b7291908d37ad1b41e713fb17e9ab` and patch SHA-256
  `e0e1aff176dbf157844a1e19a7e4e24673d7a7db77d41977dfca5a7c96118bd3`.
  It returned no findings. The clean result at confidence `0.98` exhausts
  review cadence.
