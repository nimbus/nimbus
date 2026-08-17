# NNC6.5d Sandbox And Machine Teardown Substitution Audit

Status: `complete; A1-A20 green; product source unchanged`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Scope

NNC6.5d originally combined Container execution drain and stop, Krun execution
drain and stop, shared host-managed attachment detach and release, and
forwarded-machine teardown. These concerns share the compute teardown protocol,
but they do not share one provider state machine, one failure surface, or one
reviewable value boundary.

This audit freezes a prospective split before the first product-source edit.
It does not signal a process, stop a sandbox, mutate an attachment, release a
lease, change a Machine API request, or register a production capability.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| A1 | The source census names every current Container, Krun, shared OCI, PEP, port, IPAM, segment, machine-publication, provider-command, compute-capability, and guest/parent teardown authority in scope. |
| A2 | Current and target call graphs distinguish execution drain, execution stop, network detach, network release, final ingress withdrawal, provider progress, durable dispatch idempotency, and upper-saga coordination. |
| A3 | The audit proves from source that Container and Krun coarse stop currently collapse execution stop with network detach/release and that Container sends TERM/KILL before durable stop intent. |
| A4 | The audit identifies which current journals and state machines remain canonical and forbids a second provider-command, attachment, IPAM, publication, PEP, port, or segment authority. |
| A5 | The audit defines an honest sandbox drain contract without claiming application-internal request settlement that the provider cannot observe. |
| A6 | The audit defines exact stop success as the commanded execution attempt being terminal or explicitly absent while network authority remains fenced. |
| A7 | The audit defines exact detached-but-not-released evidence without adding a speculative portable phase or misusing restart reprovisioning state. |
| A8 | The audit defines release prerequisites and proves that port, PEP, IPAM, segment, attachment, and provider authority cannot be reused early. |
| A9 | The audit resolves network-command manifest location from retained tenant-qualified execution evidence rather than IP address, global ID guessing, or directory-order authority. |
| A10 | The audit separates direct host-managed attachment identity from the admitted forwarded-machine provider identity and preserves guest-local versus parent-host authority. |
| A11 | Raw numeric PID is rejected as stable execution identity; every signal path must authenticate the exact runtime attempt and current process/provider identity immediately before the effect. |
| A12 | NNC6.5d1-NNC6.5d4 each have an explicit dependency, owned path set, forbidden path set, behavioral value, fail-before roster, crash/restart matrix, and deletion handoff. |
| A13 | The split keeps coarse product stop composition until the NNC6.5e-NNC6.5g caller and deletion gates; no compatibility wire format, optional no-op capability, or second coordinator is introduced. |
| A14 | Every Inspect operation is read-only, synchronizes with the exact provider-owned command/effect evidence, and reports `NotCompleted` only when no older effect can still commit. |
| A15 | Every stale, crossed, corrupt, duplicate, or unknown case has a named fail-closed result, byte-preservation requirement, and zero-effect assertion. |
| A16 | Every ambiguous effect has an inspect-before-retry proof; a same-attempt retry advances only by one dispatch epoch after exact absence. |
| A17 | Files at or above repository complexity thresholds are split by concept ownership or retain an explicit, source-derived ownership reason. |
| A18 | The target preserves `nimbus-network -> nimbus-core` as the only network-crate workspace edge and adds no socket, process, Machine API, Netavark, PEP, IPAM, or forwarding effect to `nimbus-network`. |
| A19 | The canonical band table, checkpoint ledger, Recovery Header, and routing index remain mutually recoverable with exactly one `in_progress` item. |
| A20 | Static plan verification, NNCV035 expected-red arithmetic, docs gates, proof writing lint, format/diff checks, and exactly one candidate-frozen GPT-5.6 Sol/xhigh/fast item review pass with recorded results. |

## Current Source Census

The census is product-source only unless a row says otherwise. Test modules are
proof consumers, not lifecycle authorities.

