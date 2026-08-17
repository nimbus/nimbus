# NNC6.5 teardown choreography substitution audit

Status: `in_progress; plan and expected-red contract freeze`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

This record freezes the teardown replacement before the first product-source
edit. NNC6.5 is a plan-only audit item. NNC6.5a-NNC6.5g are the prospective,
acceptance-bearing implementation items. Each item is one coherent unit of
value and one structured-review unit.

## Result

Nimbus already has the correct portable teardown phase graph and strong
provider-local cleanup state machines. It does not have the command protocol,
compute driver, or caller substitution that connects those seams. Product stop
and delete paths still call coarse provider stop operations directly.

The target ownership is:

```text
nimbus-workloads  portable teardown state, commands, fences, observations
        |
        v
nimbus-compute    sole intent submitter, CAS winner, dispatcher, driver
        |
        +--> nimbus-server   final ingress withdrawal effects
        +--> nimbus-node     DirectProcess/Systemd drain and stop effects
        +--> nimbus-sandbox  Container/Krun detach and release effects
        +--> machine/guest   exact remote effects and parent publication effects

nimbus-services   names, definitions, sessions, source claims, projections
nimbus-network    unchanged portable connectivity vocabulary and lease authority
```

No effect moves into `nimbus-network`. Its initial workspace dependency remains
only `nimbus-core`.

## Current authority and gaps

| Area | Current source | Current behavior | Required change |
| --- | --- | --- | --- |
| Portable state | `crates/nimbus-workloads/src/saga.rs`, `saga/state.rs` | Has `WithdrawalCommitted -> Withdrawn -> Drained -> WorkloadStopped -> NetworkDetached -> NetworkReleased -> Recorded`, retained references, ordered terminal observations, and `CleanupPending`. | Add a strict teardown attempt/claim/result protocol and race interlocks. Move teardown ownership to concept children. |
| Pure recovery | `crates/nimbus-compute/src/workload_saga/recovery.rs` | Derives raw teardown actions. No production code dispatches them. | Replace raw action-shaped authority with one pure teardown decision that must pass the confirmed-command gate. |
| Service stop | `crates/nimbus-compute/src/services.rs`, `resource_provision.rs`, `crates/nimbus-services/src/manager/retirement.rs` | Compute delegates to services. Services inspects and calls `SandboxBackend::stop`. | Compute submits a stopped successor and drives the durable teardown. Services projects only after safe progression. |
| Sandbox stop | `crates/nimbus-compute/src/sandboxes.rs`, services retirement | Services directly inspects, stops, and changes its projection. | Use the same compute teardown runtime as service stop. |
| Definition delete | `crates/nimbus-services/src/manager/definitions.rs` | An in-memory mutation claim surrounds direct stop and definition removal. Provision does not observe the claim. | Services owns a source claim/finalize contract. Compute cancels or inspects in-flight work, drains any late result, reaches safe terminal state, then permits removal. |
| Tenant delete | `crates/nimbus-compute/src/state.rs`, services retirement | Services enumerates in-memory projections, performs best-effort direct stops, removes artifacts, and erases source/projection state. | Compute drives durable tenant workload records. Engine deletion cannot finish while any owned saga is unresolved. NNC6.1e2 retains fresh-process enumeration and final convergence. |
| Compose down | `crates/nimbus-cli/src/compose/lifecycle.rs` | Local and forwarded paths inspect, call coarse stop, inspect again, and derive an outcome. | Open the same Engine-backed store as Compose up and submit through compute. Never add a CLI-local saga store. |
| Guest/machine stop | `crates/nimbus-cli/src/machine/backend.rs`, `machine/api/service_workloads.rs`, `machine/api/routes.rs` | Coarse stop messages have no saga phase, attempt, epoch, or transition fence. Parent publication cleanup follows guest stop. | Use exact phase envelopes. Parent publication must withdraw before guest stop and release only after exact guest/provider absence. |
| Physical machine stop | `crates/nimbus-cli/src/machine/manager/stop.rs` | Withdraws machine listeners and stops the VMM without a durable workload drain barrier. | Reject stop with a typed conflict while durable workload authority is active unless the canonical compute drain is available. Keep VM effects machine-owned. |
| Server ingress | `crates/nimbus-server/src/workload_ingress.rs` | Has publish and restart-retained withdrawal. Final cleanup is Drop-based and cannot make settlement success authoritative. | Add exact final per-workload withdrawal. Cancel and join workers, close routes, settle listener leases, and return durable evidence. |
| Node execution | `crates/nimbus-node/src/host_lifecycle.rs:32`, `reconciler.rs:307`, `reconciler.rs:486`, `reconciler.rs:509`, `direct_process.rs:137`, `systemd_transient.rs:259`, `crates/nimbus-compute/src/node_workloads.rs:29`, `crates/nimbus-cli/src/machine/api/service_workloads.rs:221` | Compute forwards assignment reconciliation to the node. A stopped desired projection calls one coarse `inspect -> stop -> inspect` path. The guest Machine API bypasses that coordinator and calls the lifecycle backend directly. DirectProcess mutates process-local state. Systemd performs one D-Bus stop. No teardown phase, attempt, epoch, transition claim, or durable ambiguous-effect settlement exists. A post-stop inspect or status-write failure can follow the effect. DirectProcess replay appends another stop log. | NNC6.5c adds separate exact drain and stop execute/inspect capabilities. Each binds a compute-confirmed command. Stale or crossed claims fail before DirectProcess or D-Bus effects. NNC6.5f removes the guest bypass. NNC6.5g makes the reconciler an observed projection, not a second teardown coordinator. |
| Container/Krun cleanup | sandbox runtime and shared OCI attachment lifecycle | Coarse stop combines runtime stop, provider detach, listener/IPAM/segment release, and artifact cleanup. Provider-local durable recovery is strong. | Expose honest phase capabilities over the existing state machines. Do not fabricate separate completion when one provider operation has not durably crossed that boundary. |
| Failed provision | compute provision reducer and driver | Definite failure stops later provision, is excluded from recovery, and has no compensation owner. | Persist an exact compensation cause, inspect unresolved effects, and enter teardown only for the retained resources that can exist. |

