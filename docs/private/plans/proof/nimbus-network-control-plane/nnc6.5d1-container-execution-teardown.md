# NNC6.5d1 Container Execution Teardown

Status: `complete; the containing commit is the durable item checkpoint`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Scope

NNC6.5d1 adds one exact Container execution drain/stop implementation and the
shared, workload-neutral sandbox teardown command substrate that it earns. The
item extends the existing provider-command journal. It adds no second journal.
It does not detach or release network authority, change Krun behavior, cut over
a product caller, or delete the coarse sandbox stop path.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| C1 | `nimbus-sandbox` exposes a workload-neutral, tenant-qualified execution teardown locator and closed drain/stop observations without importing `nimbus-workloads`. |
| C2 | The confirmed compute command retains the exact `WorkloadExecutionReference` from the durable teardown origin for every effectful step. |
| C3 | Command construction and both asynchronous callback fences authenticate the retained execution locator, required node, generation, desired digest, source digest, network-plan digest, provider target, teardown attempt, command ID, and dispatch epoch. |
| C4 | Compute derives `SandboxId` and `SandboxExecutionAttemptId` only from the retained execution locator. An IP address, port, directory order, or executable decode is never workload identity. |
| C5 | `ProviderCommandOperation` has distinct teardown operations. Teardown uses no restart source and restart ordinal zero. Provision and restart validation remain exact. |
| C6 | The Container adapter uses the backend's existing `ProviderCommandAttemptJournal`; no second journal, namespace, or durable command authority exists. |
| C7 | Drain durably publishes the exact provider-admission barrier before it can report success. The runtime stays running and every network authority remains unchanged. |
| C8 | The drain barrier rejects new creator, direct activation, runner-effect, restart, and legacy launch admission for the exact execution. Already-admitted work reports progress or ambiguity until it settles. |
| C9 | Execute-side drain and stop mutations use the exclusive lifecycle lock and a canonical manifest reread. Inspect uses the shared existing lock, is read-only, and creates no synchronization artifact. |
| C10 | Stop requires the exact drain fence for the same command subject and persists stop intent before runtime inspection. |
| C11 | The provider persists an effect-may-exist state before each TERM or KILL effect. |
| C12 | A signal authenticates the exact runtime ID and creator-attempt annotation immediately before the provider effect. A raw numeric PID never authorizes a signal. |
| C13 | Recovery inspects the exact runtime before retry. Response loss alone, unknown provider state, a crossed attempt, and corrupt or missing evidence cannot authorize another signal. A stop-only `RetryAuthorized` receipt can authorize the exact next epoch only after exact-incarnation liveness reaches a named reconciliation deadline. The Container effect owner revalidates that the claimed epoch is still current under the provider-journal lock before any transition; each epoch can issue at most one semantically idempotent KILL. |
| C14 | Stop succeeds only from an exact exit receipt or explicit provider absence for the retained execution attempt. Exact terminal or absent state, an elapsed TERM deadline, or exact-incarnation liveness after the KILL reconciliation deadline can authorize only the next exact dispatch epoch. |
| C15 | Execution stop leaves the overall Container manifest nonterminal and preserves the attachment, netns, provider, PEP, listener, port, IPAM, segment, and network-plan authority byte for byte. |
| C16 | The legacy coarse stop remains callable and unchanged in authority until NNC6.5g. The exact adapter never calls its combined cleanup helpers. |
| C17 | Exact replay adopts the current provider observation. A delayed older claimant cannot start after inspection advances durable authority. Stale, skipped, crossed, corrupt, or unknown claims fail closed with the frozen outcome and zero effect. |
| C18 | Two drain contenders publish one barrier. Two stop contenders produce at most one signal for one attempt and epoch. |
| C19 | Independent fresh-process cuts reopen only the manifest and provider-journal roots and converge across barrier, intent, TERM-may-exist, KILL-may-exist, retry-claim-before-manifest, terminal evidence, provider result, and compute-CAS boundaries. |
| C20 | A real `ContainerTeardownAdapter` substitutes for the exact compute drain and stop capability roles without fallback or a god provider interface. |
| C21 | The item adds no Krun behavior, detach/release behavior, machine transport, caller cutover, coarse-stop deletion, or `nimbus-network` effect/dependency. |
| C22 | `container/runtime.rs`, `container/runtime/runner.rs`, and `container/runtime/restart.rs` gain no concept logic. New teardown lifecycle logic and tests stay in concept-owned children. |
| C23 | Focused and full `nimbus-sandbox` and `nimbus-compute` tests, strict Clippy, rustdoc, format, dependency/effect scans, and the teardown verifier pass with exact counts. |
| C24 | NNCV035 remains the sole expected aggregate red condition at exact `0/7`; the item does not claim caller cutover or deletion. |
| C25 | Docs gates, strict proof writing lint, diff checks, and ledger/routing recovery pass. The item review is a separate closeout gate and runs only after C1-C25 are green. |

