# NNC6.1a Compute Node Workload Coordinator

Status: `complete`

## Outcome owned by this item

NNC6.1a makes `nimbus-compute` the only production caller that coordinates an
issued node-workload reconcile command. `nimbus-node::NodeWorkloadReconciler`
remains the local execution seam. Its exact order stays:

```text
validate -> inspect -> start or stop -> inspect -> write observed status
```

The compute owner delegates an already-admitted local assignment. It does not
yet compile a workload saga, persist desired state, reserve network resources,
or decide activation. NNC6.1b-e own that vocabulary and durable handoff.

This item also removes systemd as an independent tenant-workload restart
authority. A node reconcile request may omit `Restart` or set `Restart=No`.
The node seam rejects `OnFailure`, `Always`, and duplicate restart properties
before backend validation or provider effects.

## Read-only audit

The audit ran at exact parent
`68c14c9aed9ebfc4a728b1b95122eb60ee2c485c`.

| Seam | Current state | NNC6.1a target |
| --- | --- | --- |
| Local reconcile owner | `NodeWorkloadReconciler<B, W>` owns validation, inspection, start/stop, final inspection, and observed-status write. | Preserve this order and its canonical tests. |
| Node-agent batch | `NodeAgent<B, W>` calls the reconciler for each assignment and reports per-assignment outcomes. | Expose one small type-erased reconcile/inspect capability implemented by the real node agent. |
| Compute orchestration | Compute depends on `nimbus-node` for host pressure only. It has no node-workload coordinator. | Add one concrete compute-owned coordinator around the node capability and retain an optional Arc in `ComputeState`. |
| Standalone node executor | CLI constructs `NodeWorkloadReconciler` and invokes it directly. | Construct a `NodeAgent`, wrap it in the compute coordinator, and issue assignments only through compute. |
| Guest-machine service execution | `GuestNodeWorkloadService` owns a generic `NodeAgent` and directly reconciles or inspects its backend. | Hold the concrete compute coordinator and use its reconcile and read-only inspect methods. |
| Restart authority | Production `RunnerSpec` emits `Restart=OnFailure`; the reconciler accepts any allowlisted restart property. | Emit `Restart=No` and reject provider-owned tenant-workload restart before backend validation. |
| Observed status | `nimbus-system::SystemTenantStatusEvidenceWriter` implements the node writer and persists observed status through Engine. | Remain the projection adapter. It cannot become desired state or saga authority. |
| Workload vocabulary | `nimbus-workloads` owns the current desired-workload types and in-memory store. Compute has no direct edge. | Preserve this boundary. NNC6.1b owns the direct edge and saga vocabulary. |

Production construction currently exists at two adapter composition roots:
the standalone node executor and the guest Machine API. Tests construct both
real node backends. This proves substitution for a small node capability
without inventing a god provider.

## Frozen architecture decisions

1. `nimbus-node` owns `NodeWorkloadReconcileCapability`. It is object-safe and
   exposes only capability reporting, result-preserving single-assignment
   reconciliation, batch reconciliation, and side-effect-free assignment
   inspection.
2. `nimbus-compute::NodeWorkloadCoordinator` is concrete. It stores one
   `Arc<dyn NodeWorkloadReconcileCapability>` and does not expose the inner
   capability.
3. `NodeWorkloadReconciler` alone orders local lifecycle. The compute
   coordinator delegates. It does not copy that state machine.
4. `ComputeState` and `NodeServicesConfig` retain an optional Arc to the
   compute coordinator. Repeated access returns the same identity.
5. A protocol-only compute profile reports no node coordinator. A profile
   cannot install a node coordinator without the shared network manager.
6. The standalone node executor and guest Machine API are composition
   adapters. They may construct a real node agent, but they cannot call the
   agent or reconciler directly after construction.
7. `RunnerSpec` requests `Restart=No`. The reconciler rejects provider restart
   before `HostLifecycleBackend::validate`, inspection, start, stop, or status
   write.
