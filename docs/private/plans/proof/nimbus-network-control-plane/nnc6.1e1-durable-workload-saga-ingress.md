# NNC6.1e1 Durable Workload-Saga Ingress

Status: `complete; I1-I20 green`

Starting checkpoint: `b0a97e4404cd8afc3b11fdbf2053fa12e0b1c3d7`

This record freezes the NNC6.1e1 implementation boundary and source allowlist.
It also freezes the failure contract and acceptance matrix. The item adds one durable
submission capability to the existing compute-owned workload-saga coordinator.
It does not dispatch a workload or network command and does not cut over an
effectful caller.

## Audit Verdict

The existing portable record, state machine, object-safe store port, Engine
adapter, compute coordinator, and pure recovery decision are sufficient for a
durable ingress. One sequencing defect in the earlier plan wording required a
prospective boundary correction before product edits:

1. The current `ServiceManager` lifecycle methods perform coarse start, stop,
   and restart effects. They do not accept an idempotent saga command for one
   exact phase and revision.
2. A caller could persist `IntentCommitted` and immediately call one of those
   methods. The durable saga could then remain at `IntentCommitted` while a
   workload already runs or has stopped.
3. A fresh process would then derive `ReserveNetwork` from durable truth and
   could repeat work against an out-of-phase live provider effect.
4. Exact replay and ambiguous-result recovery may return the same pure decision
   more than once. The ingress therefore cannot claim exactly-once dispatch.

NNC6.1e1 owns durable submission and confirmed decision derivation only.
NNC6.3, NNC6.4a, NNC6.5, and NNC6.6 retain their effectful caller cutovers and
choreography. NNC6.1e2 retains fresh-process convergence. This correction does
not add a roadmap item, remove an acceptance obligation, or weaken the NNC6
band gate.

The completed NNC6.1e proof preserves the split wording reviewed with that
item. This proof prospectively supersedes only its NNC6.1e1 dependency and
caller-cutover assignment before NNC6.1e1 product implementation.

## Current Authority And Effect Census

The audit ran from the starting checkpoint and changed no files. Counts are
production Rust syntax sites unless a row says otherwise.

| Authority or effect | Current count | Source-derived finding |
| --- | ---: | --- |
| Product `WorkloadSagaCoordinator` construction | 1 | `nimbus-compute/src/state.rs` retains the server-injected store in the sole coordinator. |
| Product exact intent compilation | 0 | `WorkloadNetworkPlanCompiler` use is test-only. Caller composition belongs to later choreography. |
| Product saga-intent construction | 0 | No caller can yet submit a complete admitted intent. |
| `ServiceManager: RuntimeServiceRegistry` implementation | 1 | The mixed trait grants read resolution, lazy activation, and tenant teardown to one object. |
| Product asynchronous lazy-activation call | 1 | The runtime bridge invokes the effectful registry directly. |
| Product synchronous service lookup | 1 | It is read-only and remains services-owned. |
| ServiceManager backend start sites | 2 | Native service and standalone sandbox start effects remain command sinks. |
| ServiceManager backend stop sites | 7 | Service, sandbox, definition deletion, and tenant teardown remain command sinks. |
| ServiceManager backend inspect sites | 2 | Inspection stays with the provider owner and must remain observational. |
| ServiceManager egress reload sites | 1 | Egress forwarding remains outside the workload-saga ingress. |
| ServiceManager tenant-artifact removal sites | 1 | Tenant cleanup remains NNC6.1e2/NNC8.3 work. |
| Direct standalone Compose provider lifecycle | 4 helpers | Compose has no Engine saga composition today and requires an explicit later cutover. |
| Compute tenant-teardown registry calls | 1 | Freeze at one until NNC6.5/NNC6.1e2 retire it. Growth is forbidden. |

The complete caller families are:

- runtime lazy activation for Convex query, paginated query, mutation, action,
  subscription, and Cloud Functions HTTP/callable execution.