## Load-bearing findings

1. `WorkloadSagaRecord::advance` cannot grant provider authority. It has no
   teardown claim or result correlation.
2. `requires_recovery` excludes definite provision failure. A failed attempt
   can retain resource evidence without a compensation driver.
3. Workload state rejects a higher intent while provision awaits dispatch,
   requires inspection, or has a definite failure. Stop cannot fence these
   races.
4. A successor can veto a restart, but no production transition hands the
   terminal restart result to `WithdrawalCommitted`.
5. Services still owns an effectful `Arc<dyn SandboxBackend>` through retirement.
   That duplicates compute lifecycle authority.
6. Definition deletion can cross in-flight provision. A late success can appear
   after desired source removal.
7. Drop cleanup in server ingress logs settlement failure. It cannot authorize
   `Recorded`.
8. Provider-local attachment teardown already has durable progress and exact
   ambiguity handling. The new adapter must reuse it, not add a second cleanup
   journal.
9. Host-managed and machine-forwarded publication have different owners and
   evidence. Capability selection must remain exact and fail closed.
10. Tenant and system views are projections. They cannot become enumeration,
    desired-state, lease, or teardown authority.
11. Container stop sends TERM/KILL before it durably writes stop intent. Krun
    already persists intent and has the stronger resumable cleanup pattern.
12. The shared attachment callback named `BackendWithdrawn` can stop execution
    and the PEP. Its name is not proof of an upper-saga withdrawal or drain
    boundary.
13. Parent machine publication stays active during the current guest stop.
    Parent release does not authenticate the complete guest absence response.
14. Node general stop lacks the exact claim/fencing already present for node
    restart.

## Frozen portable protocol

`nimbus-workloads` will own these inert concepts:

- `WorkloadTeardownCause`: an exact stopped-successor retirement or an exact
  failed-provision compensation.
- `WorkloadTeardownStep`: `WithdrawPublication`, `DrainExecution`,
  `StopExecution`, `DetachNetwork`, and `ReleaseNetwork`.
- `WorkloadTeardownSubjects`: the exact publication, execution, or network
  reference required by the step.
- `WorkloadTeardownAttemptId`, `WorkloadTeardownDispatchEpoch`, dispatch claim,
  authorization, inspection result, absence evidence, effect result, and
  disposition.
- `WorkloadTeardownProviderTarget`: exact ingress selection, execution provider,
  or attachment selection. It derives only from retained durable evidence.

`RecordTerminalEvidence` is a pure state transition. It is not a provider
command. A resource-free step advances without a command and without a
fabricated observation.

Each attempt binds:

- workload key and saga ID.
- active generation and desired digest.
- exact source and provider evidence.
- exact retained resource subject.
- source phase, target phase, and step.
- successor generation/digest or compensation cause.
- issuing revision, stable attempt ID, monotonic dispatch epoch, and confirmed
  transition ID.

