# NNC5.4a — Machine-Forwarded Batch Convergence

Status: `done; R1-R20 green; exact item checkpoint`

Source checkpoint:

- commit: `239c9a5523d38350c0a74348f1501f0cb014ff2a`
- tree: `4b7e54e5d1db8cec46de8fa8fab60137e2f3180d`
- owner worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`
- branch: `codex/nimbus-network-architecture-audit`
- source was clean when the item started
- original dirty checkout and clean `machine-os` companion were inspected only
  and remain unchanged

## Unit Of Value

NNC5.4a owns one Container-only unit: crash-safe convergence of the complete
machine-forwarded publication and withdrawal batch under the exact parent
provider instance and generation.

NNC5.4 already owns the portable attachment, Netavark/IPAM, namespace,
segment, and shared Container/Krun crash matrix. NNC5.4a composes with that
state machine; it does not replace or duplicate it. NNC5.6 still owns
side-effect-free workload inspection and restart decisions. NNC8.3 still owns
startup orphan cleanup, final removal, and eventual capacity reuse.

No structured autoreview runs during this audit, fail-before work,
implementation, cleanup, or acceptance convergence. Exactly one full
GPT-5.6 Sol/xhigh/fast review runs only after R1-R20 and every affected gate
are green and the complete item is candidate-frozen. An accepted material
executable defect permits exactly one narrow correction review.

## Read-Only Source Audit

### Current authority map

| Concern | Current owner | Audit result |
| --- | --- | --- |
| Desired publication | persisted `ContainerSandboxManifest`, including explicit `MachineForwarded` mode, exact ordered bindings and `PortLeaseRequest`s, and persisted `OciMachinePortForwarderConfig` | Correct desired owner. It must remain immutable input, not provider truth. |
| Portable attachment lifecycle | `OciAttachmentLifecycle` plus `LocalNetworkAttachmentAuthority` | Correct shared owner. NNC5.4 deliberately fails partial machine publication to this item and preserves all portable fences. |
| Local listener/workers | `MachinePortProxyLifetimeRegistry` and `MachinePortProxyRegistration` | Correct process-lifetime owner. It may prove current local liveness, but owner death is not provider absence. |
| Provider mutation/inspection | `oci/network/forwarding.rs` | Correct gvproxy adapter owner. Native wire shapes remain status-only `POST /expose`, status-only `POST /unexpose`, and read-only `GET /all`. |
| Terminal provider evidence | `runtime/machine_port_evidence.rs` | Strict atomic `Exposed` or `Absent` batches authenticate tenant, sandbox, provider instance/generation, ordered bindings, and outcomes. This is sound terminal evidence but has no durable in-flight operation. |
| In-flight publication progress | local vectors and booleans in `MachinePortProxyRegistration` / `MachinePortProxyCleanupState` | Incorrect authority for crash recovery. `publication_may_exist`, `publication_withdrawn`, and `publication_absence_receipts` disappear with the process. |
| Host-port allocation/lifetime | `LocalPortLeaseAuthority` through `OciPortLeaseCoordinator` | Correct host-global authority. `Active`, `Withdrawing`, and `CleanupPending` already fence reuse, but cannot identify per-route gvproxy outcome. |
| Final detach ordering | Container composition callbacks around shared `detach_machine_forwarded` | Correct composition seam, but it currently consumes process-local withdrawal progress. |

### Current effect call graph

```text
create / restart publication
  configure_network
    -> shared attachment attach/recovery
    -> start exact local proxy listeners/workers
    -> expose_machine_ports
         POST /expose once per binding
         GET /all once after the whole mutation loop
    -> atomically persist complete Exposed evidence
    -> portable attachment Active

final / restart withdrawal
  shared attachment begins Deleting
    -> begin process-local proxy cleanup
         port leases begin withdrawal when final
         stop local workers
    -> for each process-local pending binding
         POST /unexpose
         GET /all
         remember success only in process memory
    -> atomically persist complete Absent evidence
    -> shared provider detach / namespace removal
    -> release or retain exact listener generation
    -> shared IPAM / segment / portable terminal transition
```

The gvproxy adapter already refuses to mint a terminal receipt from generic
HTTP status. Exact `GET /all` observation is the evidence boundary. The defect
is ordering and durability around that boundary:

1. expose sends every mutation before the first exact inspection;
2. neither expose nor unexpose persists an exact batch attempt before the
   first provider effect;
3. same-process withdrawal remembers successful siblings only in memory;
4. a fresh process reconstructs `publication_may_exist=true` and every
   withdrawal slot pending, so it reissues mutations already proven by the
   former process;
5. Active machine attachment recovery deliberately skips the backend
   publication callback, while partial phases are fenced to NNC5.4a;
6. terminal evidence does not bind the attachment resource version or exact
   listener lease generations, so it is insufficient as the new operation
   authority.

### Current versus target ownership

```text
current
  desired manifest
  + terminal Exposed/Absent file
  + process-local publication/withdrawal flags
  + fresh provider observation
  -> same-process convergence only