- native service start, stop, restart, and force definition deletion.
- native standalone sandbox create and stop.
- standalone Compose up and down.
- host-forwarded Machine commands and guest Machine API provider commands.
- tenant retirement.
- read-only service resolution and session snapshots.

Machine API and node reconciliation are effect-plane sinks. They must later
receive exact fenced commands from the host coordinator. They must not create a
guest workload-saga store. Read-only naming, definition, binding, and session
queries stay in `nimbus-services`.

## Corrected Ownership And Dependency Order

```text
nimbus-services / tenant admission / source catalog
             later NNC6.3 or NNC6.5 composes exact intent
                              |
                              v
nimbus-compute::WorkloadSagaCoordinator::submit_intent
  load -> validate/apply -> at most one CAS -> ambiguous fresh read
                              |
                              v
             confirmed durable record + pure decision
                              |
           later fenced choreography only
             /                |                 \
     NNC6.3 provision   NNC6.4a restart   NNC6.5 teardown
             \                |                 /
              existing services/sandbox/node effect owners
```

The initial workspace dependency invariant remains:

```text
nimbus-network -> nimbus-core
```

NNC6.1e1 adds no Cargo edge. It creates no services-to-compute dependency, no
second store, no second coordinator, and no effect capability in
`nimbus-network` or `nimbus-workloads`.

## Frozen Unit Of Value

NNC6.1e1 adds one public method on the existing
`WorkloadSagaCoordinator`. Names may be refined during fail-before test
authoring, but the semantic shape is fixed:

```text
submit_intent(
  key: WorkloadSagaKey,
  complete_intent: WorkloadSagaIntent,
) -> Result<ConfirmedWorkloadSagaIntent, WorkloadSagaStoreError>

ConfirmedWorkloadSagaIntent {
  record: WorkloadSagaRecord,
  decision: WorkloadSagaDecision,
  disposition: Applied | ConfirmedReplay,
}
```

The result means only that the exact record is durably confirmed and the pure
decision follows from that record. It never means that a command was dispatched
once, that a provider effect exists, or that the saga advanced beyond the
returned phase.

The existing raw `commit_loaded` method becomes private to the coordinator
module. External compute callers may submit complete intent or request a
read-only recovery decision. They may not construct a parallel raw transition
path.

## Ingress Algorithm

The algorithm is closed and bounded:

1. Load the exact `WorkloadSagaKey` once.
2. Reject a loaded record whose key differs from the requested key as corrupt.
3. If the record is missing, build `WorkloadSagaRecord::new` from the complete
   intent.
4. If the record exists, call `apply_intent` exactly once.
5. Return an exact replay without a CAS when `apply_intent` is `Unchanged`.
6. Otherwise perform at most one compare-and-swap through the existing
   coordinator primitive.
7. Only an ambiguous CAS may trigger one fresh read through the existing
   ambiguity resolver.
8. Reject a crossed-key record from the ambiguity read as corrupt.
9. Derive `WorkloadSagaDecision::for_record` only from the record confirmed by
   steps 5-8.
10. Return the exact confirmed record, decision, and disposition.

No branch loops, sleeps, retries a conflict, compiles a network plan, selects a
provider, allocates generation, reads a system projection, or calls an effect.

## Outcome Matrix