| Concern | Current source-derived result |
| --- | --- |
| Compute teardown capabilities | `crates/nimbus-compute/src/workload_saga/teardown_registry.rs` defines separate execution drain, execution stop, network detach, and network release ports. Exact registry selection has no fallback. |
| Confirmed command | `teardown_command.rs` carries the confirmed claim, source, and complete compiled network plan. A network step carries only `WorkloadNetworkReference` as its typed subject today. |
| Retained locator evidence | `WorkloadPhaseDetail::references()` retains the exact execution reference during teardown, but `ConfirmedWorkloadTeardownCommand` does not yet carry it to a network adapter. |
| Provider dispatch journal | `crates/nimbus-sandbox/src/provider_command.rs` owns claim-before-effect, exact replay, terminal observation, ambiguity, and next-epoch-after-absence. Its operation classification currently understands only provision and restart. |
| Container coarse stop | `runtime.rs:645-734` reaches `execute_stop`; `runtime.rs:1241-1271` sets stop intent only in memory, sends TERM/KILL, then calls `release_execution_artifacts` before final manifest persistence. |
| Container cleanup | `runtime/execution_cleanup.rs:27-268` combines runtime deletion, PEP stop, host or machine publication withdrawal, provider/netns detach, listener settlement, IPAM/segment release, launch-artifact cleanup, and one final Boolean. |
| Krun coarse stop | `vm/lifecycle.rs:65-140` reaches `execute_stop`; `vm/lifecycle.rs:439-477` durably persists stop intent, sends TERM/KILL, calls final network release, removes launch artifacts, and then persists terminal state. |
| Shared host attachment | `attachment_lifecycle.rs:1073-1435` combines durable detach preparation, provider and namespace removal, listener settlement, IPAM/segment release, and terminal transition. |
| Attachment transition | `attachment_lifecycle/recovery.rs:469-591` moves provider deletion into `Deleting`; restart completion returns to `Provisioning`, while final completion moves to `Released`. |
| Terminal resource release | `attachment_lifecycle/detach_release.rs:5-77` already isolates ordered listener, IPAM, and segment release after confirmed detach. |
| Machine publication | `runtime/machine_port_publication.rs` owns durable batch publication/withdrawal evidence; `runtime/machine_ports.rs` owns local proxy cleanup and lease settlement. |
| Parent machine publication | `crates/nimbus-cli/src/machine/publication_authority.rs` and its `confirmed` child own parent intent, exact members, retirement, and port authority. |
| Forwarded provider identity | `machine/backend/provision.rs:65-75` defines a distinct forwarded attachment and execution provider identity. It is not the guest's host-managed Container identity. |
| Current remote retirement | `machine/backend.rs:72-105` stops the guest before parent publication retirement. The target must withdraw parent publication before guest stop and release only after exact guest/provider absence. |
| Manifest location | `artifact_paths.rs:90-134` can enumerate tenant manifests or look up a globally unique `SandboxId`. It has no exact attachment-to-manifest lookup and must not become directory-order authority. |
| Runtime observation | `backends/conmon/lifecycle.rs:162-248` distinguishes exact exit receipt, present state, explicit absence, and ambiguity. Runtime deletion already inspects after effect. |
| Current signal | `backends/conmon/lifecycle.rs:584-622` reads and signals a numeric PID. PID alone is not stable workload identity and is unsafe after reuse. |

Current handwritten complexity:

| File | Lines | Audit disposition |
| --- | ---: | --- |
| `container/runtime.rs` | 1,576 | Extract execution teardown into a concept-owned child. |
| `container/runtime/restart.rs` | 1,582 | Do not grow it; restart quiescence is not final drain. |
| `container/runtime/runner.rs` | 2,085 | Do not touch unless the item first decomposes it by runner lifecycle concept. |
| `container/runtime/machine_port_publication.rs` | 1,606 | Keep the journal owner coherent; put new teardown tests/translation in children. |
| `container/runtime/machine_ports.rs` | 1,508 | Use its existing lifetime operations; extract new phase composition rather than growing the root. |
| `krun/vm/lifecycle.rs` | 1,516 | Extract execution teardown into `vm/teardown.rs`. |
| `oci/network/attachment_lifecycle.rs` | 1,522 | Move host-managed teardown composition into a concept-owned child. |
| `provider_command.rs` | 1,008 | A narrow closed operation-family extension is coherent. |

## Current Call Graphs

### Container and Krun

```text
SandboxBackend::stop
  -> backend stop_sync
  -> reconcile creator/runner ownership
  -> execute_stop
       -> stop intent
       -> TERM / optional KILL
       -> attachment/provider cleanup
       -> PEP/listener/IPAM/segment release
       -> terminal manifest
```