target
  desired manifest
  + one durable exact machine-publication state machine
  + process-local listener/worker lifetimes
  + one fresh typed provider batch observation
  -> inspect-before-effect convergence after error, response loss, or death
```

The target deletes the process-local provider-outcome booleans. The process
registry retains only local worker/listener/lifetime facts.

## Binding Decisions

### 1. One durable batch authority

Replace the terminal-only evidence record with one strict,
cross-process-locked machine-publication record. It is the sole durable owner
of both in-flight provider mutation progress and terminal observation:

```text
Absent -> Exposing -> Exposed -> Withdrawing -> Absent
```

There is no compatibility reader or dual-write path. Nimbus is pre-launch;
the old record version and old process-local provider-outcome fields are
deleted in the same item.

Every record authenticates:

- tenant and sandbox;
- stable `NetworkAttachmentId`;
- exact attachment `NetworkResourceVersion`, including plan ID, generation,
  digest, and lease epoch;
- provider handle and provider generation;
- monotonically increasing machine-batch generation;
- canonical ordered `SandboxPortBinding` and `PortLeaseRequest` pairs; and
- one exact slot state per binding.

An IP address, route, socket, filename, or receipt is never workload identity.

The batch generation is this record's own monotonic transition sequence, not
an independently allocated resource generation. Its exact serialized value is
covered by the strict SHA-256 envelope, while legitimate advancement occurs
only inside `prepare` under the one cross-process lock. A second durable
counter would duplicate operation authority and is deliberately not invented.
Externally rooted fields are additionally matched against manifest,
attachment, listener-lease, and provider authority even when a test
recomputes a valid envelope.

### 2. Explicit per-slot ambiguity

Each slot has one exhaustive state:

```text
Pending
EffectMayExist
ObservedExposed(authenticated receipt)
ObservedAbsent(authenticated withdrawn/already-absent receipt)
```

`EffectMayExist` is durable before the native mutation. A response, timeout,
EOF, or connection error never advances it. Only a fresh exact provider
observation advances to `ObservedExposed` or `ObservedAbsent`.

The complete record changes to terminal `Exposed` or `Absent` atomically only
when every canonical slot has the matching authenticated observation. Terminal
readers never receive a partial batch.

### 3. Small provider capability seam

The sandbox adapter exposes a private, substitutable capability with three
operations:

- inspect the complete current native route list once under one deadline;
- request exposure of one exact binding; and
- request withdrawal of one exact binding.

Mutation return status is diagnostic only. The inspection result is a
non-serializable typed batch that classifies every desired slot as exact
exposed, exact absent, or conflicting. Malformed, oversized, duplicate,
partial, unavailable, timed-out, or crossed evidence remains unknown.

The real implementation stays in `nimbus-sandbox` and speaks the existing
gvproxy wire contract. No HTTP, socket, gvproxy, provider, or effect type
enters `nimbus-network`.

### 4. Inspect before every retry decision

For either action, the coordinator:

1. authenticates desired, attachment, listener, provider, and durable batch
   identity;
2. persists the action/batch generation before the first effect;
3. performs one exact read-only batch inspection;
4. advances already-satisfied slots without mutation;
5. persists `EffectMayExist` before mutating one unsatisfied slot;
6. performs the one native mutation;
7. re-inspects before advancing that slot; and
8. publishes the terminal complete batch atomically.

After process death, `EffectMayExist + exact exposed` or
`EffectMayExist + exact absent` converges without replay. If the expected
effect is absent, the prior mutation did not persist and one retry is safe. If
inspection is unknown or conflicting, no mutation or release occurs.

### 5. Lifecycle composition

Provision:

```text
portable attempt durable
-> local listener leases/workers current
-> machine Exposing durable
-> expose/observe every slot
-> complete Exposed durable
-> portable publication/Active
```

Teardown:

```text
portable Deleting durable
-> machine Withdrawing durable
-> stop local workers
-> unexpose/observe every possibly visible slot
-> complete Absent durable
-> shared provider/netns detach
-> listener settle/release
-> IPAM/segment settle/release
-> portable terminal
```

An empty binding set still writes an exact header-only terminal record and
performs no provider I/O.

The shared Active/partial attachment seam may call the Container publication
coordinator, but the shared crate does not learn gvproxy states. A complete
same-generation `Exposed` record plus exact fresh observation is effect-free;
partial records resume through the Container callback. Host-managed Container
and Krun behavior is unchanged.

## Named Crash Cuts

### Exposure

| Label | Durable/effect boundary | Fresh-process obligation |
| --- | --- | --- |
| `machine.expose.local_provider_ready` | local listeners/workers and lease lifetimes current; no batch effect | create or reopen the exact batch; no external effect without its record |
| `machine.expose.batch_prepared` | exact `Exposing` generation durable | inspect before mutation |
| `machine.expose.slot_effect_prepared` | selected slot is `EffectMayExist` | inspect that exact provider generation before retry |
| `machine.expose.slot_effect_returned` | mutation may have committed; return path is not evidence | inspect; never infer from response |
| `machine.expose.slot_observed` | exact slot receipt durable | never replay the satisfied slot |
| `machine.expose.batch_exposed` | complete canonical `Exposed` batch durable | replay is inspection-only and byte-stable |
| `machine.expose.attachment_active` | portable Active durable | rebuild only local process lifetime as needed; provider mutation remains inspect-before-effect |

### Withdrawal

| Label | Durable/effect boundary | Fresh-process obligation |
| --- | --- | --- |
| `machine.withdraw.batch_prepared` | exact `Withdrawing` generation durable before local/provider stop | retain every port/attachment/segment fence |
| `machine.withdraw.local_provider_stopped` | local workers absent; external publication may remain | process death is never external absence |
| `machine.withdraw.slot_effect_prepared` | selected slot is `EffectMayExist` | inspect before retry |
| `machine.withdraw.slot_effect_returned` | unexpose may have committed | inspect; never infer from response |
| `machine.withdraw.slot_observed_absent` | exact slot absence receipt durable | never replay the satisfied slot |
| `machine.withdraw.batch_absent` | complete canonical `Absent` batch durable | provider mutation replay is zero |
| `machine.withdraw.listener_settled` | exact restart retention or final release committed | only terminal absence may reach this cut |

Every cut is exercised for the production Container machine-forwarded route.
The harness uses real child-process death, a provider state server that
survives children, reopened durable roots, bounded waits, and complete
status/stdout/stderr diagnostics.

## Failure Decision Table

| Durable batch | Fresh provider observation | Decision |
| --- | --- | --- |
| none for exact current generation | exact absent | prepare `Exposing` for provision, or prove no provider effect for pre-publication compensation |
| none | any exact exposed slot | fence as unjournaled provider effect; do not adopt or mutate |
| `Exposing` | all exact exposed | persist complete `Exposed`; zero expose mutations |
| `Exposing` | mixed exact exposed/absent | preserve satisfied slots; expose only exact-absent unsatisfied slots |
| `Exposing` | conflict or unknown | retain batch and every authority; zero mutation |
| `Exposed` | all exact exposed | idempotent effect-free success |
| `Exposed` | missing/conflict/unknown | fail closed; ordinary drift repair is not invented here |
| `Exposing` or `Exposed` entering teardown | any exact exposed/absent mixture | transition to `Withdrawing`; withdraw every exact-visible or effect-ambiguous slot |
| `Withdrawing` | all exact absent | persist complete `Absent`; zero unexpose replay |
| `Withdrawing` | mixed exact exposed/absent | preserve absent slots; unexpose only exact-visible unsatisfied slots |
| `Withdrawing` | conflict or unknown | retain CleanupPending and all reuse fences; zero mutation/release |
| `Absent` | all exact absent | idempotent effect-free success |
| `Absent` | any exact exposed/conflict/unknown | fail closed; never release or reinterpret |
| any state with substituted identity/version/provider/listener batch | any | byte-preserving rejection before provider I/O |

## Frozen Acceptance Criteria

| ID | Criterion |
| --- | --- |
| R1 | The read-only call graph, current authority map, exact failure windows, target state machine, named cuts, and NNC5.4/NNC5.6/NNC8.3 boundaries are recorded before executable changes. |
| R2 | Expected-red provider and lifecycle tests demonstrate duplicate expose after response loss and duplicate withdrawal after owner-state loss, with exact request censuses and unchanged durable authority snapshots. |
| R3 | One strict durable machine-publication record owns in-flight and terminal batch state. The old terminal-only record and process-local provider-outcome booleans are deleted; no dual write, compatibility reader, or second operation authority remains. |
| R4 | Every record authenticates exact tenant, sandbox, attachment ID/full resource version, provider handle/generation, batch generation, ordered bindings, and ordered lease requests. Every substitution fails byte-preserving before provider I/O. |
| R5 | The action and selected slot `EffectMayExist` state are durable before their corresponding provider effects. Fault injection at each write/effect boundary proves write-before-effect ordering. |
| R6 | One private provider capability has real and deterministic substitutes. The real adapter retains native gvproxy shapes and bounded I/O; mutation responses remain diagnostic and only typed current observation advances durable state. |
| R7 | One complete native batch inspection classifies every desired slot as exact exposed, exact absent, or conflicting. Omission from a complete list is exact absence; a wrong or duplicate route for the same listener is conflicting. Unsupported status, malformed or truncated bodies, oversized responses, timeout, EOF, and refusal are unknown and perform zero mutation fallback. |
| R8 | Fail-Nth exposure across a multi-binding batch mutates every required slot at most once after exact observation, publishes only a complete canonical `Exposed` batch, and preserves every satisfied sibling across retry. |
| R9 | Exposure response loss at every binding plus process death at every named exposure cut reopens the same batch generation, inspects first, performs zero duplicate mutation for an effect that exists, and converges to one exact Exposed result or a named fenced unknown. |
| R10 | Fail-Nth withdrawal attempts every possibly visible binding, persists progress after each exact absence observation, retries only still-visible slots, and atomically publishes one complete canonical `Absent` batch. |
| R11 | Withdrawal response loss at every binding plus process death at every named withdrawal cut reopens the same batch generation, performs zero duplicate mutation for an already-absent route, and converges or remains precisely CleanupPending. |
| R12 | Fresh-process recovery uses only durable manifest/attachment/port/batch roots plus current provider inspection. No crash marker, response, process-local registry entry, or prior child baseline is treated as provider truth. |
| R13 | Two concurrent processes contend through one bounded cross-process batch lock. Exactly one generation/effect sequence wins; the other reopens and observes or receives a typed timeout. Lock/stage crashes reconcile without corrupting canonical bytes. |
| R14 | Local workers/listener lifetimes remain process-owned. Recovery reclaims only exact dead-owner port lifetimes, rebuilds the exact normalized local route set, and never treats worker death as external provider absence. |
| R15 | Portable attachment phases remain shared. Partial machine publication resumes through the Container capability without duplicating the portable state machine; complete Active replay is provider-effect-free; host-managed Container/Krun tests remain unchanged and green. |
| R16 | Final and restart teardown persist `Withdrawing` before local/provider effects. No port lease, IPAM generation, segment hold, attachment generation, or endpoint becomes reusable until complete exact `Absent` evidence exists. Restart retains the exact generations; final releases exactly once. |
| R17 | Primary error plus compensation/recovery diagnostics survive. Unknown or conflicting provider evidence yields a named fenced result, never generic success, inferred absence, blind retry, or early release. |
| R18 | Concept ownership remains readable: the durable state machine, provider adapter, process-lifetime registry, and composition roots are separate; no changed file crosses repository thresholds without an explicit owner justification; no god `NetworkProvider` or speculative abstraction appears. |
| R19 | `nimbus-network -> nimbus-core` remains the sole initial workspace edge. No gvproxy, HTTP, socket, Netavark, nft, proxy, tenant, service, server, system, machine transport, or cleanup effect enters the portable crate. |
| R20 | Focused happy/edge/error/substitution/fault/crash/concurrency tests, full affected suites, all-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, dependency/effect scans, static verifier and adversarial mutations, docs gates, and exactly one candidate-frozen Sol/xhigh/fast review pass with exact evidence before the one exact item commit. |

## Expected-Red Packet

Before production changes:

1. add a two-binding expose response-loss case where the first native expose
   effect commits but exact post-mutation observation fails; the current retry
   must issue a second expose for that already-visible route;
2. add a two-binding fresh-owner withdrawal case where one route is confirmed
   absent before the process-local progress is lost; the current fresh owner
   must issue a second unexpose for that already-absent route;
3. assert exact provider request sequences, port/attachment/segment byte
   stability, and the absence of a durable in-flight batch;
4. record exact command, exit status, pass/fail/ignored counts, and the first
   failing invariant;
5. preserve every already-green provider/readiness/cleanup test; and
6. add the new static NNCV021 contract before candidate closeout, not as a
   substitute for behavioral proof.

The two regression families must fail at the duplicate-effect assertions, not
at fixture construction or an unrelated transport error.

### Captured expected-red evidence

Command:

```text
timeout 300 cargo test -p nimbus-sandbox --lib nnc5_4a_ -- --nocapture
```

The final pre-production run exits `101`: `0` passed, `4` failed, `0` ignored,
and `896` filtered out. All four failures are the frozen duplicate-effect
invariant:

- direct expose response loss performs `POST, GET(unknown), POST, GET(exposed)`
  and counts two expose mutations instead of one;
- direct withdrawal response loss performs
  `POST, GET(unknown), POST, GET(absent)` and counts two unexpose mutations
  instead of one;
- the two-binding lifecycle expose retry preserves the exact port-authority
  records but issues two native exposes for each already-visible route; and
- the two-binding fresh-owner withdrawal proves the first owner observed one
  route absent, drops all process-local cleanup state, reopens every port in
  `CleanupPending`, preserves the combined port/attachment/segment authority
  bytes across provider I/O, and then issues a second unexpose for the
  already-absent route. The failed route is correctly attempted once by each
  owner.

The only diagnostics outside those four assertions are the pre-existing
vendored Brotli warnings. Production code was unchanged while this evidence
was captured.

## Behavioral Proof Matrix

| Family | Required rows |
| --- | --- |
| Pure transition table | every valid phase/slot transition; every illegal regression, skipped slot, duplicate binding, generation mismatch, and terminal resurrection |
| Identity substitutions | tenant, sandbox, attachment ID, plan ID, resource generation, digest, lease epoch, provider handle, provider generation, batch generation, binding order/member, lease ID/generation/epoch |
| Provider observation | all exposed, all absent, mixed, duplicate/same-listener conflict, wrong remote, wrong protocol, extra unrelated route, truncated response, malformed, oversized, status, timeout, EOF, refusal |
| Exposure faults | fail before/after each slot mutation; response loss for each slot; post-effect inspection loss; progress-write acknowledgement loss; terminal publication acknowledgement loss |
| Withdrawal faults | fail before/after each slot mutation; response loss for each slot; post-effect inspection loss; progress-write acknowledgement loss; terminal publication acknowledgement loss |
| Real process cuts | every named exposure and withdrawal label, fresh recovery, and a second fresh terminal replay |
| Concurrency | two process contenders, lock timeout, stale stage cleanup, non-regular lock/stage/canonical artifact rejection |
| Integration | initial launch, restart retention/re-exposure, final detach, partial attach cleanup, empty batch, local worker death, provider unknown/conflict, exact Active replay |
| Regression | NNC5.3a readiness/current-observation, NNC5.4 attachment crash matrix, NNC3.8 listener lifetime, provider cleanup, host-managed Container/Krun |

## Failure And Rollback Semantics

- The operation record is the rollback/reconciliation plan; there is no
  compensating guess.
- An exposure failure retains the exact attachment, listener leases, local
  publication fence, batch record, and provider identity. Cleanup transitions
  the same record to `Withdrawing` and withdraws every possibly visible slot.
- A withdrawal failure retains CleanupPending, exact port leases, IPAM,
  segment association/quarantine, attachment generation, and all durable slot
  witnesses.
- A provider conflict or unknown observation performs no mutation and no
  release. Later exact inspection resumes the same generation.
- Atomic file publication follows staged write, file sync, rename, and
  directory sync. Ambiguous sync acknowledgement is resolved by reopening the
  canonical record under the same lock.
- Terminal replay is byte-stable and provider-effect-free.

## Sovereignty And Provider Semantics

This is a host-managed machine capability under a parent-issued provider
handle and generation. The durable record proves which provider incarnation
was authorized; the current native route list proves only that incarnation's
observed routes. A deployment that cannot supply exact current observation
reports the capability unavailable/unknown and remains fenced.

Nimbus does not require DNS, xDS, Consul, an overlay, or a cloud control-plane
API. The gvproxy adapter remains optional provider-local code in
`nimbus-sandbox`. The portable network crate retains provider-neutral desired
and lifecycle vocabulary only.

## Non-Goals

NNC5.4a does not:

- move gvproxy, HTTP, sockets, proxy workers, Netavark, nftables, IPAM, PEP,
  policy, service naming, or cluster transport into `nimbus-network`;
- change machine membership, routing, overlay, or future `ClusterTransport`;
- change workload restart policy or make `inspect` start effects;
- implement startup orphan cleanup, final artifact removal, or capacity reuse;
- absorb egress PDP/PEP or certificate authority;
- repair arbitrary post-terminal provider drift;
- introduce a general `NetworkProvider`;
- add compatibility parsing, dual writes, feature flags, or legacy shims; or
- push, open a PR, or alter the original dirty checkout.

## Modularity Ledger

The canonical plan is a deliberate 1,500–1,999 documentation-band exception
at 1,961 lines: it is the one owner for dependency order, all item statuses,
and compaction recovery. Splitting current status from implementation order
would duplicate routing authority. NNC5.4a may replace existing recovery rows
but must not take that owner to 2,000 lines; detailed evidence remains here.

The 1,551-line aggregate verifier is a deliberate 1,500–1,999 executable-band
exception. It is the one fail-closed orchestration and exact mutation-summary
owner for all named `NNCV000`–`NNCV021` conditions; splitting its shared
temporary-worktree mutation protocol would create a second aggregation
authority. Condition-owned logic remains outside that composition root:
NNC5.4a's source contract is in
`verify-nimbus-network-machine-forwarded-batch-convergence.mjs`, and its
mutations are in
`nimbus-network-control-plane/machine-forwarded-batch-convergence-contract.sh`.
No new NNC5.4a lifecycle or source-analysis logic was added inline.

## Frozen Path Ownership

Production and unit-test owners:

- `crates/nimbus-sandbox/src/backends/container/mod.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_evidence.rs`
  (delete)
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_evidence/tests.rs`
  (delete)
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication.rs`
  (add)
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication/store.rs`
  (amended before the modularity edit: concept-owned atomic publication,
  strict reopen, cross-process lock, and staged-artifact reconciliation keep
  the lifecycle state machine below the repository threshold)
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication/tests.rs`
  (add)
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication/tests/fault_matrix.rs`
  (amended before the fault-matrix edit: concept-owned fail-Nth,
  response-loss, ambiguous-inspection, and diagnostic rows keep the core
  state-machine test owner below the repository modularity threshold)
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication/tests/store_faults.rs`
  (amended before the store-fault edit: concept-owned stage/rename
  acknowledgement and typed lock-contention proofs keep durable storage
  mechanics out of lifecycle tests)
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_ports.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/execution_cleanup.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/restart.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/runner/recovery.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/execute_inspection.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/attachment_authority.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/plan_only_inspection.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/egress_reload_recovery.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/restart_policy.rs`
  (add)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/machine_proxy_activation.rs`
  (add)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/machine_proxy_recovery.rs`
  (add)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/machine_proxy_concurrency.rs`
  (add)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/network_configuration.rs`
  (add; the preceding lifecycle-test paths were amended before R18
  decomposition so the parent remains only a concept-routing switchboard)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/assertions.rs`
  (add)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/machine_publication.rs`
  (add)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/execution_context.rs`
  (add)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/network_finality.rs`
  (add; the provider-cleanup child paths were amended before R18
  decomposition along effect ownership rather than raw line count)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/forwarder_observer.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/machine_forwarded_readiness.rs`
  (readiness regressions must stop snapshotting the deleted process-local
  publication-outcome flag)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/machine_port_batch_recovery.rs`
  (add)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/preselected_identity.rs`
  (amended during broad-suite convergence: the plan-only zero-binding
  regression proves it cannot fabricate an attachment-bound provider record;
  the real empty-batch proof lives under the attached state machine)
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/machine_port_batch_recovery/fresh_process.rs`
  (add if the harness needs a concept-owned child)