8. Low-level systemd request vocabulary may still represent other restart
   policies for isolated provider tests. Production node reconciliation cannot
   admit them.
9. `nimbus-system` remains an observed projection. The coordinator cannot
   write desired state, network state, policy, or provider state.
10. This item adds no `nimbus-compute -> nimbus-workloads` edge. It adds no
    workload saga types, store interface, engine mutation path, network plan,
    activation, compensation, or restart retry policy.

## Exact owned paths

The acceptance-frozen item may edit only these paths before a recorded scope
amendment:

```text
crates/nimbus-node/src/lib.rs
crates/nimbus-node/src/host_lifecycle.rs
crates/nimbus-node/src/reconciler.rs
crates/nimbus-node/src/systemd_transient.rs
crates/nimbus-compute/src/lib.rs
crates/nimbus-compute/src/node_workloads.rs
crates/nimbus-compute/src/config/node_services.rs
crates/nimbus-compute/src/state.rs
crates/nimbus-cli/Cargo.toml
crates/nimbus-cli/src/node_workload_executor.rs
crates/nimbus-cli/src/machine/api.rs
crates/nimbus-cli/src/machine/api/service_workloads.rs
Cargo.lock
docs/private/plans/README.md
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/proof/nimbus-network-control-plane/nnc6.1a-compute-node-workload-coordinator.md
scripts/nimbus-network-control-plane/compute-node-workload-coordinator-contract.sh
scripts/verify-nimbus-network-control-plane.sh
scripts/verify-nimbus-network-source-contract.mjs
```

The item uses at most 19 paths. The acceptance freeze initially expected 16
paths and no manifest edit. The first affected compile found no direct
`nimbus-cli -> nimbus-compute` dependency. The scope gained the CLI manifest
and lockfile before either edit. R4 requires that direct edge because a
transitive dependency would make the coordinator route implicit and brittle.

The first all-features check found an unchanged macOS dead-code warning. The
existing cfg enabled a Linux-only zbus test helper on every test target. The
scope gained `systemd_transient.rs` before that edit. Matching the helper cfg to
its Linux-only consumer forms part of the backend substitution and portability
proof.

The sole full item review found one executable defect and one missing behavior
assertion. The standalone adapter decoded a failed one-item batch as a missing
outcome and replaced the original typed error with `Error::Internal`. The
correction adds a result-preserving single-assignment method to the existing
node capability and compute coordinator. It does not add an owner or change
the local reconcile state machine. The correction also adds the R3 assertion
that a valid protocol-only state returns `None`. Both changes stay within the
existing path ceiling.

If compilation requires another caller or test path, amend this list and the
recovery ledger before editing that path.

## Acceptance criteria

| ID | Verifiable criterion |
| --- | --- |
| R1 | `nimbus-node` exposes one small object-safe node reconcile capability. Its single-assignment route preserves the typed lifecycle error. The actual `NodeAgent` implements it for both DirectProcess and systemd-backed agents. |
| R2 | `nimbus-compute` owns one concrete coordinator. No second production type contains coordinator, saga-coordinator, or reconcile-orchestrator authority. |
| R3 | `NodeServicesConfig` and `ComputeState` retain the exact optional coordinator Arc. Managed access is pointer-identical; protocol-only access returns `None`. |
| R4 | The standalone node executor and guest Machine API construct a node agent but invoke reconcile and inspect only through the compute coordinator. No production upper crate calls `NodeWorkloadReconciler::reconcile_*`, `NodeAgent::reconcile_assignment`, `NodeAgent::reconcile_assignments`, or the backend directly. |
| R5 | The compute coordinator delegates one issued assignment or assignment batch exactly once. It does not evaluate policy, desired state, network plan, provider effects, service names, or system projection writes. |
| R6 | The node reconciler rejects `Restart=OnFailure`, `Restart=Always`, and duplicate `Restart` properties before backend validation and any effect. An omitted property or exactly one `Restart=No` is accepted. |
| R7 | `RunnerSpec` emits `Restart=No`. Both real node backends preserve the canonical reconcile order and receive no provider-owned restart policy. |
| R8 | Guest read-only inspection validates the assignment and inspects through the node capability without reconciliation, status write, restart, or provider mutation. |
| R9 | `nimbus-system::SystemTenantStatusEvidenceWriter` remains the sole Engine-backed node status writer. Compute does not import it into coordinator logic or treat projection state as desired state. |
| R10 | Compute still has no direct `nimbus-workloads` dependency. Network retains only its core workspace edge. No new dependency cycle or provider-effect edge appears. |
| R11 | NNCV026 rejects every named authority, restart, bypass, early-dependency, and effect mutation exclusively. The live verifier and full aggregate self-test pass. |
| R12 | Focused happy, edge, and error tests; full affected suites; all-target/all-feature check; strict Clippy; warning-denied rustdoc; format/diff/script checks; proof lint; docs/site gates; one candidate-frozen Sol/xhigh/fast review; and one exact item commit pass. |