| Starting truth and submission | Store activity | Required result | Command authority |
| --- | --- | --- | --- |
| Missing record, valid intent | One load, one CAS expected `Missing` | Confirmed initial record and its exact decision | None |
| Exact current-generation replay | One load, zero CAS | `ConfirmedReplay` with the same record and decision | None |
| Higher generation while active | One load, one CAS | Confirm the successor/withdrawal transition and return the active-generation withdrawal decision | None |
| Higher generation after terminal `Recorded` | One load, one CAS | Confirm exact successor promotion or initial phase selected by the existing state machine | None |
| Missing stopped intent | One load, one CAS | Confirm terminal `Recorded` and `Quiescent` | None |
| Stale generation | One load, zero CAS | Typed invalid transition | None |
| Equal-generation divergent content | One load, zero CAS | Typed equal-generation conflict | None |
| Load unavailable or corrupt | One load, zero CAS | Preserve the typed store error | None |
| Load returns a valid record for another key | One load, zero CAS | Typed corrupt error | None |
| CAS conflict | One load, one CAS, zero retry | Preserve the typed conflict | None |
| Ambiguous CAS, fresh read equals proposed record | One load, one CAS, one fresh read | Confirm exact record and derive its decision | None |
| Ambiguous CAS, fresh read equals old record | One load, one CAS, one fresh read | Typed ambiguity | None |
| Ambiguous CAS, fresh read is missing | One load, one CAS, one fresh read | Typed ambiguity | None |
| Ambiguous CAS, fresh read is competing record | One load, one CAS, one fresh read | Typed conflict | None |
| Ambiguous CAS, fresh read fails | One load, one CAS, one fresh read | Preserve the fresh-read error | None |

An exact replay can return the same decision many times. Later command ports
must use stable identity, desired generation, saga revision/transition fencing,
and inspect-before-retry semantics. They cannot depend on process-local
single-dispatch assumptions.

## Identity, Integrity, And Fencing

Every confirmed result must preserve and expose the exact:

- tenant-qualified logical workload key and derived saga ID.
- active and successor generation.
- admission decision, workload UID, and assigned node.
- desired digest, desired state, activation intent, and publication intent.
- complete compiled `NetworkPlan` carrier and retained canonical resource
  bytes.
- saga revision, phase, transition identity, and pure decision target/action.

The ingress never derives workload identity from an IP address, port, provider
handle, returned sandbox ID, manifest filename, or system record.

The Engine adapter already protects requested-key correlation in three layers:

1. the physical lookup uses the saga ID derived from the requested key.
2. the codec requires the document ID to equal the decoded saga ID.
3. record validation requires the saga ID to equal the decoded key's derived
   saga ID.

Another decoded-key check inside the Engine adapter would guard only a
successful SHA-256 collision across two valid keys. The item does not add that
redundant adapter check. The generic store port can still have an incorrect
third-party or test implementation, so the coordinator validates each loaded
record against the requested key. A crossed store result returns `Corrupt`
before CAS or decision derivation.

## Cancellation And Crash Semantics

The ingress accepts no cancellation token and dispatches no effect:

- cancellation before invocation produces no store call.
- dropping the submitting task while a store future is pending does not grant
  command authority.
- cancellation cannot roll back a record whose durability was confirmed.
- a fresh process can derive the same pure decision from confirmed durability.
- an unresolved ambiguous outcome returns no decision.
- later caller items own cancellation between confirmed durability and fenced
  command dispatch.

The process proof must use the repository subprocess crash-cut harness. It
kills a writer before durable commit and after durable commit but before the
parent receives a dispatch-capable result. A genuinely fresh process opens only
the Engine root. The first cut reveals no confirmed new record. The second cut
reveals the exact record and deterministic decision.

## Later-Owner Map

No acceptance obligation disappears from the earlier omnibus wording.

| Caller or concern | Owning item | Required later proof |
| --- | --- | --- |
| Convex/Cloud Functions lazy activation | NNC6.3 | Full admission and source snapshot precede one exact compile/submission. Provision phases dispatch through fenced commands. Sync resolution remains read-only. |
| Native service start | NNC6.3 | Stable generation and exact execution source produce admit→reserve→start→attach→publish→observe. |
| Native standalone sandbox create | NNC6.3 | A tenant-qualified logical ID exists before provider start. The provider handle stays opaque. |
| Standalone Compose up | NNC6.3 | Use the canonical server/Engine-backed saga authority. Do not add a CLI-local store. The item must freeze server-client versus local Engine composition before editing. |
| Container/Krun eligible restart | NNC6.4a | Restart is a durable desired transition and same-generation fenced choreography, not local stop/start. |
| Service stop and force definition delete | NNC6.5 | Durable withdrawal precedes stop. Definition removal waits for safe terminal progression. |
| Standalone sandbox stop | NNC6.5 | Exact logical identity and generation precede withdraw→drain→stop→detach→release→record. |
| Standalone Compose down | NNC6.5 | Canonical durable authority precedes provider stop. Ambiguous persistence issues no stop. |
| Tenant retirement effect path | NNC6.5 then NNC6.1e2 | Teardown choreography first. Fresh-process durable enumeration and final convergence follow. |
| Service binding during withdrawal | NNC6.6 | A concurrent resolver cannot publish or return a newly routable handle after withdrawal starts. |
| Startup recovery and desired execution reconstruction | NNC6.1e2 | Fresh process obtains the exact durable execution source and emits only fenced compute commands. |
| Cleanup finalization and capacity reuse | NNC8.3 | Unknown effects remain cleanup-pending. Provider cleanup precedes fence release and reuse. |