- `crates/nimbus-sandbox/src/backends/oci/network.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/forwarding.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/forwarding/receipt.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/forwarding/tests.rs`
  (add to keep the provider composition root below the modularity threshold)
- `crates/nimbus-sandbox/src/backends/oci/network/process.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/process/machine_proxy_lifetime.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/proxy.rs`
  (amended before R14 recovery convergence: the exact local listener
  preparation owner must reclaim only dead process lifetimes before rebuilding
  the canonical route set; external publication remains governed by the
  durable batch and fresh provider inspection)
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/plan.rs`
  (the publication authority must reconstruct the same canonical attachment
  plan rather than duplicate its digest compiler)
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/active_reconciliation.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/durable_recovery.rs`
- `crates/nimbus-sandbox/src/backends/oci/port_lifecycle.rs`
  (fresh-owner withdrawal must authenticate the exact `CleanupPending`
  listener generation without treating that reuse fence as provider absence)
- `crates/nimbus-sandbox/src/backends/oci/port_lifecycle/machine.rs`
  (add)
- `crates/nimbus-sandbox/src/backends/oci/port_lifecycle/batch_state.rs`
  (amended before R18 decomposition: machine-listener lifecycle stays one
  capability-owned child while shared batch authentication remains in the
  existing batch-state owner)