## Fail-before packet

Before product edits, NNCV026 must be the only live verifier failure.

| Packet | Fail-before observation | Corrected proof |
| --- | --- | --- |
| F1 compute owner absent | No compute coordinator module, state field, or accessor exists. | R2-R3. |
| F2 direct upper callers | Both production CLI paths call the node seam without compute. | R4-R5. |
| F3 provider restart remains | `RunnerSpec` emits `OnFailure`, and reconciliation does not fence it. | R6-R7. |
| F4 inspect bypass | Guest inspection reaches the backend through `node_agent.reconciler().backend()`. | R8. |
| F5 future regressions | Each named mutation must fail only NNCV026 after correction. | R9-R11 and aggregate self-test. |

The initial expected-red command is the live aggregate verifier. Accept it only
when NNCV000-NNCV025 pass and NNCV026 is the sole failure. A compile error or
an unrelated verifier failure is not evidence.

## NNCV026 mutation matrix

| Mutation | Required failure |
| --- | --- |
| `missing-node-capability` | The object-safe node capability is absent. |
| `missing-compute-coordinator` | The concrete compute owner is absent. |
| `missing-state-coordinator` | Compute state no longer retains the Arc. |
| `missing-profile-fence` | Protocol-only state can install a coordinator. |
| `direct-cli-reconcile` | The standalone executor bypasses compute. |
| `direct-guest-reconcile` | The guest service bypasses compute. |
| `direct-guest-inspect` | Guest inspection reaches the backend directly. |
| `runner-provider-restart` | `RunnerSpec` restores `OnFailure`. |
| `missing-restart-fence` | Reconciliation no longer rejects provider restart. |
| `duplicate-restart-accepted` | Duplicate restart properties can reach a backend. |
| `coordinator-desired-store` | Compute coordinator imports or owns desired-workload state. |
| `coordinator-network-authority` | Compute coordinator imports or owns network authority/effects. |
| `second-coordinator` | Another production coordinator type appears. |
| `early-workloads-dependency` | Compute gains the NNC6.1b-owned dependency early. |

## Behavioral proof matrix

| Behavior | Proof obligation |
| --- | --- |
| Managed identity | A coordinator injected into `ComputeState` returns pointer-identically. |
| Protocol-only edge | State without a coordinator stays usable and reports `None`. |
| Profile error | Coordinator plus no network manager fails before node capability use. |
| Direct-process happy path | Compute delegation preserves `validate, inspect, start, inspect, write`. |
| Already-running edge | A second pass observes without starting. |
| Stopped happy path | Compute delegation preserves `validate, inspect, stop, inspect, write`. |
| Inspect-only edge | Assignment inspection performs validate and inspect only. |
| Restart error | `OnFailure`, `Always`, and duplicate restart properties produce typed errors before the recording backend sees a call. |
| Capability substitution | DirectProcess and systemd node agents both implement the same type-erased capability. |
| Guest adapter | Machine service start/stop/inspect behavior remains green through the compute owner. |
| Standalone adapter | One-shot and loop command behavior use the compute owner, preserve typed reconcile errors, and preserve status evidence. |