Before NNC6.3 can dispatch `PrepareWorkload`, it must name a durable or exactly
reconstructable owner for the executable sandbox/service specification. The
current saga carries the complete network plan but not the executable
`SandboxSpec`. NNC6.1e1 does not conceal this gap by placing execution bytes in
an ingress-local cache or by claiming that a desired digest is executable
content.

## Explicit Non-Goals

NNC6.1e1 does not:

- construct a production service, sandbox, Compose, Machine, or tenant intent.
- allocate workload generation or mint a caller's logical workload ID.
- choose node identity, provider capability, or sovereignty profile.
- define canonical desired-spec encoding.
- resolve logical service names or own service definitions/sessions.
- install or remove `RuntimeServiceRegistry` implementations.
- call reserve, start, attach, publish, stop, detach, release, inspect, and reload.
- call
  cleanup, proxy, egress, listener, Netavark, nft, gvproxy, Iroh, cluster, cloud,
  or certificate effects.
- mutate observed `nimbus-system` projections.
- change saga phases, legal transition edges, or wire format.
- add an Engine adapter, store implementation, coordinator, command bus,
  generic repository, god provider, compatibility shim, feature flag, or Cargo
  edge.

## Frozen Source Allowlist

Product behavior:

```text
crates/nimbus-compute/src/workload_saga.rs
crates/nimbus-compute/src/workload_saga/ingress.rs
crates/nimbus-compute/src/workload_saga/ingress/tests.rs
crates/nimbus-server/src/workload_saga_store/tests/mod.rs
crates/nimbus-server/src/workload_saga_store/tests/ingress.rs
```

The server store implementation may enter the allowlist only if a fail-before
process test reproduces an adapter defect rather than an ingress defect. The
audit found no such defect. `nimbus-workloads/src/saga/**` may enter only if a
fail-before test proves the existing state machine cannot express one frozen
outcome. The audit found no missing phase or legal edge.

Static proof and recovery state:

```text
scripts/nimbus-network-control-plane/workload-saga-ingress-contract.sh
scripts/nimbus-network-control-plane/workload-network-plan-compiler-contract.sh
scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh
scripts/verify-nimbus-network-control-plane.sh
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/README.md
docs/private/plans/proof/nimbus-network-control-plane/nnc6.1e-durable-discovery-recovery-decisions.md
docs/private/plans/proof/nimbus-network-control-plane/nnc6.1e1-durable-workload-saga-ingress.md
```

Every effectful caller path is forbidden in this item. A source need outside
the allowlist requires an evidence-backed amendment before that path is edited.

## Modularity And Complexity Guard

The implementation must:

- keep `workload_saga.rs` as the thin coordinator composition root.
- put submission semantics and tests in the concept-owned `ingress` child.
- reuse the existing store port and ambiguity resolver.
- make raw `commit_loaded` internal.
- return typed saga/store errors without mapping them to `ComputeError` inside
  the state machine.
- keep policy, compilation, persistence, pure decision, and effect execution as
  separate stages.
- keep `nimbus-network` transport-free and effect-free.
- add no general `NetworkProvider`, command bus, repository abstraction, or
  second lifecycle facade.