## Read-Only Audit

### Current call graph

```text
SandboxBackend::stop
  -> ContainerSandboxBackend::stop_sync
     -> exclusive runner lifecycle reconciliation
     -> execute_stop
        -> read numeric PID
        -> TERM, wait, optional KILL
        -> release_execution_artifacts
           -> runtime delete
           -> provider and netns detach
           -> PEP/listener/IPAM/segment release
        -> terminal manifest
```

The coarse path cannot report execution stop while it retains network
authority. It also sends the first signal before durable stop intent.

### Existing authorities to reuse

| Concept | Existing authority |
| --- | --- |
| Upper lifecycle order and desired state | `nimbus-workloads` teardown reducer plus the compute coordinator |
| Provider dispatch identity and replay | `nimbus-sandbox::ProviderCommandAttemptJournal` |
| Container effect progress | `ContainerSandboxManifest` |
| Execute serialization | runner exclusive lifecycle lock plus canonical manifest reread |
| Read-only inspection serialization | runner shared lifecycle lock plus canonical manifest reread |
| Runtime identity | runtime ID plus creator-attempt annotation from exact runtime state |
| Runtime terminality | exact exit receipt or explicit runtime-provider absence |
| Network authority | existing attachment, provider, PEP, listener, port, IPAM, segment, and manifest owners |

The durable workload record already retains the exact
`WorkloadExecutionReference`. `ConfirmedWorkloadTeardownCommand` currently
drops it when it creates the ephemeral provider command. The command result and
raw provider observation also omit that independent fence.

## Fail-Before Baseline

Captured at branch HEAD
`b186074e105dd3f5636df4361b3dbdb0a2596887` before a product-source edit.

| Check | Expected-red result |
| --- | --- |
| `test -f crates/nimbus-sandbox/src/teardown.rs` | Exit 1: no neutral sandbox teardown contract exists. |
| `test -f crates/nimbus-compute/src/workload_saga/teardown_sandbox.rs` | Exit 1: no real Container teardown adapter exists. |
| `test -f crates/nimbus-sandbox/src/backends/container/runtime/teardown.rs` | Exit 1: no concept-owned Container execution teardown state machine exists. |
| `rg -q 'DrainExecution|StopExecution' crates/nimbus-sandbox/src/provider_command.rs` | Exit 1: the provider journal has no teardown operation family. |
| `rg -q 'execution_locator' crates/nimbus-compute/src/workload_saga/teardown_command.rs` | Exit 1: the confirmed command has no retained execution locator. |
| `rg -q 'execution_teardown' crates/nimbus-sandbox/src/backends/container/runtime/manifest.rs` | Exit 1: the manifest has no durable drain/stop progress. |

Source inspection also proves these behavioral failures:

- `activate_provision_workload` does not share an exclusive barrier with
  teardown before creator spawn or runtime start.
- restart and legacy launch paths do not reject a durable final-drain barrier.
- `execute_stop` uses `read_pid` and `signal_process`, then calls
  `release_execution_artifacts`.