Only a confirmed CAS winner receives an Execute command. Replay or an ambiguous
store result receives Inspect. Execute ambiguity first persists
`InspectionRequired`. Only then can a provider inspection run. Exact inspection
that proves the effect incomplete authorizes the same attempt at the next epoch
once.

Exact satisfaction or absence advances. Stale, crossed, duplicate, or
reselected input fails before a provider effect.

A definite teardown failure enters `CleanupPending` with the last safe phase,
retained references, exact inspection requirements, and failure evidence.
NNC8.3 remains the only cleanup finalization and resource-reuse owner.

## Frozen lifecycle and race rules

Provisioning:

```text
admit -> compile -> persist -> reserve -> prepare -> attach -> activate
      -> ready -> publish -> observe
```

Teardown:

```text
persist withdrawal -> withdraw -> drain -> stop -> detach -> release -> record
```

Rules:

- Persist `WithdrawalCommitted` before the first provider call.
- Withdrawal must prove the exact endpoint set absent before drain can advance.
- Drain must prevent new work and settle in-flight work before stop.
- Stop must prove the exact execution attempt absent or terminal before detach.
- Detach must prove provider and namespace absence before release.
- Release must prove listener, IPAM, segment, and provider authority terminal
  before `Recorded`.
- A newer successor during teardown updates only the queued successor. It does
  not cancel an issued effect or replace active-generation evidence.
- Compute inspects and settles an issued provision or restart command before
  teardown. Compute retains and retires a late success. Compute does not retry
  a proven absence.
- Cancellation before durable submission makes zero store and provider calls.
  Cancellation after submission cancels only the waiter.
- Definition/source/session removal waits for safe durable lifecycle progress.
- Unresolved persistence makes zero stop effects.
- NNC6.5 withdraws provider publication. NNC6.6 alone owns concurrent logical
  service-name/cache resolution fencing.

## Capability map

Compute will compose small capabilities with real substitution:

| Capability | Effect owner | Contract |
| --- | --- | --- |
| Final ingress withdrawal | server ingress and machine parent publication | Execute/inspect one exact publication reference. Cancel/join workers and prove exact absence. |
| Execution drain | DirectProcess/Systemd/guest/sandbox execution owner | Reject new work, settle in-flight work, and report exact attempt evidence. |
| Execution stop | DirectProcess/Systemd/guest/sandbox execution owner | Stop and inspect the exact execution attempt. |
| Network detach | Container/Krun shared attachment adapter and machine forwarding adapter | Detach provider/namespace while retaining release authority. Report exact attachment evidence. |
| Network release | Same provider-local attachment owner | Release listeners, PEP, IPAM, segment, and provider authority only after exact detach. |

There is no `NetworkProvider` or `TeardownProvider` god interface. The compute
registry selects the exact admitted execution and network realm. There is no
first-available fallback.

## Prospective implementation split

| Item | Owned value | Dependency | Completion proof |
| --- | --- | --- | --- |
| NNC6.5 | Audit and expected-red freeze only | NNC6.4a | A1-A24, NNCV035 expected red, 55/55 mutations, no product-source change, one full review, and one narrow correction review. |
| NNC6.5a | Portable teardown protocol and durable reducer | NNC6.5 | Strict wire/state matrices, claim/result/ambiguity/race tests, server codec round trip, no effect trait or product caller. |
| NNC6.5b | Compute decision, confirmed-command gate, dispatcher, driver, registry, and runtime | NNC6.5a | CAS-before-effect, one winner, crash cuts, exact routing, same-attempt retry, cancellation, no provider implementation. |
| NNC6.5c | Server ingress plus DirectProcess/Systemd execution adapters | NNC6.5b | Exact final ingress withdrawal and exact drain/stop substitution. Worker settlement failure blocks progress. |
| NNC6.5d | Container/Krun execution drain/stop plus host-managed and forwarded-machine detach/release adapters | NNC6.5b | Honest drain, stop, detach, and release evidence over existing provider journals. Host-managed and machine-forwarded crash/race matrices. |
| NNC6.5e | Native service/sandbox stop and definition deletion cutover | NNC6.5c-NNC6.5d | K1-K32 in `nnc6.5e-native-source-retirement-cutover.md` pass. All native callers use compute. A late provision result drains. Removal waits. Services loses effect authority. Source and execution generations remain distinct. |
| NNC6.5f | Compose, guest, forwarded, and physical-machine boundary cutover | NNC6.5c-NNC6.5d | Compose uses Engine/compute. Exact remote envelopes preserve parent-before-guest order. Physical stop fails closed with active workload authority. |
| NNC6.5g | Failed-provision compensation, tenant-retirement cutover, convergence, and deletion gate | NNC6.5e-NNC6.5f | Failed resources retire in exact reverse effect order. Tenant delete waits. The item deletes coarse stop paths and unused lifecycle bypasses. NNCV035 is green. |