Static/verifier/proof owners:

- `docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json`
  (amended before R20 census convergence: NNC5.4a deletes the legacy direct
  forwarder effect, moves concept-owned modules, and adds one diagnostic-only
  macro occurrence; the source-derived line/symbol census must remain exact)
- `scripts/verify-nimbus-network-attachment-readiness.mjs`
  (amended before verifier convergence: NNCV019 must follow the deliberate
  single-owner deletion and complete `Exposed` publication record without
  restoring a compatibility file or process-local outcome boolean)
- `scripts/verify-nimbus-network-machine-forwarded-batch-convergence.mjs` (add)
- `scripts/nimbus-network-control-plane/machine-forwarded-batch-convergence-contract.sh`
  (add)
- `scripts/verify-nimbus-network-control-plane.sh`
- this proof
- the canonical plan and routing index

Any executable path outside this set requires a recorded path amendment and
ownership reason before it is edited. Forbidden seams include
`crates/nimbus-network/**`, workload restart-policy owners, startup orphan
cleanup/reuse owners, service/tenant/proxy-policy/server/system crates, and
machine/cluster transport.

## Candidate Verification Evidence

| Gate | Exact result |
| --- | --- |
| Core NNC5.4a filter | `17` passed, `0` failed, one child-only ignore. |
| Real-process cuts | `5` parent tests passed, `0` failed, one child-only ignore; every named expose/withdraw cut reopens in a new process, terminal replay is exact, the provider survives the killed child, and two contenders share one generation/effect sequence. |
| Focused owners | Durable publication/store `19/19`; forwarding adapter `20/20`; provider cleanup `30/30`; port lifecycle `51` with two intentional ignores; proxy provider `19/19`; machine recovery `8/8`; shared attachment lifecycle `59` with five child-only ignores. |
| Full affected suite | Final `cargo test -p nimbus-sandbox --lib`: `898` passed, `0` failed, `27` intentionally ignored, `0` filtered. |
| Build/docs quality | All-target/all-feature check, strict no-deps Clippy, warning-denied rustdoc, format, staged/unstaged diff checks, bind census, Node/Bash syntax, and ShellCheck pass. Diagnostics are limited to pre-existing vendored Brotli warnings and expected caught-panic fixtures. |
| Dependency/effect boundary | Metadata reports `nimbus-core` as the sole `nimbus-network` workspace dependency; the portable production transport/provider scan is clean. |
| Static proof | Live verifier `22/22`. The first full mutation closeout exposed two NNCV021 search-scope gaps (`109` pass, `2` fail): a neighboring struct masked a removed durable field and later duplicate labels masked a reordered cut array. Exact canonical-struct and exact canonical-array checks make both mutations fail exclusively as NNCV021; the unchanged full rerun passes `111/111`. |
| Documentation | `scripts/check-docs.sh`: `108` pages link-clean; docs-site verifier: `17/17`. |
| Review cadence | One full GPT-5.6 Sol/xhigh/fast item review and, after accepted executable corrections, one narrow correction review ran. The narrow review's sole accepted IPv4-mapped defect is corrected and proven; no third review ran or is warranted. |
| Final executable/script digest | Exact `git diff --binary HEAD -- crates scripts` SHA-256: `a5eadd2b4795589ce7cf1244a74d54a3ca5d82edf851cd0cae92944304f940e5`. |