- a manifest cannot represent exact execution terminality while network state
  remains retained.

## Frozen Translation And Outcome Rules

Compute keeps all `nimbus-workloads` types. Sandbox receives only neutral IDs,
opaque digest strings, and its own exact `ProviderCommandClaim`.

The provider claim binds the teardown saga as authority and the teardown
attempt as `attempt_id`. The canonical effect subject contains the retained
execution locator. The claim also binds dispatch epoch, generation, all content
digests, and the exact provider target. It has no source attempt and uses
restart ordinal zero.

| Provider observation | Execute result | Inspect result |
| --- | --- | --- |
| `Succeeded` | Step-specific success | `Satisfied` |
| `DefiniteFailure` | `DefiniteFailure` | `DefiniteFailure` |
| `Absent` | `Ambiguous` | `NotCompleted` |
| `RetryAuthorized` | `Ambiguous` | `NotCompleted` |
| `Claimed` or `InProgress` | `Ambiguous` | `InProgress` |
| `Ambiguous` | `Ambiguous` | `Ambiguous` |

Execute-side absence or retry authority never authorizes a new epoch. Only a
confirmed Inspect command can turn exact provider-proven absence or stop-only
safe-redelivery evidence into `NotCompleted`.

## Required Behavior Proofs

The focused test roster must cover:

- retained locator construction, execute/inspect parity, and callback crossing.
- validation failure before journal or backend mutation for every stable fence.
- exact provider-role registration with no fallback.
- durable drain barrier and runtime-preserving success.
- activation, creator, runner, restart, and legacy-launch admission rejection.
- stop ordering, runtime-attempt authentication, TERM/KILL may-exist states,
  response loss, exact terminality, and unknown/crossed fail-closed outcomes.
- byte-exact network authority across drain, stop, replay, ambiguity,
  contention, and fresh-process recovery.
- one effect for two thread contenders and one effect for two process
  contenders at the same exact attempt and epoch.

## Implemented Seams

### Neutral command contract

`nimbus-sandbox::teardown` owns the workload-neutral execution locator,
closed operation and observation vocabulary, and exact command/result records.
The locator is tenant-qualified and retains sandbox ID, execution-attempt ID,
generation, and restart ordinal. It has no `nimbus-workloads` dependency.

Compute retains the durable `WorkloadExecutionReference` in the confirmed
command. Command construction compares the locator and every existing stable
fence. Both asynchronous result callbacks make the same comparison before a
provider command can run or a result can commit. The Container adapter derives
sandbox identity only from this locator.

### One provider journal

The existing `ProviderCommandAttemptJournal` now accepts the distinct
`DrainExecution` and `StopExecution` operations. Both require no source
attempt and restart ordinal zero. Provision and restart retain their prior
validation rules. The adapter uses the existing `container-runtime` journal
root and does not add another namespace or command store.

Provider-journal failures retain the frozen outcome vocabulary. Each claimed
retry carries a strict ordered lineage of exact `Absent` or stop-only
`RetryAuthorized` receipts. The lineage rejects gaps, crossed attempts,
checksum-valid semantic corruption, and missing fields before backend access:

- an invalid claim is `sandbox_teardown_command_invalid`.
- stale generation, restart ordinal, or dispatch epoch is
  `sandbox_teardown_command_stale`.
- skipped or crossed epochs and unresolved earlier effects are
  `sandbox_teardown_epoch_invalid`.
- corruption or store ambiguity is `sandbox_teardown_ambiguous`.

Only an `ExecuteClaimed` result can invoke a provider effect. Exact replay
adopts the durable observation. Inspect-side absence and retry authority report
`NotCompleted`. Execute-side forms remain ambiguous and cannot mint a retry
epoch.

The execution claim is a single-use credential, not a durable authority by
itself. The Container effect owner reopens the same workload root and
`container-runtime` namespace, then calls `execute_current_claim`. That method
locks the exact journal stream, revalidates the complete current observation,
and holds the lock through the Container transition and provider-result
publication. It returns that exact durable observation to compute. Compute
does not publish a second result after the journal releases the lock.