NNC6.1e2 still owns fresh-process startup enumeration and final tenant-retirement
convergence. NNC6.5g supplies the explicit durable choreography and caller
cutover needed by that later item.

### Prospective product path ownership

These sets are the exclusive primary implementation boundaries. A later item
must not edit an earlier set unless the deletion-only handoff below names the
exact path. The shared control-plane set contains the canonical plan, plan
index, one item-owned proof, and static verifier evidence. The integration
owner changes them serially. They do not define product ownership.

- NNC6.5a owns `crates/nimbus-workloads/src/saga/teardown.rs`, its
  `saga/teardown/` children, `saga/state/teardown.rs`, `saga.rs`,
  `saga/state.rs`, `saga/state/provision.rs`, `saga/state/restart.rs`,
  `saga/tests.rs`, `saga/tests/provision_state.rs`,
  `saga/tests/restart_state.rs`, `saga/test_support.rs`, `store/tests.rs`, and
  `lib.rs`. It owns the server store files `workload_saga_store/codec.rs`,
  `schema.rs`, `tests/codec.rs`, `tests/durability.rs`, `tests/restart.rs`,
  `tests/mod.rs`, `tests/recovery.rs`, and `tests/tenant_enumeration.rs`.
  It also owns mechanical fixture conversion only in the compute test paths
  `workload_saga/test_support.rs`, `workload_saga/recovery/tests.rs`, and
  `workload_saga/tests.rs`. These test-only paths cannot add compute behavior.
- NNC6.5b owns the exact `crates/nimbus-compute/src/workload_saga/` paths
  `teardown_decision.rs`, `teardown_command.rs`, `teardown_dispatch.rs`,
  `teardown_driver.rs`, `teardown_registry.rs`, and `teardown_runtime.rs`, plus
  each same-named test child directory. It also owns
  `workload_saga/teardown_test_support.rs`, `workload_saga.rs`, the compute
  `lib.rs`, `workload_saga/recovery.rs`, `restart_runtime.rs`, and
  `restart_runtime/tests.rs`. It has a narrow mechanical-test exception for
  `workload_saga/recovery/tests.rs` to permit deletion of raw teardown actions
  without a compatibility surface. The real process proof has a test-only
  server exception for `workload_saga_store/tests/teardown_driver_process.rs`,
  `workload_saga_store/tests/mod.rs`, and
  `workload_saga_store/tests/composition.rs`. This avoids a compute-to-server
  dependency cycle. It must remove raw teardown-action authority and compose
  the runtime from existing saga state and store accessors. It does not own
  `crates/nimbus-compute/src/state.rs`, the remaining NNC6.5a-owned compute
  fixture files, `workload_saga/teardown_node.rs`, or
  `workload_saga/teardown_sandbox.rs`.
- NNC6.5c owns `crates/nimbus-server/src/workload_ingress.rs` and its child
  directory, `listener_lease.rs`, `listener_lease/`, and the server `lib.rs`.
  It also owns `crates/nimbus-node/src/direct_process.rs`, `host_lifecycle.rs`,
  `host_lifecycle/`, `systemd_transient.rs`, `systemd_transient/`, and the node
  `lib.rs`. It does not own the node reconciler caller. Its compute adapter is
  `crates/nimbus-compute/src/workload_saga/teardown_node.rs` and its children.
- NNC6.5d owns `crates/nimbus-compute/src/workload_saga/teardown_sandbox.rs`.
  It owns sandbox capability/module roots, Container `runtime.rs` and its
  teardown, machine-publication, manifest, cleanup, and attributed test
  children. It owns Krun `vm.rs`, `vm/lifecycle.rs`, and attributed tests. It
  owns OCI attachment-lifecycle, IPAM, segment, forwarding, process, port, and
  egress roots and children. Its scope includes execution drain and stop, not
  only network release.