## Non-goals

- No durable workload saga record or store.
- No `nimbus-compute -> nimbus-workloads` dependency.
- No Engine mutation table or schema selection.
- No service-manager in-memory-store removal.
- No network plan compilation, reservation, attach, publish, or release.
- No restart retry policy or desired-generation decision.
- No change to tenant admission, service naming, machine provider facts, or
  sandbox provider effects.
- No change to egress, proxy, certificates, system projections, or cluster
  transport.
- No compatibility wrapper for direct node reconciliation.

## Verification ledger

| Checkpoint | Status | Evidence |
| --- | --- | --- |
| Read-only audit | `done` | Constructor, caller, restart, projection-writer, dependency, and canonical-order census above. |
| Acceptance freeze | `done` | R1-R12, F1-F5, 14 mutations, behavior matrix, exact paths, and non-goals are frozen before product edits. |
| Expected-red | `done` | The live verifier reports `26 passed, 1 failed`. NNCV026 is the sole failure. Its eight diagnostics name the missing node capability, compute coordinator, state/profile handoff, both direct upper callers, guest inspect bypass, runner restart setting, and pre-backend restart fence. |
| Implementation | `done` | The node capability, compute coordinator, state/profile handoff, both caller routes, restart fence, explicit CLI dependency, and Linux-only zbus test-helper cfg are implemented within the 19-path ceiling. |
| Candidate convergence | `done` | Behavior, affected quality, dependency, live-verifier, 187-case mutation, proof-lint, docs `108`, and site `17/17` gates are green. |
| Candidate review | `done` | The sole full Sol/xhigh/fast item review reported two P3 findings at confidence `0.93`. Both are accepted and corrected. The affected NNCV026 mutation family passes `14/14`. |
| Narrow correction review | `done` | One Sol/xhigh/fast pass reviewed only the two accepted corrections. It reported no finding and judged the patch correct at confidence `0.98`. |
| Exact checkpoint | `done` | The narrow-reviewed staged tree is `e1f89ab54a6df8248c5715852a06ec1fb4ce6848`. Its executable/script SHA-256 is `b5e076dcec20fde384347900ff7b7ef78926fc0e7a5145e520263c8632566afc`; its full staged-patch SHA-256 is `b99c857eb6d342759a30397b5923cc87d721cf299bcc709f4f0c6b134e51137c`. Final ledger-only edits do not change executable code and do not trigger another review. |

## Review disposition

The configured `item` gate skipped before model invocation because this
repository uses the `pre-pr` cadence. It does not count as a review. The sole
full item review then ran through the manual gate with GPT-5.6 Sol, xhigh
reasoning, and fast mode.

| Finding | Disposition | Correction proof |
| --- | --- | --- |
| P3: the standalone executor replaced a failed batch disposition with a new `Error::Internal` value. | Accepted. The existing capability and compute coordinator now expose a single-assignment route that returns the original typed lifecycle result. The standalone adapter uses only that compute route. | The type-erased capability preserves `Error::InvalidInput` in the strengthened node test. The direct-bypass mutation fails only NNCV026 at `26/1`. |
| P3: R3 lacked a valid protocol-only accessor assertion. | Accepted. A new test constructs the valid protocol-only profile and asserts that the accessor returns `None`. | The two correction-focused tests pass `2/2`. The full affected suite passes `1,060/1,060` with one declared child-only skip. |

The correction does not add a second coordinator or alter local lifecycle
ordering.

## Narrow correction review