This call graph cannot produce four honest upper-saga results. One coarse
return currently means that execution and every network authority converged.

### Shared host-managed attachment

```text
release_execution_artifacts / release_network_artifacts(Final)
  -> OciAttachmentLifecycle::detach_host_managed
       -> durable Deleting preparation
       -> backend prerequisite callback
       -> segment quarantine
       -> listener cleanup preparation
       -> provider detach
       -> namespace removal
       -> listener settlement
       -> IPAM release
       -> segment release
       -> attachment Released
```

### Forwarded machine

```text
parent ForwardedMachineApiSandboxBackend::stop
  -> coarse Machine API guest stop
  -> parent publication retirement and port release

guest exact provision path already has
  confirmed envelope -> provider command journal -> Container/node effects
```

The target must reverse the unsafe publication order and use exact teardown
phase envelopes. Parent and guest retain different journals and provider
identities because they own different effects.

## Frozen Architecture Decisions

### D1: Honest sandbox drain

For Container and Krun, `ExecutionDrained` means that:

- the exact final publication step is complete.
- the backend durably rejects a new Nimbus activation, restart, creator handoff,
  or provider dispatch for that execution attempt.
- every creator, activation, restart, and lifecycle operation owned by the
  backend is absent or settled.
- the runtime remains running unless it already exited.

The sandbox backend owns no application protocol queue or application-internal
request counter. It must not claim that private process work is complete when
it cannot observe that state. Server and machine publication owners settle
the listener/proxy work they own before this barrier. A future application-
cooperative drain requires a deliberately admitted provider capability. This
plan does not fabricate that capability.

### D2: Exact stop

Stop succeeds only when the exact execution attempt is terminal or explicitly
absent. Stop does not detach a provider, stop a PEP, settle or release a port,
release IPAM, release a segment, or mark the attachment terminal.

The provider persists exact stop intent before the first signal. It records an
effect-may-exist boundary before each TERM or KILL dispatch. Recovery
authenticates the exact runtime attempt and provider/process identity, inspects
state, and never signals from a stale numeric PID.

The overall sandbox manifest can remain `Stopping` after execution stop. It
becomes terminal `Stopped` only after release completes. Exact execution
absence lives in a concept-owned teardown substate, not in a second command
journal.

### D3: Detached but retained

The target does not add a portable `Detached` phase. The network resource remains in
`NetworkResourcePhase::Deleting` after provider and namespace absence. Exact
detach completes only when all these facts are true:

- the current tenant, attachment, plan, generation, lease epoch, association,
  selected provider, and stable provider handle.
- terminal provider-delete evidence and explicit namespace absence.
- publication absence for every machine-forwarded member when applicable.
- stopped/retained listener and PEP lifetime evidence.
- quarantined segment authority.
- retained IPAM, segment, port, PEP, and attachment authority.
- the exact `DetachNetwork` command-journal success observation.

`AttachmentTeardownMode::Restart` is not final detach. It returns the resource
to `Provisioning` and exists only for same-generation restart.

Release requires the full compound detached proof. It releases retained
listener and PEP authority, IPAM, the segment hold, and then the attachment.
Only release transitions the portable resource to `Released`.

### D4: One provider-command authority

`ProviderCommandAttemptJournal` remains the only sandbox/forwarded provider
dispatch-idempotency journal. Its closed operation family gains teardown
operations without treating teardown as provision or restart:

- provision has no source attempt and restart ordinal zero.
- restart has an exact source attempt and nonzero ordinal.
- teardown has no restart source and restart ordinal zero.
- teardown records its exact attempt and dispatch epoch in existing opaque
  claim fields.

The command journal owns dispatch identity and result. Backend manifests and
provider journals own effect progress. These are complementary, not duplicate,
authorities.

### D5: Exact manifest locator

The compute-confirmed sandbox command carries the retained
`WorkloadExecutionReference` as an authenticated provider locator even for a
network subject. Compute derives it from the same confirmed record and checks
it against the active intent and retained phase references. The portable
network subject remains unchanged.

The sandbox adapter derives `SandboxId` from the stable execution ID, as the
provision/restart adapters already do. It authenticates tenant, execution
attempt, generation, desired/source/plan digest, and attachment association
against the manifest before any journal mutation or effect. It does not scan
by IP address, accept the first manifest, or use a globally ambiguous ID.