The source audit also records later complexity owners:

- `RuntimeServiceRegistry` mixes read, activation, and teardown capabilities.
- `ServiceManager` combines policy projections, process-local claims, handles,
  observations, and provider commands.
- force definition deletion interleaves catalog CAS, sessions, inspection,
  stop, projection, and deletion.
- sandbox logical identity is currently learned after provider start and its
  generation is fixed to one.
- `ServiceActivationPlan` drops the source definition generation.
- standalone Compose has no canonical Engine saga composition.
- system projection failures after effects can invite unsafe client retries.

These pockets remain visible in the later-owner map. They do not justify
expanding NNC6.1e1 into caller choreography.

## Fail-Before Contract

Before implementation, NNC6.1e1 must add:

1. behavioral tests that fail because `submit_intent` and its confirmed result
   do not exist.
2. one distinct-process crash-cut test that fails because there is no public
   ingress to submit and recover.
3. `workload-saga-ingress-contract.sh`, wired as aggregate verifier NNCV030.
4. a fail-closed mutation suite for missing ingress, duplicate coordinator,
   externally callable raw commit, and effect authority in ingress. It also
   covers early decision, replay CAS, conflict retry, and a missing ambiguous
   fresh read. Crossed result identity and deferred-authority census drift are
   also covered.
5. an exact expected-red transcript linked from the item ledger.

The mutation count and complete aggregate arithmetic become immutable in the
fail-before checkpoint after the helper exists. The audit does not claim a
count for scripts that have not yet been written.

The historical command

```text
bash scripts/nimbus-network-control-plane/workload-saga-authority-contract.sh implementation
```

remains red with one failure because runtime lazy activation still bypasses the
compute saga. That red now belongs to NNC6.3's caller cutover. NNC6.1e1 must not
turn it green by moving a coarse effect call behind persistence.

### Recorded Fail-Before Evidence

The item-local compile probe added the frozen public type and method use to a
temporary compute integration test. The command

```text
timeout 300 cargo check -p nimbus-compute --test nnc61e1_fail_before
```

exited `101` with exactly the two target surface errors:

```text
E0432: no ConfirmedWorkloadSagaIntent in workload_saga
E0599: no method named submit_intent for &WorkloadSagaCoordinator
```

The temporary test was then deleted byte-for-byte. No product path remains
dirty from the probe.

The new item contract exits `1` with 17 exact gaps: missing concept-owned
source, module/export, public ingress, internal raw commit, seven behavioral
cases, and six process-proof seams. Its mutation suite is green `12/12` for:

```text
missing-ingress
duplicate-submit
public-raw-commit
effect-import
missing-replay-test
missing-ambiguity-test
missing-crossed-key-test
missing-contention-harness
missing-crash-harness
duplicate-coordinator
unexpected-path
wrong-plan-route
```

The aggregate verifier is exact expected red:

```text
PASS NNCV000 through NNCV029
FAIL NNCV030 durable-workload-saga-ingress
Summary: 30 passed, 1 failed
```

The retained complete mutation baseline is `215/215`. NNCV030 adds 12 direct
green fail-closed cases, so the frozen aggregate arithmetic is `227/227`.
Product implementation may start only from this checkpoint.

## Acceptance Matrix