- NNC6.5e owns `crates/nimbus-compute/src/services.rs`, `sandboxes.rs`,
  `resource_provision.rs`, and new concept-owned `resource_retirement.rs`.
  It has narrow composition, source-fence, settlement, and projection
  exceptions in compute `state.rs`, `workload_saga.rs`, `lib.rs`,
  `workload_provisioner.rs`, `workload_saga/restart_runtime.rs`, and
  `workload_projection.rs`; NNC6.5g retains tenant deletion, failed-provision
  compensation, and final convergence in those roots. It owns server
  `workload_composition.rs`, native HTTP service/sandbox context adapters, and
  their exact tests. It owns the local-only CLI `network_composition.rs`
  registration; forwarded and Compose composition remain NNC6.5f-owned. It
  owns services `catalog.rs`, `lib.rs`, `manager.rs`, `manager/types.rs`,
  `manager/definitions.rs`, `definition_mutation.rs`, `source.rs`, `sessions.rs`,
  `session_channels.rs`, `sandboxes.rs`, and `handles.rs` only for source
  claim/finalize policy and distinct source/execution projections. It excludes
  `manager/retirement.rs` and `manager/tests/tenant_teardown.rs`. Compute owns
  in-flight provision/restart settlement; services does not acquire provider
  or lifecycle-coordinator authority. The exact superseding path and seam
  contract is in `nnc6.5e-native-source-retirement-cutover.md`.
- NNC6.5f owns `crates/nimbus-compute/src/machines.rs` and
  `machine_lifecycle.rs`. It owns `crates/nimbus-machine/src/api.rs`, its new
  teardown child, tests, and the machine `lib.rs`. It owns the CLI Compose
  lifecycle and module paths. It owns the lifecycle, forwarding, and support
  tests. It also owns CLI machine API, routes, and service-workload teardown
  children. It owns backend teardown children, client, publication authority,
  physical stop/port paths, stubs, module roots, and attributed tests.
- NNC6.5g owns `crates/nimbus-compute/src/state.rs`, provision decision and
  driver files and tests, `workload_provisioner.rs`, and
  `config/node_services.rs`. It owns services `manager.rs`,
  `manager/retirement.rs`, `manager/tests/tenant_teardown.rs`, and `lib.rs`. It
  owns node `reconciler.rs` and its new teardown tests. It owns
  `crates/nimbus-sandbox/src/backend.rs`. It may add concept children for
  compensation and tenant choreography under the compute saga owner.

NNC6.5g has one narrow deletion-only handoff after NNC6.5e and NNC6.5f are
green. The handoff covers node `host_lifecycle.rs`, `direct_process.rs`, and
`systemd_transient.rs`. It also covers Container `runtime.rs`, Krun `vm.rs` and
`vm/lifecycle.rs`, and only the CLI test doubles that still implement the
deleted coarse sandbox trait.

NNC6.5g may remove obsolete declarations and implementations from those paths.
It may not add behavior there. Earlier caller owners must delete their own
obsolete calls. The final diff must label every handoff path as deletion-only.
This exception keeps one implementation owner and one later legacy-deletion
authority.

## NNCV035 expected-red contract

The audit adds one source-derived verifier using the existing scanner and
assertion patterns. It must not add a second parser.

Planned files:

- `scripts/nimbus-network-control-plane/workload-teardown-source-contract.mjs`
- `scripts/nimbus-network-control-plane/workload-teardown-contract-fixture.mjs`
- `scripts/nimbus-network-control-plane/workload-teardown-test-assertion.mjs`
- `scripts/nimbus-network-control-plane/workload-teardown-contract.sh`
- registrations in `scripts/verify-nimbus-network-source-contract.mjs` and
  `scripts/verify-nimbus-network-control-plane.sh`

The first aggregate run exposed an inherited defect. It was in the future-item
behavior for NNCV034. Its semantic checks read current source. Its historical
path checks compare the NNC6.4a start to the current dirty tree. Freeze only
those path ranges at the exact NNC6.4a item commit. Keep every semantic source
check live. Do not use an aggregate baseline flag or an old source tree to make
NNCV034 pass.

NNCV035 uses the same range rule for its audit-only path census. Before the
NNC6.5 item commit, it checks the current staged and untracked paths. The
post-commit recovery row records the exact NNC6.5 item commit. After that row
exists, NNCV035 compares the NNC6.4a recovery checkpoint to that exact item
commit.

Later product work cannot change the frozen audit range. Current semantic
source checks remain live. NNCV035 resolves the item checkpoint through Git and
maps a missing or invalid commit to its named `paths` diagnostic. NNCV008
separately validates the Recovery Header checkpoint.