## Structured Review Disposition

The sole full review froze staged tree
`17c715de56f0d013ba588c090e9246eeb6c40f12` with patch SHA-256
`e3a6d79b38306f76e8b582dfffa7cf248d760251114cae9860cf27fb2c6fbcd9`.
One GPT-5.6 Sol/xhigh/fast invocation split the large bundle into two coverage
passes:

- `019fb8e3-44ce-7ba3-8157-8ec36ca5d691`;
- `019fb8e7-5990-7ee1-9073-bcc24cd02b9b`.

It returned six findings at overall confidence `0.97`:

1. **Accepted narrowly — unconditional withdrawal preparation.** The broad
   PlanOnly/Netavark allegation was source-rejected, but an exact
   machine-forwarded nonempty, launch-owned `Reserved`/`Failed` `NeverBound`
   batch was incorrectly blocked. The fail-before test exited `101` with
   `0` passed, `1` failed, `0` ignored, and `921` filtered. Cleanup now
   classifies first, prepares withdrawal only for `ProviderOwned` or
   `RestartRetained`, skips authenticated `NeverBound`/`TerminalNoEffect`,
   rejects impossible Netavark claims before effects, and proves exact release,
   finality, and no fabricated publication file.
2. **Rejected — legacy bare-evidence migration.** Nimbus is pre-launch, the
   repository explicitly requires breaking replacement and forbids migration
   shims, and R4 requires a strict envelope with no legacy reader. Adding the
   proposed compatibility path would violate the governing contract.