| ID | Verifiable criterion | Required evidence |
| --- | --- | --- |
| I1 | Exactly one product coordinator remains and it exposes one durable submission capability. | Construction census plus compile-time API test. |
| I2 | Missing valid intent produces the exact initial durable record and decision. | Behavioral store-spy test. |
| I3 | Exact replay performs zero CAS and returns the same record/decision as confirmed replay. | Behavioral call-count test. |
| I4 | Higher generation in every active provision phase confirms withdrawal before exposing any successor reservation decision. | Table-driven phase test. |
| I5 | Higher generation after terminal `Recorded` and a missing stopped intent preserve existing state-machine semantics. | Table-driven terminal tests. |
| I6 | Stale and equal-divergent intent perform zero CAS and return typed failures. | Behavioral negative tests. |
| I7 | Unavailable, corrupt, crossed-key, and invalid load/intent paths expose no decision. | Behavioral negative tests. |
| I8 | CAS conflict performs one CAS, no retry, and exposes no decision. | Behavioral call-order test. |
| I9 | Every ambiguous branch performs exactly one fresh read and confirms only an exact resulting record. | Behavioral ambiguity matrix. |
| I10 | Returned identity, generation, admission, desired content, complete compiled plan, revision, phase, transition, and decision are exact. | Full-field equality assertions. |
| I11 | Two distinct processes submitting the same intent converge on one record. Divergent equal-generation submissions yield one winner and one typed conflict. | Repository process-contention harness. |
| I12 | Crash before durability exposes no new confirmed record. Crash after durability exposes the exact record and deterministic decision to a fresh process. | Repository subprocess crash-cut harness. |
| I13 | Cancellation never grants effect authority or rolls back confirmed durability. | Bounded task-drop tests and absence-of-effect structural proof. |
| I14 | `commit_loaded` is internal and no second transition writer, store, coordinator, or lifecycle facade exists. | Rust visibility proof and source census. |
| I15 | Ingress contains no provider/effect call and all effectful caller paths remain byte-unchanged. | NNCV030 plus frozen-path diff. |
| I16 | Logical naming, policy, compilation, desired-spec ownership, provider selection, and system projection stay outside ingress. | Dependency/source scans and type surface inspection. |
| I17 | `nimbus-network -> nimbus-core` remains its only workspace edge. No manifest changes occur. | Cargo metadata comparison and manifest diff. |
| I18 | Focused, full affected, static, format, Clippy, docs, and site gates pass with exact counts. | Command transcript in this proof. |
| I19 | One candidate-frozen GPT-5.6 Sol/xhigh/fast structured review is fully dispositioned. | Review artifact after I1-I18 are green. |
| I20 | Exact proof, recovery header, ledger, committed tree, and item commit are durable. | Plan/index links and Git object proofs. |

No criterion can be marked green from compilation alone. Behavioral tests must
assert exact results, call order, call count, edge/error cases, and bounded
process cleanup.

## Audit Checkpoint Verification

The prospective freeze ran these checks before product implementation:

| Check | Result |
| --- | --- |
| Three independent read-only lanes plus owner audit | One coordinator, zero product compiler/intent construction, complete caller/effect census, and no file changes during audit. |
| Task/ledger correspondence | `99/99`; zero duplicates or missing rows. |
| Historical caller cutover | Expected red `0/1` with only runtime lazy activation reported. |
| New proof technical-writing lint | Pass with warnings only. |
| NNCV028 compiler contract | `18` direct checks and `7/7` fail-closed mutations. |
| NNCV029 durability contract | `23` direct checks and `10/10` fail-closed mutations. The two new cases reject a missing completion checkpoint and unreadable frozen source range. |
| Live aggregate verifier | Audit baseline `30/30`; after NNCV030 wiring, exact expected red `30/1` with NNCV030 as the sole failure. |
| Aggregate mutation arithmetic | Prior durable baseline `213/213` plus two new targeted NNCV029 mutations equals `215/215`. An unsplit replay reached the unchanged bind-exemption prefix before its 600-second bound; this checkpoint does not misreport that timed-out replay as a new full pass. |
| Script checks | Bash syntax and ShellCheck pass for both changed helpers. Aggregate ShellCheck passes with its established SC2034/SC1091 exclusions. |
| Docs | `check-docs` passes `108` pages; docs-site verification passes `17/17`. |
| Diff and ledger integrity | `git diff --check` passes; the completed NNC6.1e proof is byte-unchanged; no product or manifest path changed. |

The verifier range correction is load-bearing. NNCV029 now checks the exact
NNC6.2a source range from its starting checkpoint through durable item commit
`ba7830360`. It fails closed when either Git object or the range is unreadable.
Future item paths no longer invalidate the completed NNC6.2a allowlist.

## Review Cadence