### D6: Direct and forwarded provider realms

Direct Container and Krun attachment adapters use their existing host-managed
provider IDs. A forwarded-machine workload uses distinct admitted parent IDs
for attachment, execution, and ingress. The guest can translate an exact remote
phase into local host-managed Container effects. It does not rewrite the parent
saga's provider identity or own its CAS transitions.

The parent provider owns publication withdrawal and final parent lease release.
The guest owns execution and guest attachment effects. Parent release requires
the exact guest response and independent parent observation to prove guest
provider absence. Unknown or partial sibling results retain the whole batch.

## Prospective Implementation Split

| Item | Owned value | Dependency | Completion proof |
| --- | --- | --- | --- |
| NNC6.5d | Read-only sandbox/machine teardown audit and split | NNC6.5c | A1-A20, exact task/ledger routing, no product-source change, static/docs gates, and one candidate-frozen item review. |
| NNC6.5d1 | Exact Container execution drain/stop adapter plus the earned shared teardown command substrate | NNC6.5d | Exact command authentication, one provider journal, durable drain/stop progress, intent-before-signal, authenticated runtime identity, replay/ambiguity/process cuts, and real compute substitution. No network detach/release. |
| NNC6.5d2 | Exact Krun execution drain/stop adapter | NNC6.5d1 | The same execution contract with Krun-specific creator/runtime evidence, no raw PID authority, no network detach/release, and real compute substitution. |
| NNC6.5d3 | Shared host-managed Container/Krun attachment detach/release adapters | NNC6.5d1-NNC6.5d2 | `Deleting` plus compound detached proof, distinct final release, exact manifest location, two real backend substitutions, fresh-process crash matrix, and no premature reuse. |
| NNC6.5d4 | End-to-end forwarded-machine teardown provider adapters and exact guest phase envelopes | NNC6.5d3 | Distinct admitted provider IDs, exact parent/guest commands, parent publication withdrawal before guest stop, release after exact guest/provider absence, sibling-batch fencing, process recovery, and no caller cutover. |

NNC6.5e depends on NNC6.5d3 for native Container/Krun service and sandbox
teardown. NNC6.5f depends on NNC6.5d4 for Compose and machine caller cutover.
NNC6.5g remains the only legacy deletion and final NNCV035 convergence gate.

## Frozen Path Ownership

The canonical plan, routing index, item proof, and shared static verification
are integration-owner paths. Later sub-items can make a mechanical module
registration edit in a shared root only when the root contains no lifecycle
logic.

### NNC6.5d audit only

- this proof.
- `docs/private/plans/nimbus-network-control-plane-plan.md`.
- `docs/private/plans/README.md`.

Product source is forbidden.

### NNC6.5d1 Container execution

Primary ownership:

- `crates/nimbus-compute/src/workload_saga/teardown_sandbox.rs` plus
  `teardown_sandbox/container.rs` and attributed tests.
- the narrow confirmed-command retained-locator addition.
- `crates/nimbus-sandbox/src/provider_command.rs` and its tests.
- a neutral sandbox-owned teardown claim/result module and narrow exports.
- `crates/nimbus-sandbox/src/backends/container/runtime/teardown.rs` and
  concept-owned children/tests.
- narrow Container manifest and runtime composition-root edits.
- exact conmon runtime/process identity and signal-provider changes.

Forbidden: Krun behavior, attachment detach/release, machine transport,
services/Compose/caller cutover, coarse-stop deletion, and `nimbus-network`
effects or dependencies.

### NNC6.5d2 Krun execution

Primary ownership:

- `teardown_sandbox/krun.rs` and attributed tests.
- `crates/nimbus-sandbox/src/backends/krun/vm/teardown.rs` and children/tests.
- narrow Krun manifest, inspection, lifecycle, and composition-root edits.
- shared conmon signal identity only if D1 did not already complete the common
  mechanism.

Forbidden: attachment detach/release, machine transport, caller cutover,
coarse-stop deletion, or a second journal.

### NNC6.5d3 Host-managed attachment

Primary ownership:

- `teardown_sandbox/attachment.rs` and tests.
- a sandbox-owned neutral network teardown contract.
- `backends/oci/network/attachment_lifecycle.rs` plus concept-owned host
  teardown, recovery, and detach/release children.
- narrow port-lifetime, PEP cleanup, IPAM, segment, and manifest authentication
  changes with attributed tests.
- Container/Krun network-teardown composition children.

Forbidden: runtime stop effects, forwarded Machine API transport, parent
publication authority, caller cutover, legacy deletion, or portable-state
phase growth without a new owner decision.

### NNC6.5d4 Forwarded machine

Primary ownership:

- a concept-owned `machine/backend/teardown.rs` parent adapter and tests.
- an exact `machine/api/service_workloads/teardown.rs` guest handler and tests.
- the exact Machine API teardown envelope, client/route translation, and
  command journal reuse.
- parent `publication_authority` confirmed-retirement children and port-lifetime
  tests.
- narrow composition exports needed to prove real capability substitution.

Forbidden paths include Compose down, other product callers, and physical-
machine stop. Tenant policy, service naming, and a CLI-local saga store are
also forbidden. The item cannot add a public route or delete coarse stop.

## Fail-Before Roster

Every implementation sub-item must record expected-red evidence before its
behavioral edit. A fail-before result changes no provider-side durable byte and
makes zero provider effects. Compute can persist the authenticated result only
through the existing result CAS after the adapter returns.

The table uses closed compute outcomes.

`DefiniteFailure` includes the stable failure code shown in parentheses.
`Ambiguous` requires read-only inspection before any retry. `InProgress`
requires exact live-owner evidence. An adapter returns `NotCompleted` only
after exact authoritative absence. That evidence must prove that no older
operation can finish. Exact replay adopts the current journal observation
without a new claim or effect.

| Case | Required result | Durable-byte and effect proof |
| --- | --- | --- |
| Wrong step or subject kind | `DefiniteFailure(sandbox_teardown_command_invalid)` | Reject before manifest lookup, journal claim, or effect. |
| Crossed provider ID or role | `DefiniteFailure(sandbox_teardown_command_crossed)` | Preserve the manifest and all journals. Do not fall back to another provider. |
| Crossed tenant, workload UID, execution ID, attempt, restart epoch, node, or generation | `DefiniteFailure(sandbox_teardown_command_crossed)` | Reject before path derivation or provider inspection. |
| Substituted desired, source, network-plan, selection, or provider-target digest | `DefiniteFailure(sandbox_teardown_command_crossed)` | Reject before journal mutation or effect. |
| Stale generation or dispatch epoch | `DefiniteFailure(sandbox_teardown_command_stale)` | Retain the newer record byte for byte. Do not inspect or affect the stale target. |
| Skipped dispatch epoch or crossed command or transition ID | `DefiniteFailure(sandbox_teardown_epoch_invalid)` | Retain the current attempt. Only exact absence can authorize exact epoch plus one. |
| Exact duplicate command and epoch | Exact replay | Map `Claimed` or `InProgress` to `InProgress`, success or absence to `Satisfied`, failure to `DefiniteFailure`, and ambiguity to `Ambiguous`. Make no new effect. |
| Missing manifest or attachment association | `Ambiguous`, unless an exact terminal journal observation can be replayed | Never infer absence from a missing file. Preserve every surviving artifact. |
| Corrupt manifest, journal, or association | `Ambiguous` | Quarantine the bytes for diagnosis. Do not overwrite, repair by guess, or run an effect. |
| Crossed manifest or attachment association | `DefiniteFailure(sandbox_teardown_command_crossed)` | Preserve both the requested and discovered authorities. |
| Pending creator, activation, or restart work | Execute returns `Ambiguous`; Inspect returns `InProgress` with exact owner evidence | The drain barrier blocks new work. Existing work must settle or become conclusively absent. |
| Raw PID without exact runtime attempt and process birth identity | `Ambiguous` | Never signal. Preserve the runtime record and inspect through the authenticated process seam. |
| Detach before exact execution terminality | `DefiniteFailure(sandbox_teardown_order_invalid)` | Preserve all attachment, provider, listener, PEP, IPAM, and segment state. |
| Release before compound detached proof | `DefiniteFailure(sandbox_teardown_order_invalid)` | Release no reusable authority. |
| Active listener, PEP, publication, IPAM, or segment evidence during release | Execute returns `Ambiguous`; Inspect returns `InProgress` with exact owner evidence | Keep every lease and hold fenced. |
| Unknown listener, PEP, publication, IPAM, segment, provider, namespace, or process state | `Ambiguous` | Inspect before retry. Unknown state never becomes absence. |
| Stale callback after successor generation or provider replacement | Typed stale callback rejection | Preserve the successor and the stale evidence. The callback cannot publish an outcome or run an effect. |
| Address or port offered as workload identity | `DefiniteFailure(sandbox_teardown_identity_invalid)` | Reject before lookup. Only stable tenant-qualified IDs can select authority. |