Diagnostic groups are `vocabulary`, `reducer`, `command`, `order`, `service`,
`definition-delete`, `compose`, `machine`, `ingress`, `tenant`, `compensation`,
`behavior`, `network`, `paths`, and `ledger`.

The current product source must fail only the 11 implementation groups:
`reducer`, `command`, `order`, `service`, `definition-delete`, `compose`,
`machine`, `ingress`, `tenant`, `compensation`, and `behavior`. The direct
result is `0 passed, 11 failed`. After aggregate registration, NNCV035 is the
only failing condition.

The green fixture has 54 sole-diagnostic mutations:

- vocabulary 3.
- reducer 5.
- command 5.
- order, including restart settlement before withdrawal, 5.
- service 4.
- definition deletion 4.
- Compose 4.
- machine 5.
- ingress 4.
- tenant 3.
- compensation 3.
- behavior 5.
- network 2.
- paths, including an unusable completed-item checkpoint, 2.
- ledger 1.

The self-test must report `55 passed, 0 failed`. Measure aggregate retained
mutation arithmetic from the live helper. The expected starting value
is `413 + 55 + 1 = 469` only if no earlier retained suite changes. A separate
positive fixture proves that future product paths do not alter the completed
audit range.

## Behavioral proof roster

Portable and compute tests must prove:

1. complete attempt identity and strict wire tamper rejection.
2. persistence before claim and effect.
3. exact teardown order and observation prefixes.
4. exact stale/crossed/duplicate result rejection.
5. ambiguous claim and effect recovery by inspection only.
6. same-attempt, next-epoch retry once after exact incomplete inspection.
7. `CleanupPending` retains exact references and inspections.
8. pending provision/restart races settle before teardown.
9. no-reference steps make zero provider calls.
10. one effect under contender and replay races.
11. crash after every claim and after every effect before result CAS.
12. pre-submission and post-submission cancellation rules.
13. failed provision compensates only effects that exact evidence permits.

Upper integration tests must include:

- `service_stop_persists_then_observes_complete_teardown_order`.
- `sandbox_stop_persists_then_observes_complete_teardown_order`.
- `force_delete_unresolved_submission_keeps_definition_and_makes_zero_stop_effects`.
- `definition_delete_keeps_source_and_sessions_until_recorded_teardown`.
- `definition_delete_fences_and_joins_inflight_provision_before_removing_source`.
- `late_provision_result_after_force_delete_is_retired_before_definition_removal`.
- `compose_down_local_uses_engine_saga_and_compute_teardown`.
- `compose_down_forwarded_uses_engine_saga_and_exact_machine_phases`.
- `compose_down_unresolved_submission_makes_zero_provider_calls`.
- `compose_down_replay_is_idempotent_and_reports_durable_outcome`.
- `parent_publication_withdraws_before_guest_stop_and_releases_after_exact_absence`.
- `crossed_machine_teardown_fences_fail_before_effects`.
- `machine_stop_rejects_active_workload_saga_authority`.
- `standalone_machine_stop_fails_closed_without_engine_drain_authority`.
- `final_ingress_withdrawal_cancels_joins_closes_and_settles_exact_leases`.
- `ingress_settlement_failure_retains_cleanup_and_blocks_recorded`.
- `tenant_delete_waits_for_every_durable_workload_teardown_before_storage_delete`.
- `failed_service_start_enters_durable_compensation_without_caller_stop`.
- `failed_sandbox_start_enters_durable_compensation_without_caller_stop`.
- `restart_result_is_settled_before_withdrawal_committed`.

Provider tests must cover a countable 90-case starting matrix:

- seven adapters by exact execute, replay, stale generation, and crossed
  attempt/transition: 28 cases.
- four workload providers by absent, present, and unknown stop observation:
  12 cases.
- Container and Krun at 12 durable/effect crash boundaries: 24 cases.
- parent/guest two-realm retirement: 10 cases.
- pre-effect and adopted-never-spawned compensation: 10 cases.
- six no-premature-reuse states for port, cleanup, IPAM, namespace, segment,
  and stale finalization: 6 cases.

Crash cases use a real subprocess that reopens the same durable authority. An
in-memory clone is not sufficient.

Keep existing SDK stop/delete route parity unchanged. Do not add a route.

## Failure and recovery matrix