A delayed epoch-N caller fails before a manifest write or provider effect when
epoch N+1 is current. An inspector cannot advance authority between an effect
and its result. The unclaimed exact execution method exists only in the private
test harness.

### Container lifecycle

Drain runs under the existing exclusive lifecycle lock, rereads the canonical
manifest, and durably publishes an exact execution barrier. It leaves the
runtime and all network authority unchanged. Creator, activation, runner,
restart, and legacy launch admission all consult this barrier before their
first effect. The exclusive lock treats an owner-dead runner claim as no-effect
when no effect started. Completed durable restart checkpoints are also settled
boundaries. An effect cannot start after drain because the later runner
transition checks the same barrier.

Stop requires the matching drain subject. It persists stop intent, then a
TERM-may-exist boundary before TERM. A still-live exact process after the
durable TERM deadline can authorize KILL only at the exact next dispatch
epoch, after a KILL-may-exist boundary is durable. TERM is never redelivered.

KILL response loss alone cannot authorize redelivery. If the same authenticated
process incarnation remains live after the named one-second reconciliation
deadline, a stop-only `RetryAuthorized` receipt permits the exact next epoch.
That epoch can send one semantically idempotent KILL. The provider persists
that epoch and its next deadline before signaling. Both may-exist recovery
paths check the exact exit receipt before pidfile inspection. Success requires
that receipt or authenticated provider absence.

The manifest remains `Stopping`. Attachment and every network resource remain
retained.

### Authenticated process effect

The conmon child module captures runtime ID, creator attempt, provider PID,
pidfile PID, and OS process-birth evidence. Immediately before a signal it
reauthenticates OCI state, the bounded no-follow regular pidfile, and process
birth. Linux uses pidfd open, revalidation, and pidfd signal delivery. A raw
PID, recycled process, missing/crossed evidence, or unsupported signal cannot
authorize an effect. The existing coarse conmon helpers remain unchanged for
the later deletion gate.

### Small capability substitution

`ContainerTeardownAdapter` implements only the compute-owned execution drain
and execution stop capabilities. Both roles share one narrow provider-phase
adapter and the existing journal. The registry test substitutes the real
adapter for both roles, reopens the journal, and proves exact replay without a
fallback or broad provider interface.

### Routed shared-journal finding

The correction audit also found that earlier provision and restart producers
discard `ExecuteClaimed` credentials before their effects. This behavior
predates NNC6.5d1 and is outside the Container teardown path. NNC8.2 owns the
cross-producer correction and must prove that delayed provision or restart
epochs cannot start after exact inspection authorizes a later epoch. This item
does not claim a global live-claim exclusion.

### Modularity disposition

The new teardown lifecycle and tests stay below 1,500 lines in concept-owned
children. Three changed inherited files remain above the review thresholds:

- `container/runtime/runner.rs` remains the runner-handoff state machine. Its
  NNC6.5d1 edit is one admission check and one teardown-field exclusion in the
  runner projection. Extracting either line would split the runner invariant.
- `container/runtime.rs` remains the Container composition root. Its edits
  register the teardown child, inject its runtime capability, initialize
  manifest state, and call one admission guard.
- `container/runtime/restart.rs` remains the restart state machine. Its edit
  delegates one admission decision to the manifest-owned teardown guard.

No new lifecycle state machine or provider effect remains in those inherited
files.

## Reliability Proofs

The in-process state-machine tests prove:

- drain persistence, read-only inspection, and byte-stable runtime/network
  authority.
- exact stop terminality with retained network authority.
- crossed-command and stale-epoch failure before durable mutation.
- delayed live-claim rejection after the journal advances to the next epoch.
- TERM/KILL may-exist ordering and no duplicate signal.
- exact receipt convergence when the pidfile is already absent.
- one barrier and one signal under thread contention.
- every creator, activation, runner, restart, and legacy-launch admission
  fence.