The one permitted correction review used GPT-5.6 Sol with xhigh reasoning and
fast service. It reviewed only the two accepted defects. It reported no
finding and judged the patch correct at confidence `0.98`. The review confirmed
that the single-assignment route preserves `nimbus_core::Error`, delegates once
through compute, and leaves batch behavior unchanged. It also confirmed that
the new test constructs the valid protocol-only profile and directly asserts
`None`. The review cadence permits no further NNC6.1a review.

## Candidate evidence

| Gate | Exact result |
| --- | --- |
| Expected-red | `26 passed, 1 failed`; only NNCV026 failed with all eight intended missing-seam diagnostics. |
| Focused behavior | The original combined node/compute/CLI selection passed `11/11`, and the restart fence passed `1/1`. The correction selection passed `2/2`: the erased node capability retains `InvalidInput`, and protocol-only compute returns no coordinator. |
| Full affected behavior | After the correction, `cargo nextest run -p nimbus-node -p nimbus-compute -p nimbus-cli` ran 1,060 tests and passed 1,060. The sole skipped test is the subprocess-only `nimbus-cli machine::manager::tests::ports_state::external_machine_port_owner_child`. The Linux/session-systemd node-executor lane is cfg-excluded on this macOS host and is not claimed as live provider evidence. |
| Affected compile | Default-feature and all-target/all-feature checks pass for `nimbus-node`, `nimbus-compute`, and `nimbus-cli`. The initial all-feature macOS failure was the unchanged Linux-only zbus test constructor; the recorded cfg correction makes the real gate green. |
| Static quality | Strict all-target/all-feature Clippy with `-D warnings` passes. No-dependency all-feature rustdoc with `RUSTDOCFLAGS=-D warnings` passes. Vendored Brotli warnings remain dependency diagnostics, not affected-crate failures. |
| Dependency boundary | Cargo metadata reports `nimbus-network: nimbus-core`; compute has no direct `nimbus-workloads` edge; CLI declares the new direct `nimbus-compute` edge. |
| Live architecture verifier | NNCV000-NNCV026 pass: `27 passed, 0 failed`. |
| Correction mutation | The revised `direct-cli-reconcile` mutation calls the new direct node-agent route. It fails only NNCV026: `26 passed, 1 failed`. |
| Mutation proof | The first aggregate run found one stale pre-NNCV026 expected total in the legacy NNCV005 self-test. Its isolated mutation proved the new exclusive result is `26 passed, 1 failed`; after correcting that bookkeeping, the complete aggregate reports `187 passed, 0 failed`, including all 14 NNCV026 mutations. No product change followed the aggregate. |
| Correction mutation family | The correction changed only the NNCV026 contract. All 14 affected mutations pass exclusively after the correction. Two attempts to repeat the unchanged full aggregate reached their external 300-second and 1,200-second limits while every completed mutation remained green. They did not report a contract failure. The earlier complete `187/187` result remains the proof for unchanged families. |
| Structured review | The sole full Sol/xhigh/fast review reported two accepted P3 findings at confidence `0.93`. The one narrow correction review is clean at confidence `0.98`. No further review ran or is warranted. |
| Script and patch quality | Node syntax, Bash syntax, contract ShellCheck, aggregate ShellCheck with its established SC2034/SC1091 exclusions, Cargo format, and diff check pass. |

## Modularity budget

Current files remain below 1,500 lines except the 1,512-line node reconciler.
That file is an explicit narrow exception. Its production state machine ends
before the private test module. The remainder is the canonical, tightly
coupled ordering, error, and substitution suite for that state machine.
Splitting those fixtures during this frozen migration would add a review seam
without reducing production authority.

Host lifecycle is 1,336 lines. Compute state is 880 lines. The new compute
coordinator is 44 lines. Guest service workloads is 1,236 lines, and the
standalone executor is 413.

New compute coordination belongs in its concept-owned child.
Existing composition roots receive wiring only.
The source verifier remains the previously justified deep structural-scanner
exception. Its NNCV026 mode reuses the same test-item masking and source walk.