Forwarded-machine cases add these results:

| Case | Required result | Durable-byte and effect proof |
| --- | --- | --- |
| Parent versus guest provider-ID substitution | `DefiniteFailure(machine_teardown_provider_crossed)` | Preserve both realms and send no Machine API request. |
| Crossed forwarder instance or generation | `DefiniteFailure(machine_teardown_forwarder_stale)` | Preserve the current parent binding and guest record. |
| Missing or partial guest response | `Ambiguous` | Retain the parent request-may-exist record and every lease. Inspect the exact guest command before retry. |
| One absent publication member with one present sibling | `InProgress` with exact batch evidence | Retain the complete parent batch and all port leases. |
| One absent publication member with one unknown sibling | `Ambiguous` | Retain the complete batch. Inspect the unknown sibling before any retry or release. |
| Parent release before exact guest detach and provider absence | `DefiniteFailure(machine_teardown_order_invalid)` | Release no parent port or publication authority. |
| Standalone machine stop while durable workload authority is active | Typed `ActiveWorkloadTeardownRequired` conflict | Make zero machine-stop effects. NNC6.5f routes the caller through the workload saga. |

## Crash, Restart, And Concurrency Matrix

Local recovery starts a new process over the same Engine, manifest, provider,
and lease roots for that one host realm. It receives no in-memory snapshot.
Each of the four capabilities owns a separate workload claim, confirmed
command, provider-journal claim, provider outcome, and compute result CAS. The
provider records one capability completion before compute can claim the next.

Every local capability runs this independent outer matrix:

1. the workload claim is durable before confirmed command construction.
2. the exact provider-journal claim is durable before provider progress.
3. phase-local progress or effect-may-exist evidence is durable before the
   related effect.
4. the effect returns, loses its response, or the process dies.
5. a new process adopts the exact provider claim and inspects before retry.
6. the provider observation is durable before it returns to compute.
7. compute has the exact result but dies before its result CAS.
8. a new compute process inspects and commits the same exact result.
9. the result CAS is durable while the next capability has no claim.
10. only then can the next capability create its independent claim and epoch.

`DrainExecution` adds these phase-local cuts for each backend:

1. provider-admission barrier intent is durable before admission closes.
2. the barrier rejects new creator, activation, restart, and provider dispatch
   work.
3. each already-admitted provider-owned operation settles or becomes
   conclusively absent.
4. exact drained evidence is durable while the execution is still running.

`StopExecution` adds these phase-local cuts for each backend:

1. stop intent is durable before runtime inspection.
2. TERM-may-exist evidence is durable before TERM dispatch.
3. TERM returns or loses its response before an exit receipt.
4. the timeout expires before KILL preparation.
5. KILL-may-exist evidence is durable before KILL dispatch.
6. KILL returns or loses its response before an exit receipt.
7. exact execution terminality or absence is durable while all network
   authority remains fenced.

`DetachNetwork` adds these host-managed cuts for each backend:

1. `Deleting` and segment quarantine are durable.
2. local listener and PEP stop intent is durable before each stop effect.
3. listener and PEP retained-state settlement is durable.
4. provider-detach-may-exist evidence is durable before provider deletion.
5. provider deletion returns or loses its response.
6. exact provider absence is durable before namespace removal.
7. namespace-removal-may-exist evidence is durable before removal.
8. explicit namespace absence and the compound detached proof are durable.
9. every IPAM, segment, port, PEP, listener, and attachment authority remains
   retained and fenced.

`ReleaseNetwork` adds these host-managed cuts for each backend:

1. the adapter reauthenticates the full compound detached proof before release
   intent.