The populated attachment fixture includes the canonical network control-plane
store. It also includes provider state, netns/status artifacts, listener and
port lease, PEP, IPAM, segment, plan, and manifest-held network fields. It
proves byte identity across drain, TERM ambiguity, KILL ambiguity, terminal
persistence, and replay.

Two independent OS processes contend over one manifest and provider journal.
The parent proves distinct child PIDs, exactly one `ExecuteClaimed`, exactly
one exact adopter for drain and stop, and exactly one TERM witness.

The fresh-process crash matrix reopens only durable roots after cuts at:

1. drain barrier.
2. stop intent.
3. TERM before effect.
4. TERM response loss.
5. KILL before effect.
6. KILL response loss.
7. terminal evidence.
8. provider-result loss.

A separate five-process proof crashes twice immediately after adjacent retry
claims and before manifest progress. Two distinct inspectors use the exact
journal receipt lineage without writes, and a fifth process advances the
original manifest fence to the fourth epoch. An in-process matrix proves the
same two-epoch lag for drain plus stop intent, TERM-may-exist, and
KILL-may-exist. Direct backend calls without the journal receipt fail before a
durable byte or signal changes.

A separate production-entry proof retains an epoch-1 stop credential, advances
the durable journal to epoch 2, and then invokes the delayed credential. The
Container effect owner returns the exact typed stale-epoch error. The compute
adapter maps it to the frozen definite failure. The rejected call changes no
durable byte, and stop progress stays `NotRequested`.

A separate publication-gap proof holds one execution effect under the exact
journal lock while an inspector waits. The executor publishes and receives its
exact `InProgress` observation before the inspector can publish
`RetryAuthorized`. A compute-level regression advances the journal after that
publication and proves that the original Execute result remains ambiguous,
not a false definite failure.

Recovery inspects first. It uses only the exact next epoch after durable
absence or stop-only retry authority. Recovery never duplicates TERM. It
limits KILL to one delivery per authenticated epoch. It preserves the populated
network authority.

The existing NNC6.5b server process matrix supplies the separate generic
compute-CAS proof. It kills distinct writer processes after claim CAS and after
provider effect. It then reopens the real Engine store, requires Inspect first,
and proves no effect replay.

## Acceptance Matrix

| Criteria | Result | Evidence |
| --- | --- | --- |
| C1-C6 | `pass` | Neutral sandbox types, locator-retaining compute fences, closed journal operation validation, and exact shared-journal substitution tests pass. |
| C7-C10 | `pass` | Drain persists the exact barrier under the lifecycle lock, blocks all five admission paths, keeps execution live, and stop rejects a mismatched drain subject. |
| C11-C14 | `pass` | Intent and TERM/KILL may-exist states are durable; process identity is authenticated; response loss alone cannot authorize a signal; exact-incarnation liveness after the named KILL deadline permits one next-epoch redelivery; only exact receipt or provider absence is terminal. |
| C15-C16 | `pass` | Populated network authority is byte-stable through all execution teardown states. The legacy coarse stop remains present and the exact adapter never calls its cleanup helpers. |
| C17-C20 | `pass` | Frozen result codes, exact replay, strict receipt lineage, delayed-claim exclusion, atomic effect/result publication, stale/crossed zero-effect behavior, thread/process contention, eight fresh-process cuts, the five-process retry-claim cut, generic compute-CAS cuts, and real small-capability substitution pass. |
| C21-C22 | `pass` | No Krun, detach/release, machine, caller-cutover, deletion, network effect, or dependency entered the item. Teardown logic and tests are concept-owned children; composition-root edits are narrow guards or exports. |
| C23-C24 | `pass` | Focused and full affected tests and quality gates pass. NNCV035 is exactly `0/7`; aggregate verification is `35/36` with only NNCV035 red. |
| C25 | `pass` | Docs pass `108` pages; site verification passes `17/17`; strict proof writing lint reports zero diagnostics; format and diff checks pass; the plan and routing index recover this exact candidate. |

## Verification Record