NNC6.1e1 is one review unit. No structured autoreview runs during this audit,
fail-before work, implementation, cleanup, or acceptance convergence. After
I1-I18 are green and the complete item is candidate-frozen, run exactly one
full structured autoreview with GPT-5.6 Sol, xhigh reasoning, and fast mode.

If that review finds an accepted defect that materially changes executable
code, rerun the affected proofs and one narrow correction review focused on
that defect. Do not rerun for proof wording, ledger updates, formatting,
non-material cleanup, elapsed time, or internal diff size.

## Candidate Verification

The pre-ledger executable candidate contains five exact paths. Its staged tree
is `13edd9a528fbb60eee8c11819d8a9224537a680a`. Its complete staged patch SHA-256
is
`6e3b1f1600e0f00b0000fd3e6da441d6fd64e0c3c880955e7ab05c1628edf4a2`.
Ledger-only edits do not change those executable bytes.

| Gate | Candidate result |
| --- | --- |
| Ingress behavioral matrix | `10/10`: exact missing, replay, every active provision successor, terminal/stopped, typed stale/divergent/invalid, conflict, complete ambiguity, crossed-key, and bounded pre-commit cancellation outcomes pass. |
| Distinct-process proof | `2/2` parent proofs pass with one child-only ignore. The corrected lane also passes 20 consecutive repetitions, or `40/40` parent-test executions. Same intent converges on one exact record. Divergent equal-generation intent produces one winner and one typed conflict. The winner cannot submit until the second process acknowledges contention. The pre-durability child runs `submit_intent` through its load and into a parked CAS before the kill; recovery reveals no record. After durability, a fresh process recovers the exact record and decision without snapshot handoff. |
| Full compute | `120/120` pass. One compiler child-only test is ignored. |
| Full server | The post-correction unsplit lane passes `640/640` with 29 declared skips. No fixture is filtered and no retained-authority collision occurs. |
| Affected compile/lint/docs | Post-correction all-target compute/server check, strict server Clippy, warning-denied compute/server rustdoc, format, and both diff checks pass. The initial correction Clippy run found one local `never_loop`; the composition was simplified and the exact rerun is green. Docs pass `108` link-clean pages and the site gate passes `17/17`. |
| Static and adversarial | NNCV030 passes `10` direct checks and `12/12` fail-closed mutations. The live aggregate is `31/31`. The retained complete mutation arithmetic is `215 + 12 = 227`. |
| Seam audit | One coordinator and one public submission method remain. No manifest, effectful caller, provider, compiler, naming, policy, system-projection, or `nimbus-network` path changed. `nimbus-network -> nimbus-core` remains its only workspace edge. |
| Modularity | Coordinator root is 99 lines, ingress is 104, concept-owned behavioral tests are 760, and corrected process proofs are 495. No modularity threshold is crossed. |

I1-I20 are green. The full item review is completely dispositioned, and the
one narrow correction review is clean. The ledger-bearing item commit contains
this proof, the recovery transition, and the exact reviewed executable tree.

## Complete-Item Review And Corrections

The sole full structured review ran through the Nimbus autoreview wrapper with
actual GPT-5.6 Sol, xhigh reasoning, fast mode, tools enabled, and no fallback.
Detached review object `0ccac9af65ee288d777780a4d1c6e29124304e6c`
has starting checkpoint `b0a97e4404cd8afc3b11fdbf2053fa12e0b1c3d7`
as its parent, so the one review covered both durable checkpoint commits and
the staged candidate without moving `HEAD`. TruffleHog was clean. The review
reported three findings and rated the pre-correction candidate incorrect at
confidence `0.96`.