| Cut or failure | Required durable result | Forbidden result |
| --- | --- | --- |
| Store unavailable before withdrawal | No provider command exists. | Any stop, detach, or release call. |
| Claim commit ambiguous | Exact fresh read. Use an Inspect-only confirmed command if the claim exists. | Reissue Execute from memory. |
| Process dies after claim | Fresh driver inspects exact attempt. | New attempt or new provider selection. |
| Process dies after effect before result CAS | Provider inspection settles the same attempt. | Duplicate effect. |
| Provider says in progress or ambiguous | Persist/retain inspection requirement. | Advance or release. |
| Exact inspection proves effect absent/incomplete | Same attempt may advance or receive one next-epoch Execute as defined by the step. | Change attempt identity. |
| Crossed identity, generation, epoch, reference, or result | Fail before effect and preserve durable bytes. | Best-effort cleanup of the crossed subject. |
| Withdrawal settlement fails | Keep publication cleanup pending. | Drain, stop, or `Recorded`. |
| Execution absence unknown | Keep exact inspection requirement. | Detach network. |
| Detach unknown | Keep port, IPAM, segment, and provider authority fenced. | Release or reuse. |
| Release partially fails | Enter cleanup pending with retained evidence. | `Recorded` or capacity reuse. |
| Provision succeeds after stop request | Persist the result, then retire that exact generation. | Drop the late result or remove desired source. |
| Provision is exactly absent after stop request | Enter teardown without provision retry. | Create resources only to tear them down. |
| Tenant has unresolved child saga | Keep Engine tenant-delete fence open. | Finish storage deletion. |

## Dependencies and exclusions

Required dependency direction remains:

```text
nimbus-network -> nimbus-core
nimbus-workloads -> nimbus-network
nimbus-compute -> nimbus-workloads + upper capability owners through adapters
```

Forbidden additions include `nimbus-network` imports of sandbox, tenant,
services, server, node, system, Axum, Iroh, Pingora, Netavark, nftables,
gvproxy, cloud SDKs, or cluster transport.

This split does not own:

- service-name/cache lookup fencing, owned by NNC6.6.
- automatic startup recovery and final tenant enumeration, owned by NNC6.1e2.
- cleanup-pending finalization and capacity reuse, owned by NNC8.3.
- logical naming, tenant admission, policy, TLS certificate selection, PEP
  forwarding, machine VMM effects, cluster membership, or system projections.

## Owned paths for NNC6.5 audit

NNC6.5 may change only:

- this proof.
- `docs/private/plans/nimbus-network-control-plane-plan.md`.
- `docs/private/plans/README.md`.
- the four NNCV035 helper/fixture files.
- `scripts/verify-nimbus-network-source-contract.mjs`.
- `scripts/verify-nimbus-network-control-plane.sh`.
- `scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs`
  only for the historical path-range end checkpoint described above.

All product Rust and package source is forbidden until NNC6.5 is complete and
NNC6.5a is active.

## Full-review disposition

The sole full reviewer was GPT-5.6 Sol with xhigh reasoning and fast service.
It reported one P1 and four P2 findings at overall confidence `0.98`. All five
findings have evidence. The integration owner accepts all five.

| Finding | Priority | Disposition and correction |
| --- | --- | --- |
| Audit-only path allowlist would reject future implementation candidates | P1 | Accepted. NNCV035 now uses current paths only before the item commit. The recovery row supplies the exact item end checkpoint. Later paths do not enter the frozen audit range. |
| Restart-to-teardown race was absent from NNCV035 | P2 | Accepted. The fixture, source condition, behavior roster, and one sole-diagnostic mutation now require restart settlement before `WithdrawalCommitted`. |
| Container/Krun drain had no implementation owner | P2 | Accepted. NNC6.5d now owns sandbox execution drain and stop as well as detach and release. |
| NNC6.5a-NNC6.5g lacked product path sets | P2 | Accepted. The prospective path section freezes primary ownership and one deletion-only convergence handoff. |
| The node authority census was incomplete | P2 | Accepted. The authority table now records the exact reconciler caller, DirectProcess and Systemd effects, projection ordering, failure gap, and target seam. |

The first two corrections change executable verifier code. After the affected
proofs pass, run one narrow correction review for all five dispositions. Do not
run a second full review.

## Narrow-review disposition

The one narrow GPT-5.6 Sol/xhigh/fast review reported three P2 findings at
overall confidence `0.98`. All three findings have evidence. The integration
owner accepts all three. The plan's review cadence is complete. The plan permits
no third structured review.