3. **Rejected — automatic terminal-drift repair.** NNC5.4a owns convergence of
   one desired batch, not synthesis of a new desired generation after terminal
   provider drift. Exact opposite terminal observation remains a deliberate
   fail-closed condition; inventing a generation here would duplicate desired
   state authority.
4. **Accepted — overlapping wildcard observation.** The fail-before adapter
   test exited `101` with `0` passed, `1` failed, `0` ignored, and `920`
   filtered. TCP locals are now parsed, native and socket wildcards overlap
   conservatively in either direction, known UDP/UNIX/NPIPE routes remain
   disjoint, and malformed or unknown protocols fail provider-unknown.
5. **Accepted — NNCV021 scanned the wrong retired-field owner.** The verifier
   now scans the publication and process-lifetime owners; a direct restored
   process-local field mutation fails exclusively as NNCV021, the aggregate
   mutation run reports only that failure, and the complete self-test passes
   `112/112`.
6. **Accepted — bind census anchored a read-only inspection.** The AST scanner
   recognizes the exact `POST /expose` and `POST /unexpose` production effects,
   excludes `GET /all`, and passes `12/12`; the source-derived inventory records
   both mutation occurrences and remains exact at `63` authorities and `34`
   classified non-authority risks.

Those accepted executable corrections froze staged tree
`38226a13869fa999c8fba513cd2a8f908923361e` with patch SHA-256
`678a68399e76b609fb77af032b97c45089b1634306d1af6faa0160926606b26a`.
The sole narrow GPT-5.6 Sol/xhigh/fast correction review used one invocation
with three coverage passes:

- `019fb921-f561-77a3-ad05-45d70c558ccb`;
- `019fb922-af0d-7083-a133-9bd5f05dd68e`;
- `019fb925-4027-7622-a67e-f0d0cde3d031`.

It returned one P2 finding at aggregate confidence `0.96`: an IPv4-mapped IPv6
gvproxy local could be misclassified as absent. The exact fail-before test
exited `101` with `0` passed, `1` failed, `0` ignored, and `924` filtered.
Overlap comparison now canonicalizes only IPv4-mapped addresses while retaining
the exact serialized identity for conflict evidence. Mapped exact and mapped
wildcard cases pass in the full `20/20` forwarding suite and the final
`898/0/27` Sandbox suite. This was the permitted narrow correction review; no
third review ran.

## Recovery Ledger

| Checkpoint | Status | Evidence / next action |
| --- | --- | --- |
| Source durability | `done` | NNC5.4 commit `239c9a5523d38350c0a74348f1501f0cb014ff2a`, tree `4b7e54e5d1db8cec46de8fa8fab60137e2f3180d`; owner worktree clean at start. |
| Read-only call graph | `done` | Desired manifest, portable attachment, provider adapter, terminal evidence, process lifetime registry, port authority, launch/restart/final cleanup consumers, and existing tests mapped. |
| Authority audit | `done` | Terminal evidence and provider inspection are sound; in-flight publication/withdrawal progress is duplicated in process-local booleans and is not crash-recoverable. |
| Binding decisions | `done` | One strict durable batch state machine, explicit slot ambiguity, private provider capabilities, inspect-before-effect retry, and exact lifecycle ordering frozen. |
| Acceptance criteria | `done` | R1-R20, named cuts, failure table, proof matrix, non-goals, and path ownership frozen before executable edits. |
| Fail-before | `done` | Final pre-production command exits `101`: 0 passed, 4 failed, 0 ignored, 896 filtered. Direct expose/withdraw response-loss and lifecycle expose/fresh-owner-withdraw cases fail only on duplicate mutation counts; the lifecycle cases preserve exact durable authority. |
| Implementation | `done` | One strict v2 durable batch record and small real/substitutable provider capability replace terminal-only evidence and process-local outcomes. Exact observation, fail-Nth/response-loss, real process death, contention, dead-owner rebuild, partial recovery, pre-effect withdrawal, diagnostics, and no-release fencing satisfy R1-R17; concept-owned modules plus the explicit aggregate-verifier exception satisfy R18; the core-only/effect boundary satisfies R19. |
| Focused/affected gates | `done` | Exact focused, full Sandbox `898/0/27`, check/Clippy/rustdoc, live verifier `22/22`, mutations `111/111`, verifier self-test `112/112`, AST `12/12`, census, dependency/effect, script, format/diff, docs `108`, and site `17/17` results are recorded above. |
| Candidate-frozen review | `done` | The sole full Sol/xhigh/fast review's four accepted and two rejected findings are dispositioned above. Its accepted executable corrections warranted exactly one narrow review; that review's sole mapped-address defect is corrected and proven. No partial or third review ran. |
| Ledger/commit | `done` | This proof, canonical ledger/routing transition, and exact owned candidate form one NNC5.4a item checkpoint; no push/PR. |