The one full item review ran against the complete candidate and found three
accepted executable defects. A later manual correction audit found one
same-scope stale-live-claim race. The one authorized narrow correction review
found one related result-publication race. The implementation corrects all five
findings.

The executable correction proofs, full affected tests, quality/static gates,
and docs gates pass. The review cadence ended. The task ran no third structured
review.

| Gate | Result |
| --- | --- |
| Fail-before baseline | `6/6` expected-red source conditions at the recorded baseline. |
| Focused behavior | Provider journal `19` passed, `1` child-only ignored; Container teardown `19` passed, `1` child-only ignored; compute Container adapter `6` passed; conmon runtime-process identity `8` passed; manifest durability `19` passed; NNC6.5b compute-CAS subprocess proof `2` passed, `1` child-only ignored. The stale journal token, production Container delayed-stop, exact locked publication, and compute publication-race tests each pass `1/1`. |
| Full affected crates | `nimbus-compute --all-features`: `352` passed, `1` ignored. Serialized `nimbus-sandbox --all-features`: `1,049` passed, `44` declared ignored across the library, helper, and provider-specific integration targets. The earlier unaffected `nimbus-cli --all-features` gate remains `948` passed, `1` ignored; final check and strict Clippy compile its corrected dependency graph. |
| Transient host rerun | One parallel sandbox run reported one short-lived Krun PID race and two local readiness-server `WouldBlock` failures. Exact serialized reruns passed Krun `1/1` and readiness `10/10`; the later complete serialized suite passed. |
| Dependency and effect scans | Cargo metadata reports exactly `[\"nimbus-core\"]` as `nimbus-network` workspace dependencies. The direct forbidden-dependency/effect source contract and aggregate NNCV012 pass. Strict Clippy and warning-denied rustdoc pass for all three affected crates; only inherited vendored Brotli warnings are emitted outside the owned crates. |
| NNCV035 expected-red arithmetic | Direct self-test `55/55`; direct live result `0/7`; aggregate `35/36`, with only NNCV035 red. |
| Docs and writing gates | Docs `108`; site `17/17`; strict proof writing lint zero; format and diff checks pass. |
| Candidate-frozen Sol review | Complete: GPT-5.6 Sol/xhigh/fast reviewed the complete 281,204-byte staged bundle in thread `019fe832-401c-78c2-8171-88ea3f38e089`; three findings were accepted at overall incorrect confidence `0.99`. |
| Narrow Sol correction review | Complete: GPT-5.6 Sol/xhigh/fast reviewed the complete 367,583-byte corrected bundle in thread `019fe889-679f-7672-8967-ac7c4952a09a`; one P2 publication-gap finding was accepted at confidence `0.94`. The correction publishes the provider result under the live-claim lock and returns that durable observation to compute. Both new regression proofs, both full affected suites, and all quality gates pass. |
| Final executable identity | The final pre-ledger Rust patch changes `36` files with `7,036` additions and `83` deletions. Its SHA-256 is `e51a878979a44b3cd5577d5f9d6a37d985225c5d5fc323da55283c0d180bd10f`. |

## Review Ledger

| Review | Result | Disposition |
| --- | --- | --- |
| Full item review | Three accepted findings | P1: retry claim could precede manifest epoch persistence. P1: KILL-before-effect could leave an exact live process in progress forever. P2: missing manifest teardown fields defaulted fail-open. All three corrections are implemented and their affected proofs pass. |
| Manual correction audit | One accepted same-scope finding | A delayed epoch-N credential could start after Inspect advanced the journal to epoch N+1. `execute_current_claim` now revalidates and locks the exact Container effect interval; both fail-before tests pass. Earlier provision/restart producers are routed to NNC8.2. |
| Narrow correction review | One accepted P2 finding | `execute_current_claim` released the exact-stream lock before compute published the callback result. The provider journal now publishes the result under the same lock and returns the durable observation; compute consumes it without a second write. The journal and compute publication-race regressions, focused suites, full affected suites, and strict quality gates pass. No third review ran. |