| Finding | Priority | Disposition and correction |
| --- | --- | --- |
| A completed row without a usable item checkpoint failed open | P2 | Accepted. The parser now distinguishes an in-progress item from a completed item. A completed row with a missing, malformed, or unresolved checkpoint produces only the named `paths` diagnostic. |
| Restart settlement tokens did not prove order | P2 | Accepted. The source-derived driver order now requires restart settlement, late-result retention, and the committed-withdrawal handoff before `persist_withdrawal_committed`. The focused mutation moves withdrawal before settlement and fails only `order`. |
| The NNC6.5b wildcard overlapped NNC6.5c and NNC6.5d | P2 | Accepted. NNC6.5b now lists six exact compute modules and same-named test children. It explicitly excludes the node and sandbox adapter paths. |

After the review, the integration owner inspected the dependency-ready NNC6.5a
packet and found one documentation-only ownership omission. Exact
provision/restart validators,
strict server fixtures, and three compute test-fixture files must change with
the portable format replacement. The prospective path section now assigns
those paths to NNC6.5a and removes the three compute fixtures from NNC6.5b.
This correction moves no compute production behavior and creates no primary
path overlap.

## Audit acceptance ledger

| Criterion | Status | Evidence |
| --- | --- | --- |
| A1 current portable phase/evidence graph | `pass` | Workloads source and state validation census above. |
| A2 pure recovery consumer census | `pass` | No production teardown action consumer found. |
| A3 service stop graph | `pass` | Compute -> resource provisioner -> services direct stop. |
| A4 sandbox stop graph | `pass` | Compute -> services direct stop. |
| A5 definition delete race graph | `pass` | In-memory claim does not fence provision. Removal follows direct stop. |
| A6 tenant delete graph | `pass` | Projection enumeration and best-effort direct stop confirmed. |
| A7 Compose local/forwarded graph | `pass` | Both paths use coarse `SandboxBackend::stop`. Down does not compose the Engine. |
| A8 machine/guest graph | `pass` | Coarse stop wire lacks teardown fences. Parent ordering is wrong for final teardown. |
| A9 ingress authority graph | `pass` | Restart-only withdrawal and non-authoritative Drop settlement confirmed. |
| A10 Container/Krun provider graph | `pass` | Coarse stop plus shared durable attachment lifecycle confirmed. |
| A11 failed-provision compensation gap | `pass` | Definite failure halts without recovery or compensation. |
| A12 provision/restart race gap | `pass` | Pending provision rejects stop. Restart veto has no teardown handoff. |
| A13 target owner map | `pass` | One compute coordinator exists. Effects stay with named owners. |
| A14 portable protocol freeze | `pass` | Exact types, identity, outcome, and retry rules recorded. |
| A15 provider capability freeze | `pass` | Five small capabilities and exact selection recorded. |
| A16 lifecycle/race freeze | `pass` | Required order and late-result rules recorded. |
| A17 failure/recovery freeze | `pass` | Failure matrix records every persistence/effect boundary. |
| A18 later-owner reconciliation | `pass` | NNC6.6, NNC6.1e2, and NNC8.3 exclusions recorded. |
| A19 prospective item split | `pass` | NNC6.5a-NNC6.5g have dependency order, exclusive primary product path sets, exact portable provision/restart validator ownership, compile-green fixture ownership, and one explicit deletion-only handoff. |
| A20 product path freeze | `pass` | No product source changed during audit. |
| A21 main-overlap check | `pass` | Incoming product overlap is outside planned teardown sources. The plan index needs later reconciliation. |
| A22 NNCV035 expected-red implementation | `pass` | Corrected current source is exact `0/11`; the green and future-path fixtures pass; focused mutations are `55/55`, including restart-order and invalid-completed-checkpoint cases; NNCV034 is `86/86`; live aggregate is `35/36` with sole expected-red NNCV035; retained aggregate mutations are `469/469`. |
| A23 static/docs/recovery-ledger gates | `pass` | JavaScript and Bash syntax, Prettier, scoped ShellCheck, diff, proof lint with zero diagnostics, NNCV001-NNCV034, recovery-ledger, docs `108`, and site `17/17` gates pass. Product source remains unchanged. |
| A24 candidate-frozen item review | `pass` | The one full Sol/xhigh/fast review reported five accepted findings at `0.98`. The one narrow correction review reported three accepted P2 findings at `0.98`. All eight findings are corrected and proven. The later owner inspection corrected one documentation-only path omission. No third structured review ran or is warranted. |

The full and narrow reviews are complete. All A1-A24 criteria pass. Do not run
another structured review. Commit the exact NNC6.5 item, then record its commit
in the recovery row and activate NNC6.5a.