| Finding | Disposition | Correction and proof |
| --- | --- | --- |
| P2: the pre-durability crash child did not invoke `submit_intent` or enter CAS. | Accepted. This failed I12's required ingress crash cut. | `PreCommitCrashStore` delegates the real Engine-backed load, acknowledges entry from its `compare_and_swap`, and parks before any durable write. The child spawns the real submission, waits boundedly for that checkpoint, proves the task has not returned, and only then reports the kill boundary. Fresh-process recovery observes no record. |
| P2: winner/contender classification depended on whether the loser happened to see a temporary lock before it disappeared. | Accepted. This made I11 scheduler-dependent. | The first owner now waits for an atomic contender acknowledgement before submission. The contender retains that acknowledgement until it acquires the released single-writer lock. Thus the winner cannot commit and unlink before the other process has observed ownership. Focused `2/2`, 20 repeated lanes or `40/40` parent executions, and full server `640/640` pass. |
| P3: the proof header still described expected-red fail-before state. | Accepted. The stale recovery direction could restart implementation work. | The header now records correction verification and the pending narrow review. Plan/index recovery text carries the same state. |

The production ingress implementation did not change. The accepted executable
findings changed only the required process proof, so affected process/full-server,
check, strict Clippy, rustdoc, static, format, and diff proofs were rerun. The
one allowed narrow correction review must examine only these two test defects
and their proof truthfulness.

Actual GPT-5.6 Sol, xhigh reasoning, and fast mode
reviewed correction object `176bc9d82d9e6ef819a76e9a51d7772eed5425e6`,
whose parent is complete-item review object `0ccac9af65ee288d777780a4d1c6e29124304e6c`.
It reported zero findings and rated the patch correct at confidence `0.94`.
The reviewer confirmed both bounded process corrections and the recorded
focused, repeated, full-server, static, and docs evidence. No third review ran
or is warranted.

## Recovery Checkpoint

| Field | Value |
| --- | --- |
| Current item | `NNC6.1e1` |
| Starting checkpoint | `b0a97e4404cd8afc3b11fdbf2053fa12e0b1c3d7` |
| Audit state | Complete and durable. Three independent read-only lanes plus owner inspection; fail-before commit `f8bd2b923166cea6413866cf7012accaab970106`. |
| Boundary decision | Durable submission and confirmed pure decision only. All caller/effect cutovers belong to NNC6.3/NNC6.4a/NNC6.5/NNC6.6/NNC6.1e2. |
| Current acceptance | I1-I20 green. Ingress `10/10`; process `2/2` plus one child-only ignore and 20 repeated lanes (`40/40` parent executions); compute `120/120` plus one ignore; unsplit server `640/640` plus 29 skips; NNCV030 direct `10`, mutations `12/12`, aggregate `31/31`; affected check/strict Clippy/rustdoc/format/diff/docs gates pass. |
| Final scope | Five exact compute/server candidate paths plus this proof, canonical plan, and routing index. No manifest, effectful caller, provider, compiler, naming, policy, projection, or `nimbus-network` path changed. |
| Complete-item review | Actual GPT-5.6 Sol/xhigh/fast, no fallback, three accepted findings at overall confidence `0.96`. Production ingress semantics were judged consistent; two process-proof defects and one stale status were corrected. |
| Narrow correction review | Actual GPT-5.6 Sol/xhigh/fast, no fallback, zero findings, patch correct at confidence `0.94`; correction object `176bc9d82d9e6ef819a76e9a51d7772eed5425e6`. Review cadence exhausted. |
| Candidate identity | Corrected executable tree `c58924152386425107a6623bf08289258692eff3`; five-path patch SHA-256 `0d7fbd10c14c26fd10c5b4840a8df33b967bcbdb1aec09c85827f52b9d715063`; complete staged tree before final docs result wording `cb9ce45a3b2fe2c9e32e79fc7551cadff290662c`. |
| Durability | The exact ledger-bearing NNC6.1e1 item commit contains this proof and recovery transition. Verify it with `git cat-file -e HEAD:docs/private/plans/proof/nimbus-network-control-plane/nnc6.1e1-durable-workload-saga-ingress.md`. |
| Next safe action | Begin NNC6.3 with the frozen read-only substitution and executable-spec ownership audit. Do not edit effectful callers until that item's fail-before boundary is durable. |
| Blocker | None. |