2. listener and PEP final-release intent is durable before each release.
3. exact listener and PEP release is durable before IPAM release intent.
4. IPAM-release-may-exist evidence is durable before the IPAM effect.
5. segment-release-may-exist evidence is durable before the segment effect.
6. all reusable authority is absent before the attachment becomes `Released`.

Forwarded-machine recovery uses two independent durable realms. The parent
cannot open or infer guest durable state. The guest cannot mutate the parent
journal, port lease, publication batch, or workload CAS.

Every remote guest phase runs this two-realm matrix:

1. the parent workload phase claim and parent provider-journal claim are
   durable.
2. an exact parent request ID and request-may-exist record are durable before
   Machine API transmission.
3. the parent can stop before send or die after send. The transport can deliver
   the request and lose its response.
4. the guest authenticates the complete envelope before its own workload claim
   and provider-journal claim.
5. the guest dies and recovers at every applicable local drain, stop, detach,
   or release cut above.
6. the guest phase outcome is durable before response transmission.
7. the transport delivers or loses the response, or either realm restarts.
8. parent recovery adopts the exact request and uses the read-only exact guest
   inspection envelope before any retransmission.
9. guest inspection returns `InProgress`, `Ambiguous`, `DefiniteFailure`, or
   exact `Satisfied` evidence from its own durable realm.
10. the parent persists the exact guest result, then independently proves the
    parent-visible provider state required for that phase.
11. the parent provider outcome is durable before the parent compute result
    CAS.
12. only the committed parent phase can authorize the next remote phase.

Parent publication withdrawal has its own exact ingress command and member
batch. A strict subset of retired members keeps the full batch and all port
leases fenced. Unknown sibling state is `Ambiguous`. A known live sibling is
`InProgress`. Guest drain cannot start before exact parent withdrawal. Final
parent release cannot start before exact guest detach, guest release, and
independently observed guest provider absence.

Concurrency proofs use two synchronized Execute contenders and one Inspect
contender. Only one claim/effect wins. Inspect cannot return `NotCompleted`
while an older exact effect is live or can still publish its outcome.

## Acceptance Commands

Audit closeout:

```sh
git diff --check
cargo fmt --all --check
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --self-test
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --check
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

The direct teardown contract must remain expected red with the same seven
later-owner diagnostics. Its self-test remains `55 passed, 0 failed`.

Implementation closeout, with exact counts recorded by each item proof:

```sh
cargo test -p nimbus-sandbox provider_command
cargo test -p nimbus-sandbox container_teardown -- --test-threads=1
cargo test -p nimbus-sandbox krun_teardown -- --test-threads=1
cargo test -p nimbus-sandbox attachment_lifecycle -- --test-threads=1
cargo test -p nimbus-sandbox fresh_process_teardown -- --test-threads=1
cargo test -p nimbus-compute teardown_sandbox -- --test-threads=1
cargo test -p nimbus-cli machine_teardown -- --test-threads=1
cargo test -p nimbus-sandbox
cargo test -p nimbus-compute
cargo test -p nimbus-cli
cargo clippy -p nimbus-sandbox -p nimbus-compute -p nimbus-cli --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p nimbus-sandbox -p nimbus-compute -p nimbus-cli --no-deps
cargo fmt --all --check
```

Each item also runs the network dependency/effect scan, changed-file
modularity census, NNCV035 self-test/direct/aggregate gate, proof lint, docs,
and site gates. A candidate-frozen item gets exactly one full Sol/xhigh/fast
review. Only an accepted executable defect permits one narrow correction
review.

## Retained Later Owners And Non-Goals

- NNC6.5e owns native service/sandbox/definition caller cutover.
- NNC6.5f owns Compose, guest/forwarded composition, and physical-machine
  caller cutover after the real machine provider adapters exist.
- NNC6.5g owns failed-provision compensation, tenant retirement, coarse stop
  deletion, and final NNCV035 convergence.
- NNC6.6 owns logical service-resolution fencing during withdrawal.
- NNC6.1e2 owns final startup recovery and tenant-retirement convergence.
- NNC8.3 owns orphan cleanup finalization and capacity reuse after cleanup.
- `nimbus-network` remains transport-free and effect-free.
- `nimbus-services` keeps logical names and source/session policy.
- `nimbus-tenant` keeps admission and quota policy.
- `nimbus-proxy` keeps forwarding effects. PEP policy remains separate.
- Future cluster transport, membership, routing, and super-net fencing remain
  outside this item.

No item adds a god teardown provider or application protocol drain fiction.
No item adds a compatibility decoder, feature flag, or optional no-op adapter.
No item adds a public route, CLI-local saga store, or IP-address identity.

## Structured Review And Disposition

The sole full item review ran against staged tree
`d3eec42a26c70e81b667c96847b9c9b147eb0518`. The complete patch SHA-256 was
`aa5f7c2564c31c2f012f45bc3ddb4dd8a7e60cc66ebc7ff707a77b0986d72bc2`.
The executable patch was empty. The wrapper confirmed GPT-5.6 Sol, xhigh
reasoning, fast service tier, one 59,598-byte pass, and thread
`019fe7a4-67eb-77f1-b284-2e16684abcdb`. It reported three P2 findings at
overall confidence 0.99.

| Finding | Disposition | Evidence and correction |
| --- | --- | --- |
| The fail-before roster did not assign exact outcomes. | `accepted` | The outcome tables now map every common and forwarded stale, crossed, corrupt, duplicate, missing, partial, and unknown case. Each row names replay, typed rejection, `DefiniteFailure`, `InProgress`, or `Ambiguous`, plus byte-preservation and zero-effect behavior. `NotCompleted` requires exact authoritative absence. |
| The crash matrices combined drain with stop and detach with release. | `accepted` | A common outer matrix now requires an independent workload claim, confirmed command, provider-journal claim, provider outcome, compute result CAS, and fresh-process recovery for each capability. Four separate phase-local matrices prevent recombination. |
| Forwarded-machine recovery used a single-root premise. | `accepted` | The proof now defines independent parent and guest durable realms. The two-realm matrix covers request-may-exist persistence, guest durability before response, response loss, either-side restart, exact remote inspection, parent observation, and phase-by-phase CAS order. |

All corrections are documentation-only. They change no executable code or
effect owner. The affected static, writing, format, diff, and documentation
gates pass. The review cadence does not authorize a narrow correction review.

## Evidence Ledger

| Checkpoint | Evidence |
| --- | --- |
| Read-only audit base | `bbb3098dae475e16b3d0a4a456267eaba6a7d62a`; owner worktree clean; original checkout untouched. |
| Parallel source audits | Three read-only packets independently covered Container/shared host cleanup, Krun/shared host cleanup, and attachment/machine journals. Each reported zero changed paths and the same clean HEAD. |
| Current expected red | NNCV035 self-test is `55/55`; the direct contract is `0/7`, with only service, definition-delete, Compose, machine, tenant, compensation, and behavior diagnostics remaining. |
| Plan split | NNC6.5d plus NNC6.5d1-NNC6.5d4 occur once in both task and checkpoint ledgers. NNCV008 accepts the exact bijection and sole active NNC6.5d1 row. |
| Quality and static gates | `git diff --check` and `cargo fmt --all --check` pass. Proof writing lint reports zero diagnostics. The post-correction aggregate is exact `35/36` with sole NNCV035 red. |
| Documentation gates | `scripts/check-docs.sh` passes `108` pages. `scripts/verify-nimbus-docs-site.sh` passes `17/17` conditions. |
| Structured review | One full GPT-5.6 Sol/xhigh/fast review reported three P2 documentation findings at confidence 0.99. All are accepted, corrected, and proven above. No narrow review ran because executable code did not change. |
| Final commit | The commit containing this proof is the exact three-path NNC6.5d item checkpoint. |

## Acceptance Traceability

| Clause | Candidate evidence |
| --- | --- |
| A1-A4 | Current source census, call graphs, and D4 above. |
| A5-A11 | D1-D3 and D5-D6 above. |
| A12-A18 | Prospective split, path ownership, fail-before/crash matrices, retained owners, and non-goals above. |
| A19 | Canonical plan and checkpoint rows, Recovery Header, routing index, and passing NNCV008 proof above. Exactly one item is `in_progress`. |
| A20 | Static, expected-red, proof-lint, format/diff, and documentation results above are green. The sole candidate-frozen review and all three accepted documentation corrections are recorded above. |
