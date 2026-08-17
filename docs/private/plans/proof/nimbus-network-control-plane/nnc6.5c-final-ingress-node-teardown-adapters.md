# NNC6.5c Final Ingress And Node Teardown Adapters

Status: `complete; C1-C24 green; review cadence exhausted`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

Item checkpoint before work: `3976a8b6c475bd60dcee347ba39182f3944a701d`

## Acceptance-Freeze Evidence

The owner froze C1-C24 before product or test edits. The proof used the clean
item checkpoint above and then staged only this proof and its ledger updates.

| Gate | Result | Artifact |
| --- | --- | --- |
| Strict proof lint | One file, zero diagnostics | `/tmp/nnc65c-proof-lint.json` |
| Documentation links and source map | 108 pages passed | `/tmp/nnc65c-acceptance-freeze-docs.out` |
| Documentation site contract | 17 of 17 conditions passed | `/tmp/nnc65c-acceptance-freeze-site.out` |
| NNCV035 mutation suite | 55 passed, zero failed | `/tmp/nnc65c-acceptance-freeze-nncv035-self.out` |
| NNCV035 direct baseline | Expected nonzero; exact `0 passed, 8 failed` | `/tmp/nnc65c-acceptance-freeze-nncv035-direct.out` |
| Aggregate verifier baseline | Expected nonzero; exact `35 passed, 1 failed`, only NNCV035 | `/tmp/nnc65c-acceptance-freeze-verifier.out` |

The direct wrapper reports zero passes until all diagnostics clear. NNC6.5c
clears only `ingress`, so its closeout target is `0 passed, 7 failed`, not
`1 passed, 7 failed`.

## Candidate-Freeze Evidence

The implementation and all pre-review C1-C24 obligations are green. The owner
ran no structured review during implementation or acceptance convergence.

| Gate | Result | Artifact |
| --- | --- | --- |
| Network terminal settlement | 9 passed | lane transcript; `/tmp/nnc65c-fail-before-network.out` retains the red |
| Full network library | 270 passed, one ignored subprocess entry | lane transcript |
| Server final withdrawal | 6 passed | `/tmp/nnc65c-final-server-final-withdrawal.out` |
| Server workload-ingress matrix | 18 passed | `/tmp/nnc65c-final-server-workload-ingress.out` |
| Server listener settlement | One passed | `/tmp/nnc65c-final-server-listener-settlement.out` |
| Full server library, serialized | 614 passed, 33 ignored | `/tmp/nnc65c-final-server-full-serialized.out` |
| Full node library, all features | 88 passed | `/tmp/nnc65c-final-node-all-features-after-linux-fix.out` |
| Systemd-focused node lane | 28 passed, 60 filtered | `/tmp/nnc65c-final-node-systemd.out` |
| Compute node substitutions | 4 passed, 341 filtered | `/tmp/nnc65c-compute-teardown-node.out` |
| Full compute library | 344 passed, one ignored subprocess entry | `/tmp/nnc65c-final-compute-full.out` |
| Affected check | Passed | `/tmp/nnc65c-final-affected-check.out` |
| Strict affected Clippy | Passed; only pre-existing vendored Brotli warnings | `/tmp/nnc65c-final-affected-clippy.out` |
| Warning-denied affected Rustdoc | Passed | `/tmp/nnc65c-final-affected-rustdoc.out` |
| Format and diff checks | Passed | owner terminal transcript |
| Network dependency tree | Only `nimbus-core` is a workspace edge | `/tmp/nnc65c-final-network-dependency-tree.out` |
| Network forbidden-effect scan | No executable effect; two `netavark` strings are identity/test data | `/tmp/nnc65c-final-network-forbidden-effect-scan.out` |
| Bind and composition censuses | Passed after line-only evidence refresh | `/tmp/nnc65c-final-bind-census.out`; `/tmp/nnc65c-final-composition-census.out` |
| NNCV035 mutation suite | 55 passed, zero failed | `/tmp/nnc65c-final-nncv035-self.out` |
| NNCV035 direct target | Expected nonzero; exact `0 passed, 7 failed` | `/tmp/nnc65c-final-nncv035-direct.out` |
| Aggregate target | Expected nonzero; exact `35 passed, 1 failed`, only NNCV035 | `/tmp/nnc65c-final-aggregate-verifier.out` |
| Strict proof lint | One file, zero diagnostics | `/tmp/nnc65c-proof-lint.json` |
| Documentation links and source map | 108 pages passed | `/tmp/nnc65c-final-docs.out` |
| Documentation site contract | 17 of 17 conditions passed | `/tmp/nnc65c-final-site.out` |

The default-parallel server diagnostic was not an acceptance run. It reported
564 passes and 50 `DuplicateProcessComposition` failures because independent
tests competed for the deliberate process-global network authority. The
required serialized run passed 614 of 614 executable tests. The established
real-process teardown crash-cut proof accounts for most of the 228.99-second
duration. It completed normally.

The node source audit also exercised Linux-only code. Native gates are green.
The full Linux cross-build remains environment-limited before Nimbus code.
The default toolchain lacks `aarch64-linux-gnu-gcc`. A Zig attempt reached
`libnghttp2` bindgen, but its incomplete sysroot did not provide `stdlib.h`.
Manual inspection found and corrected one real cfg-hidden zbus request-move
defect before native gates reran.

## Correction-Freeze Evidence

The sole full item review found four executable acceptance defects. Each defect
has a valid fail-before result, a narrow correction, and final affected proof.
The full item review did not repeat.

| Gate | Corrected result | Artifact |
| --- | --- | --- |
| Systemd restart-attempt key fail-before | Selected test failed with exit 101 because a later valid execution attempt collided with the old receipt | `/tmp/nnc65c-review-fail-before-systemd-attempt-key.out` |
| Systemd reconstructed-stage fail-before | Selected test failed with exit 101 because classified evidence was process-local | `/tmp/nnc65c-review-fail-before-systemd-reopen.out` |
| Systemd terminal/job-order fail-before | Selected test failed with exit 101 because terminal unit state preceded current-job classification | `/tmp/nnc65c-review-fail-before-systemd-terminal-job-order.out` |
| Ingress exact-membership fail-before | One selected test failed: a crossed same-cardinality endpoint set wrongly advanced to `Withdrawn` | `/tmp/nnc65c-review-f2-fail-before-endpoint-lease-membership.out` |
| Corrected Systemd receipt and teardown lane | 23 passed | `/tmp/nnc65c-review-node-systemd-focused.out` |
| Cross-process receipt-store lock | Four focused parent tests passed; the selected child acquired and released the real lock in another process | `/tmp/nnc65c-correction-node-store-subprocess.out` |
| Corrected server membership test | One passed | `/tmp/nnc65c-review-f2-endpoint-lease-membership.out` |
| Corrected server ingress matrix | 19 passed | `/tmp/nnc65c-review-f2-server-workload-ingress-matrix.out` |
| Corrected compute membership test | One passed | `/tmp/nnc65c-review-f2-compute-membership.out` |
| Full network behavior | 270 passed, one declared ignored subprocess entry | `/tmp/nnc65c-correction-network-full.out` |
| Full node behavior, all features | 104 passed | `/tmp/nnc65c-correction-node-full.out` |
| Full compute behavior | 345 passed, one declared ignored subprocess entry | `/tmp/nnc65c-correction-compute-full.out` |
| Full server behavior, serialized | Library 615 passed and 33 declared ignores; all binaries total 709 passed and 33 ignores | `/tmp/nnc65c-correction-server-full.out` |
| Affected all-target check | Passed | `/tmp/nnc65c-correction-affected-check.out` |
| Strict affected Clippy | Passed; only the repository's pre-existing vendored Brotli warnings appeared | `/tmp/nnc65c-correction-affected-clippy.out` |
| Post-subprocess node check and strict Clippy | Passed after the final test-only cross-process proof | `/tmp/nnc65c-correction-node-post-subprocess-check.out`; `/tmp/nnc65c-correction-node-post-subprocess-clippy.out` |
| Warning-denied affected Rustdoc | Passed | `/tmp/nnc65c-correction-affected-rustdoc.out` |
| Format and diff checks | Passed | owner terminal transcript |
| Network dependency tree | `nimbus-core` remains the only workspace edge | `/tmp/nnc65c-correction-network-dependency-tree.out` |
| Network effect scan | No transport or provider effect; matches are portable provider vocabulary and test identity data | `/tmp/nnc65c-correction-network-forbidden-effect-scan.out` |
| NNCV035 mutation suite | 55 passed, zero failed | `/tmp/nnc65c-correction-nncv035-self.out` |
| NNCV035 direct target | Expected nonzero; exact `0 passed, 7 failed` | `/tmp/nnc65c-correction-nncv035-direct.out` |
| Aggregate target | Expected nonzero; exact `35 passed, 1 failed`, only NNCV035 | `/tmp/nnc65c-correction-aggregate-verifier.out` |
| Strict proof lint | One file, zero diagnostics | `/tmp/nnc65c-correction-proof-lint.json` |
| Documentation links and source map | 108 pages passed | `/tmp/nnc65c-correction-docs.out` |
| Documentation site contract | 17 of 17 conditions passed | `/tmp/nnc65c-correction-site.out` |

The one authorized narrow correction review ran against staged tree
`5eafeecef1177daeec94ceba850d5e21fabed145`. Its complete patch SHA-256 was
`60db86bd3ecc69553f871b09905b7351542d94f2e334ece1347604bfd9bb8302` and its
executable patch SHA-256 was
`f6e8a0bdc7817bb09b381ff4977631b939747663b28a0e41629c2e0b728c73f5`.
The wrapper confirmed GPT-5.6 Sol, xhigh reasoning, and fast service tier. It
reported two P2 findings and one P3 finding at overall confidence 0.93. The
owner accepted and corrected all three. The review cadence is now exhausted.
The owner ran no third review, and the item does not need one.

| Narrow-review correction gate | Final result | Artifact |
| --- | --- | --- |
| Coherent Systemd lifecycle snapshot | `ActiveState`, `SubState`, and `Job` come from one `Properties.GetAll` reply; a second equal snapshot brackets activation-fence reads | `/tmp/nnc65c-narrow-zbus-api-probe.out` |
| Absorbing terminal receipts | One focused regression passed for both terminal success and terminal failure; a late submission update returns the retained result and cannot change durable stage | `/tmp/nnc65c-narrow-terminal-stage.out` |
| Deterministic cross-process exclusion | One parent and one actual child passed; the parent received `WouldBlock` while the child held the OS lock, then acquired it after child exit | `/tmp/nnc65c-narrow-cross-process-proof.out` |
| Full node behavior | 105 passed | `/tmp/nnc65c-narrow-node-full.out` |
| Full compute behavior | 345 passed, one declared ignored subprocess entry | `/tmp/nnc65c-narrow-compute-full.out` |
| Affected all-target check | Passed | `/tmp/nnc65c-narrow-affected-check.out` |
| Strict affected Clippy | Passed; only the repository's pre-existing vendored Brotli warnings appeared | `/tmp/nnc65c-narrow-affected-clippy.out` |
| Warning-denied affected Rustdoc | Passed | `/tmp/nnc65c-narrow-affected-rustdoc.out` |

The final ledger-only closeout recheck produced this evidence after all
accepted executable corrections:

| Closeout gate | Final result | Artifact |
| --- | --- | --- |
| Format and diff | Both passed with no output | `/tmp/nnc65c-closeout-format.out`; `/tmp/nnc65c-closeout-diff.out` |
| Network dependency and effect boundaries | `nimbus-core` is the only workspace edge; the token scan found portable provider vocabulary and test identity data but no executable provider effect | `/tmp/nnc65c-closeout-network-dependency-tree.out`; `/tmp/nnc65c-closeout-network-effect-scan.out` |
| NNCV035 mutation suite | 55 passed, zero failed | `/tmp/nnc65c-closeout-nncv035-self.out` |
| NNCV035 direct target | Expected exit 1; exact `0 passed, 7 failed` | `/tmp/nnc65c-closeout-nncv035-direct.out` |
| Aggregate target | Expected exit 1; exact `35 passed, 1 failed`, only NNCV035 | `/tmp/nnc65c-closeout-aggregate.out`; `/tmp/nnc65c-closeout-aggregate.status` |
| Strict proof lint | One file, zero diagnostics | `/tmp/nnc65c-closeout-proof-lint.json` |
| Documentation links and source map | 108 pages passed | `/tmp/nnc65c-closeout-docs.out` |
| Documentation site contract | 17 of 17 conditions passed | `/tmp/nnc65c-closeout-site.out` |

The node provider owns one strict receipt envelope with a version, checksum,
and exact operation key. Independent processes serialize through an `fs2`
lock. Each replacement writes and synchronizes a same-directory temporary
file, renames it, and synchronizes the parent directory. Corrupt or unknown
formats fail closed.

Store failure before `StopUnit` makes zero provider
effects. Failure to persist evidence after a possible provider effect returns
ambiguity. Exact teardown fails closed when the backend uses the coarse
constructor without a durable state root.

No product caller or composition root changes in this item.

The server carries the exact compiled plan from authenticated durable intent
inside the private confirmed command. Fresh-process recovery compares the
complete endpoint set, listener IDs, lease IDs, listener owners, tenant, plan,
generation, and accounting before mutation. It reuses that authenticated
durable snapshot for recovery, so no second list can cross the witness. The
server does not own a competing lease authority.

All changed production roots remain below 1,500 lines. The only larger changed
test matrix is `workload_ingress/tests.rs` at 2,439 lines. It is a deliberate
test-only concept exception for the complete local workload-ingress lifecycle.
One process-global guard and one durable runtime-history fixture share the real
network authority. The initial, restart, observation, final-withdrawal,
route-worker, and listener-lease proofs use that authority. The matrix contains
no production logic or generic fixture.

NNC6.5c adds no more behavior to this
matrix after closeout. Future growth must first move the runtime-history and
fresh-process group to a concept-owned child. The move must not duplicate the
guard or network authority.

## Result Target

NNC6.5c supplies three real provider substitutions for the compute-owned
teardown driver:

- the server withdraws final workload ingress.
- the DirectProcess node provider drains and stops one exact execution.
- the Systemd node provider drains and stops one exact execution.

The item also adds the missing transport-free lease transition that lets the
server atomically settle only the process-bound listener members of a larger
network plan. Socket, worker, D-Bus, proxy, and provider effects stay in their
current owner crates.

This item does not install the capabilities in product composition and does
not cut over a caller. NNC6.5e and NNC6.5f own those changes. NNC6.5g removes
the coarse legacy teardown paths after all callers use the confirmed saga.

## Current And Target Ownership

Current final ingress cleanup has no authoritative result:

```text
RunningIngressRoute::Drop
  -> signal listener worker
  -> join listener worker
  -> scalar listener-lease settlement
  -> log join or lease failure
  -> caller receives no exact absence proof
```

Current host stop uses a coarse execution ID:

```text
NodeWorkloadReconciler
  -> HostLifecycleBackend::stop(execution_id)
     -> DirectProcess in-memory state change
     -> or Systemd StopUnit plus JobRemoved wait
  -> coarse HostLifecycleStatus
```

The NNC6.5c target keeps each authority narrow:

```text
compute confirmed teardown command
  |
  +-> server FinalIngressWithdrawalCapability
  |    -> exact publication validation
  |    -> withdraw selected process-bound listener leases atomically
  |    -> cancel and join exact route and connection workers
  |    -> close exact listeners
  |    -> release selected listener leases atomically
  |    -> exact PublicationAbsent observation
  |
  +-> compute NodeExecutionTeardownAdapter
       -> node-owned exact drain or stop claim
       -> DirectProcess or Systemd small capability
       -> typed provider-local observation
       -> exact ExecutionDrained or ExecutionStopped observation

nimbus-network
  -> complete-plan authentication
  -> selected process-bound member transitions
  -> durable lease state only
```

Compute remains the only phase coordinator. The server owns sockets and
ingress workers. The node owns process and Systemd effects. The network crate
owns lease authority only.

## Read-Only Source Audit

The audit ran from clean item checkpoint
`3976a8b6c475bd60dcee347ba39182f3944a701d`.

| Finding | Source evidence | NNC6.5c action |
| --- | --- | --- |
| Final ingress cleanup is best effort. | `RunningIngressRoute::stop_and_settle` and `Drop` log join and settlement failures. | Add an explicit result-bearing final-withdrawal state machine. Keep `Drop` only as fail-closed cleanup. |
| Restart withdrawal has the correct worker ordering. | `listener_lease/restart_retain.rs` signals and joins every route before one atomic retain-for-rebind transition. | Reuse the ordering pattern. Keep terminal release distinct from restart retention. |
| A live batch lacks complete final-publication identity. | `RunningIngressBatch` retains the plan and route set but not the full `WorkloadPublicationReference`. The restart command can reconstruct after process loss but did not expose the target reference. | Retain one authenticated publication reference for initial and restart publication. Reconstruct the restart target in the compute command from durable intent, endpoint IDs, and exact target execution. Do not derive workload identity from IP or port. |
| The portable final command has the required fence. | `ConfirmedWorkloadTeardownCommand` retains the saga, revision, transition, generation, desired digest, node, source, plan digest, attempt, epoch, provider, subjects, step, and mode. | Validate the complete command before a route, worker, lease, state, or D-Bus effect. |
| Scalar lease settlement cannot settle a planned listener member. | `withdraw` and `release_with_lifetime` call scalar-plan authentication. That authentication rejects one member of a multi-member plan. | Add complete-plan plus exact-subset process-bound terminal transitions in `nimbus-network`. |
| Existing batch terminal APIs have the wrong scope. | Complete batch APIs require all plan members. Provider-managed APIs reject process-bound listener lifetimes. Restart subset APIs retain the port instead of releasing it. | Add one concept-owned process-bound terminal-settlement child. Do not reuse a crossed scope. |
| Crash recovery omits pre-close withdrawal. | `recover_dead_plan_members` accepts `Active` and `CleanupPending`, but not `Withdrawing`. | Admit exact process-bound `Withdrawing` records under dead-lifetime authority and settle the selected subset atomically. |
| DirectProcess stop is too coarse. | It authenticates only `WorkloadExecutionId`, changes process-local state, and appends a duplicate stop log on replay. | Add exact command-bound drain and stop state. Record one terminal stop effect. |
| DirectProcess is a process-local simulation. | It owns a `BTreeMap` and generated numeric process IDs. It does not own an OS process. | Never claim fresh-instance absence unless a tested process-lifetime authority proves that the prior in-memory effect cannot still exist. Otherwise report ambiguity. |
| The host lifecycle trait is already broad. | `HostLifecycleBackend` owns validation, coarse stop, inspection, activation, and restart. `host_lifecycle.rs` has 1,478 lines. | Add separate drain and stop ports in `host_lifecycle/teardown.rs`. Do not add teardown methods to the broad trait. |
| Systemd inspection cannot see an in-flight job. | It reads unit state, PID, and activation fence. `zbus_systemd::UnitProxy::job()` exists but the local status omits it. | Add typed current-job and stop-submission evidence. Never report `NotCompleted` while an older job can finish. |
| The compute child needs one root registration. | `workload_saga.rs` does not declare `teardown_node`. | Permit a registration-only edit to that root. No new dependency is required. |
| Product callers still use coarse paths. | Server composition installs provision and restart capabilities only. Node reconciliation calls the coarse host lifecycle trait. | Keep those callers unchanged. Later items own cutover and deletion. |

## Binding Design Decisions

1. **One complete command fence.** Each adapter authenticates the confirmed
   command and its exact subject. It does this before any effect or local
   mutation.
   A stale, crossed, incomplete, or wrong-mode command makes zero changes.
2. **One lease authority.** The network-owned port authority changes every
   selected-listener lifecycle state. The server does not write a second
   durable lease journal or settle scalar requests in sequence.
3. **Atomic selected-listener settlement.** A caller supplies the complete
   immutable plan witness and one exact non-empty listener subset. The
   authority validates every request, binding, lifetime, scope, phase, and
   generation before it changes any member. An error preserves all bytes.
4. **Process-bound only.** The new transition accepts only process-bound
   lifetimes. It cannot settle ProviderManaged PEP, sandbox, or machine
   effects. It does not add socket or provider I/O to `nimbus-network`.
5. **Terminal release differs from restart retention.** Final withdrawal
   releases the numeric slot after confirmed absence. Restart withdrawal
   retains the slot for an exact rebind. Neither operation calls the other.
6. **Result-bearing worker ownership.** Final withdrawal signals and joins all
   listener workers and their transitive connection workers. A panic or join
   failure cannot become success. The port stays fenced when join proof is
   incomplete.
7. **Publication identity is stable and tenant qualified.** The live batch
   retains the complete authenticated `WorkloadPublicationReference`. The
   restart command reconstructs the target reference from durable saga state.
   The final command and TenantPublished durable listener subset provide
   teardown recovery authority. IP addresses and numeric ports are
   observations only.
8. **Execute and inspect share one exact operation state.** They do not race
   through separate maps or locks. This rule also covers replay and crossed
   requests. `NotCompleted` requires proof that no exact older operation can
   commit.
9. **Inspection starts no mutating provider effect.** Inspection can query
   provider state. It can reconcile local durable evidence after exact absence
   or dead-owner proof. It cannot bind or close a socket, signal a live worker,
   submit `StopUnit`, stop a process, or restart work.
10. **Failure retains recovery evidence.** Failure before durable withdrawal
    leaves the live batch unchanged. Failure after withdrawal keeps exact
    `Withdrawing`, cleanup, job, or operation evidence and blocks phase
    progress. No branch fabricates absence.
11. **Drain has an explicit scope.** Drain closes Nimbus-admitted request work.
    It makes no claim about arbitrary application background work. Final
    ingress provides the admission barrier. The node claim authenticates the
    exact `Withdrawn -> Drained` command and records or rederives the barrier.
    It submits no stop request. If a source census finds another admitted
    request path, implementation stops until that path has a real drain
    handshake.
12. **Node claims are neutral.** `nimbus-node` does not depend on compute or
    network. It accepts a node-owned claim with the complete portable teardown
    claim and command identity. The claim also contains the confirmed
    transition, source, exact execution, provider target, and mode.
13. **Compute only lowers and translates.** One compute adapter converts a
    private confirmed command into a node claim. It converts a typed node
    observation into the exact portable provider outcome. It does not select
    policy, infer a provider ID, or call a coarse lifecycle method.
14. **DirectProcess stays honest.** Same-authority drain and stop synchronize
    under one record lock. Replay records one stop. The adapter treats a
    missing record as satisfied only with tested process-lifetime evidence.
    That evidence must exclude another live authority. A separate same-process
    backend with no record is ambiguous.
15. **Systemd tracks submission ambiguity durably.** A node-owned receipt store
    owns drain and stop evidence. Its key contains the execution ID,
    execution-attempt ID, and teardown-attempt ID. It persists pre-call,
    unknown-submission, accepted-job, terminal-success, and terminal-failure
    stages. It writes each stage before it returns classified evidence. It
    inspects the exact unit, activation fence, and current job before a retry.
16. **Current jobs fence retries.** An exact active stop job means progress. A
    different job means ambiguity. Active state with no current job can mean
    `NotCompleted` only with exact proof. The proof must exclude an older stop
    request that can still commit.
17. **Provider IDs come from admission.** DirectProcess and Systemd are node
    mechanisms. The compute adapter receives the admitted
    `WorkloadExecutionProviderId`. It does not invent mechanism-named provider
    IDs.
18. **No caller cutover.** This item proves real capability substitution only.
    It does not edit `ComputeState`, `NodeWorkloadReconciler`, server
    composition, guest APIs, Compose, sandbox providers, or machine providers.
19. **No compatibility layer.** The exact seams coexist with coarse callers
    until NNC6.5e through NNC6.5g replace and delete them. This item adds no
    adapter from a coarse command to an exact command.
20. **Canonical item review cadence.** Focused tests, affected suites, static
    checks, and manual inspection drive implementation. One full
    GPT-5.6 Sol/xhigh/fast review runs only after C1-C24 are green and the item
    is candidate-frozen. An accepted executable defect permits one narrow
    correction review after the affected proofs pass.

## Network Terminal-Settlement Contract

The network child owns three operations. Final names can change only when the
same concepts and checks remain explicit.

| Operation | Required input | Atomic result |
| --- | --- | --- |
| Withdraw live selected members | Complete plan, exact request/binding subset, exact live lifetime guards | Every selected `Active` member becomes `Withdrawing`; unrelated plan members remain byte identical. Exact replay stays `Withdrawing`. |
| Release after confirmed local stop | Complete plan, exact request/binding subset, exact live lifetime guards | Every selected `Withdrawing` member becomes `Released`; live lifetime authority clears. Unrelated members remain byte identical. |
| Release after owner death | Complete plan, exact request subset, exact recovery guards | Exact process-bound `Active`, `Withdrawing`, or cleanup-pending members become `Released` after dead-lifetime proof. Terminal replay is idempotent. |

The implementation must validate the full batch before mutation. Duplicate
lease IDs, incomplete witnesses, mixed generations, and crossed bindings fail
without a write. Wrong lifetimes, wrong effect scopes, mixed replay states,
and unrelated plan members also fail without a write.

## Final Ingress State Machine

| Exact state | Execute result | Inspect result |
| --- | --- | --- |
| Live batch, no withdrawal | Authenticate, persist selected lease withdrawal, then stop workers. | `NotCompleted` only when no older exact operation exists. |
| Withdrawal persisted, workers live | Signal and join every exact worker. | `InProgress` or ambiguity; inspection does not signal workers. |
| Workers joined, listener release pending | Atomically release the selected listener subset. | Reconcile local durable authority only when exact absence is already proven. |
| Every exact listener released and no live route | Return exact `PublicationAbsent`. | Return the same exact satisfied evidence. |
| Settlement or join failure | Return ambiguity and retain all fences. | Return progress or ambiguity until proof becomes conclusive. |
| Adapter restarted, owner lifetime dead | Recover the exact selected process-bound subset and release it atomically. | The same local durable reconciliation is allowed; no socket or worker effect exists to issue. |
| Matching lifetime is live in another authority | Return ambiguity. | Return ambiguity; never claim absence. |
| Crossed command, publication, plan, generation, endpoint set, or provider source | Return definite failure with zero effects. | Return definite failure with zero effects. |

The initial and restart publication paths retain the same full publication
identity. Final withdrawal never calls sandbox inspection, sandbox restart,
logical name resolution, PEP forwarding, or certificate code.

## Node Provider Outcome Contract

| Provider state | Drain observation | Stop observation | Retry rule |
| --- | --- | --- | --- |
| Crossed claim, source, provider, mode, or activation fence | Definite failure | Definite failure | Zero provider effects and zero local mutation. |
| Exact absent effect with conclusive authority evidence | Satisfied | Satisfied | Absence already meets the objective. |
| Exact running DirectProcess record | Exact admission-drain barrier | One idempotent stopped transition | Replay adds no second stop log or state change. |
| Exact stopped DirectProcess record | Satisfied | Satisfied | Return the retained canonical evidence. |
| Missing record in an unrelated live DirectProcess authority | Ambiguous | Ambiguous | Do not infer absence from an empty map. |
| Exact inactive or failed Systemd unit | Satisfied | Satisfied | Physical execution is terminal. |
| Exact active Systemd unit with no current or possible older job | Satisfied drain barrier | `NotCompleted` | Only this proof can authorize a next-epoch stop. |
| Exact current Systemd stop job | In progress | In progress | Inspect the same job; never resubmit. |
| Different current Systemd job | Ambiguous | Ambiguous | Do not compete with provider state. |
| Unknown `StopUnit` submission stage | Ambiguous | Ambiguous | Never report `NotCompleted`. |
| Accepted job with lost `JobRemoved` result | Inspect exact unit and job | Inspect exact unit and job | Adopt terminal state or retain progress/ambiguity. |
| D-Bus read failure | Ambiguous | Ambiguous | No phase progress. |

Systemd current-job evidence contains the job ID, object path, type, and state.
The stop path authenticates the retained activation fence before its one
`StopUnit` call.

## Owned And Forbidden Paths

The item owns only these product concepts:

- `crates/nimbus-network/src/port_lease.rs` for child registration and narrow
  exports.
- `crates/nimbus-network/src/port_lease/lifetime.rs` only for the existing
  dead-plan-member recovery phase predicate.
- `crates/nimbus-network/src/port_lease/terminal_settlement.rs` and its tests.
- `crates/nimbus-server/src/workload_ingress.rs` and the
  `workload_ingress/final_withdrawal.rs` concept child and tests.
- `crates/nimbus-server/src/workload_ingress/route_workers.rs` and focused
  tests for route-owned listener and transitive connection worker joins.
- `crates/nimbus-server/src/listener_lease.rs` and the
  `listener_lease/terminal_settlement.rs` concept child and tests.
- server `lib.rs` only if a narrow existing-adapter export requires it.
- `crates/nimbus-node/src/host_lifecycle.rs` and
  `host_lifecycle/teardown.rs` for node-owned claims and ports.
- `crates/nimbus-node/src/host_lifecycle/activation_fence.rs` only for the
  read-only teardown variant dispatcher.
- `crates/nimbus-node/src/host_lifecycle/restart.rs` only for the private
  restart-activation-fence teardown matcher.
- `crates/nimbus-node/src/direct_process.rs` and a concept child for exact
  drain and stop state.
- `crates/nimbus-node/src/systemd_transient.rs`, its teardown child, and the
  existing zbus client files for typed job and submission evidence.
- `crates/nimbus-node/src/systemd_transient/teardown_store.rs` and tests for
  the accepted crash-safe, exact-attempt Systemd receipt authority.
- `crates/nimbus-node/Cargo.toml` only to promote the existing workspace
  `fs2` and `serde_json` dependencies from test/transitive use to the node
  provider's production receipt store. This adds no workspace crate edge.
- `Cargo.lock` only for the required one-line `fs2` package-dependency result
  of that accepted manifest change.
- node `lib.rs` for narrow exports.
- `crates/nimbus-compute/src/workload_saga.rs` for registration only.
- `crates/nimbus-compute/src/workload_saga/teardown_node.rs` and its tests for
  command lowering and outcome translation.
- `crates/nimbus-compute/src/workload_saga/teardown_command.rs` and its tests
  only for the accepted command-carried exact compiled endpoint, listener,
  and lease membership needed by fresh-process ingress recovery.
- `crates/nimbus-compute/src/workload_saga/restart_dispatch.rs` and its tests
  only for the durable target-publication reference on restart commands.
- `docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json`
  and `nnc4.6f-production-network-authority-census.json` only for mechanical
  source-line refresh after owned modules moved. Their classifications,
  occurrence identities, summaries, and authority decisions cannot change.
- this proof and the owning plan ledger.

Except for the exact `nimbus-node` dependency promotion above, the item
forbids Cargo-manifest edits. It forbids `ComputeState` and product
composition edits. It also forbids edits to `NodeWorkloadReconciler`, sandbox,
services, tenant, machine, proxy, egress, system projection, logical naming,
cluster transport, and product callers. A finding outside the owned concepts
routes to its later owner unless it blocks a C1-C24 criterion.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| C1 | The network terminal-settlement child authenticates a complete immutable plan and atomically withdraws and releases one exact non-empty process-bound listener subset. Duplicate, incomplete, crossed, stale, mixed-scope, mixed-replay, and wrong-lifetime inputs preserve all durable bytes. Unrelated plan members stay byte identical. |
| C2 | `cargo tree -p nimbus-network --edges normal` retains only the approved `nimbus-core` workspace edge. Network source contains no socket, worker, D-Bus, Axum, Pingora, Netavark, nftables, gvproxy, Iroh, cloud SDK, tenant policy, service naming, proxy forwarding, or provider effect. |
| C3 | The server adapter authenticates the complete confirmed command, exact ingress provider selection, full publication reference, endpoint set, execution reference, plan identity, generation, desired and source digests, node, attempt, epoch, step, subject, and mode before effects. |
| C4 | Initial and restart publication retain the same full authenticated publication identity with the live batch. A fresh process reconstructs the restart target reference from the durable active intent, compiled endpoint IDs, and exact target execution. Crossed same-saga generations, attempts, plans, endpoint sets, or provider evidence select no route and make zero effects. |
| C5 | Final execute persists selected listener withdrawal, cancels every exact route, joins every listener and transitive connection worker, closes exact listener ownership, and atomically releases every selected listener lease before success. The old addresses can bind again only after release. |
| C6 | A worker panic, join failure, close ambiguity, or lease settlement failure returns ambiguity, retains exact durable recovery evidence, and blocks phase progress. The adapter attempts all sibling stop and join operations before it reports the aggregate failure. |
| C7 | Execute, inspect, and replay synchronize on one exact operation state. Inspection starts no mutating provider effect. It can read provider state and reconcile local durable authority only after exact absence or dead-owner proof. It never reports `NotCompleted` while an older exact operation can commit. |
| C8 | Process restart after durable withdrawal, worker close, or partial settlement converges through exact dead-lifetime recovery. It does not rebind a listener, start a worker, duplicate a route, touch a PEP sibling, or release a live other-owner lifetime. |
| C9 | Final release and restart retain-for-rebind remain distinct code paths and state transitions. Existing restart behavior and tests stay green. `Drop` remains best-effort cleanup and cannot produce capability success. |
| C10 | The server adapter implements the real `FinalIngressWithdrawalCapability`. A real registry and compute teardown runtime advance one exact withdrawal command with that adapter. No product composition or caller changes. |
| C11 | The node-owned claim validates the complete portable teardown claim, command ID, confirmation revision and transition, source, exact execution, provider target, subject, step, and mode. Execute and inspect use distinct authority. Crossed inputs make zero effects. |
| C12 | Node exposes separate small drain and stop ports. `HostLifecycleBackend` gains no teardown methods. Compute owns one adapter that lowers a private confirmed command and translates typed node evidence without policy selection or provider-ID inference. |
| C13 | DirectProcess drain proves only the exact Nimbus admission barrier and leaves the running state unchanged. A claim outside the exact `Withdrawn -> Drained` transition fails before state inspection or mutation. |
| C14 | DirectProcess stop authenticates the exact retained activation and teardown fences. Exact replay records one stopped transition and one stop log. Crossed or stale commands preserve state and logs. Concurrent execute and inspect cannot produce a false `NotCompleted`. |
| C15 | A missing DirectProcess record becomes satisfied only under tested process-lifetime single-authority evidence. A separate live authority with an empty map reports ambiguity. Tests do not describe generated numeric IDs as OS-process termination evidence. |
| C16 | Systemd drain performs no `StopUnit` call and returns exact admission-barrier evidence only for the correct drain transition and retained activation fence. |
| C17 | Systemd stop authenticates the exact unit and activation fence before exactly one `StopUnit` call. It retains typed pre-submission, unknown-submission, accepted-job, job-wait, and terminal evidence. |
| C18 | Systemd inspection reads the exact current unit job. A stop job reports progress, another job reports ambiguity, and an unknown submission window never reports `NotCompleted`. A lost response adopts exact terminal state without a duplicate stop effect. |
| C19 | Real substitution tests use a concrete activated `DirectProcessBackend`, a concrete `SystemdTransientUnitBackend<FakeSystemdDbusClient>`, the real compute node adapter, the real drain and stop trait objects, and commands created through the confirmed-command test seam. |
| C20 | Provider observations return canonical exact `ExecutionDrained` or `ExecutionStopped` evidence with the complete callback fence. No observation uses IP, PID, port, unit name, or provider handle as workload identity. |
| C21 | Coarse node lifecycle methods and callers remain unchanged for NNC6.5e through NNC6.5g. No compatibility adapter converts a coarse call into an exact teardown command. |
| C22 | Production composition roots stay thin. New state machines live in concept-owned children. Every handwritten production file stays below 1,500 lines or records a specific concept-owned exception before closeout. |
| C23 | Focused and full affected tests, format, strict affected Clippy, warning-denied Rustdoc, dependency/effect scans, proof lint, docs, and site gates pass. The proof records exact counts, skips, commands, and artifacts. |
| C24 | NNCV035 self-test remains exact `55/55`. Its direct wrapper becomes exact `0 passed, 7 failed` because only `ingress` clears; `service`, `definition-delete`, `compose`, `machine`, `tenant`, `compensation`, and `behavior` remain. The aggregate stays `35/36` with NNCV035 as the sole expected red. One candidate-frozen Sol/xhigh/fast review runs after C1-C24 pass. |

## Fail-Before Roster

The first implementation checkpoint adds these tests before their product
seams. Each selected command must exit nonzero because the named API or module
does not exist. Save complete output under `/tmp/nnc65c-fail-before-*.out`.

1. `process_bound_terminal_subset_is_atomic_and_preserves_plan_siblings`
2. `real_server_ingress_adapter_substitutes_for_final_withdrawal_capability`
3. `final_withdrawal_closes_routes_joins_workers_and_releases_exact_leases`
4. `final_withdrawal_settlement_failure_blocks_progress_and_preserves_fences`
5. `final_withdrawal_recovers_dead_process_bound_listeners_without_rebind`
6. `host_teardown_claim_binds_complete_confirmed_command_fence`
7. `drain_claim_requires_exact_withdrawn_to_drained_transition`
8. `direct_process_exact_stop_replay_records_one_terminal_effect`
9. `direct_process_fresh_authority_missing_state_is_not_false_absence`
10. `systemd_crossed_teardown_fails_before_stop_unit`
11. `systemd_pending_stop_job_is_never_not_completed`
12. `systemd_lost_stop_response_converges_without_duplicate_effect`
13. `node_teardown_adapters_substitute_direct_process_and_systemd`
14. `confirmed_restart_command_is_private_and_complete` gains the exact target
    publication-reference assertion.

The proof records the exact red count after the tests compile far enough to
reach the missing seam. A missing test target, unrelated compile error, hung
wait, or command hidden by a pipeline does not count.

The restart-reference audit first tested a workloads constructor that had no
durable source-publication input. The owner rejected that seam because it
could not prove cross-plan correlation after process loss. Artifact
`/tmp/nnc65c-fail-before-workloads-publication.out` records the rejected
experiment. The final fail-before artifact is
`/tmp/nnc65c-fail-before-restart-publication-reference.out`. It proves that the
durably reconstructed restart command lacked the target reference.

The owner introduced the roster in dependency-safe compile slices. Every command
preserved its real nonzero exit status.

| Slice | Exact red | Artifact |
| --- | --- | --- |
| Network terminal subset | Two missing-method errors | `/tmp/nnc65c-fail-before-network.out` |
| Server final withdrawal | Five missing-seam errors | `/tmp/nnc65c-fail-before-server.out` |
| Node claim | One unresolved-import error | `/tmp/nnc65c-fail-before-node-host.out` |
| DirectProcess | Four missing-method errors | `/tmp/nnc65c-fail-before-node-direct-process.out` |
| Systemd | Four missing-method errors | `/tmp/nnc65c-fail-before-node-systemd.out` |
| Compute substitution | One missing-type error | `/tmp/nnc65c-fail-before-compute.out` |
| Restart target publication | One missing-method error | `/tmp/nnc65c-fail-before-restart-publication-reference.out` |
| Crossed confirmation adoption | One selected test failed, zero passed | `/tmp/nnc65c-fail-before-node-crossed-confirmation.out` |

Manual acceptance inspection found the crossed-confirmation red. It proved
that an internally consistent but different confirmation could adopt a prior
provider result. The corrected provider operation fence now retains the exact
execute command. It binds its one next-revision inspect command and advances
only for exact `RetryAfterNotCompleted` evidence at the next dispatch epoch.

## Acceptance Findings And Dispositions

| Finding | Disposition and proof |
| --- | --- |
| Provider terminal state could adopt a crossed confirmation. | Accepted. DirectProcess and Systemd authenticate the retained operation fence before terminal replay. The selected fail-before above is green in the 88-test node suite. |
| The first node fence required equal execute and inspect transition IDs. | Accepted. Inspect now binds the next confirmed revision and its distinct transition ID. Reducer-realistic node tests prove the sequence. |
| A Systemd pre-call retry reused the old dispatch epoch. | Accepted. Only exact `NotCompleted` evidence authorizes a next-epoch execute. The real compute runtime test makes two classified submissions and one `StopUnit` effect. |
| Systemd terminal failure lost accepted job evidence. | Accepted. The retained failure digest includes exact job path and result; distinct results produce distinct evidence and no duplicate effect. |
| Same-process final-ingress inspection could stall after post-join ambiguity. | Accepted. A retained `Withdrawing` batch reconciles only after its exact lifetime owner is dead; a live other owner stays ambiguous and byte identical. |
| A lost route owner could prevent intact sibling stop attempts. | Accepted. Final route closure attempts every intact sibling and keeps the complete selected lease set fenced on aggregate failure. |
| Linux-only zbus inspection consumed the request before retaining its execution ID. | Accepted. The live adapter retains the ID first; native check, test, Clippy, and Rustdoc gates reran green. Linux cross-compilation remains environment-limited as recorded above. |
| NNCV035 still reported ingress after behavior was green. | Accepted. Actual server seams now carry the canonical exact execute, inspect, cancel/join, route-close, lease-settlement, absence-proof, and failure-propagation names. No dummy marker or verifier-script change was added. |
| Line movement made the bind and composition censuses stale. | Accepted as proof-only cleanup. Only source line fields changed in the existing census JSON files; classifications, occurrences, authority, and summaries did not change. NNCV006 and NNCV015 pass. |
| The Systemd operation maps use the restart-stable execution ID. | Accepted and corrected from the sole full item review. Durable drain and stop receipts use the execution ID, execution-attempt ID, and teardown-attempt ID. Valid later attempts cannot adopt or collide with older receipts. Fail-before: `/tmp/nnc65c-review-fail-before-systemd-attempt-key.out`; corrected lane: `/tmp/nnc65c-review-node-systemd-focused.out`. |
| Fresh ingress recovery authenticates endpoint cardinality but not exact endpoint-to-listener-to-lease membership. | Accepted and corrected from the sole full item review. The confirmed command retains the exact compiled-plan membership derived from durable intent. Server recovery authenticates the exact endpoint, listener, owner, and lease sets before recovery or mutation and reuses one authenticated snapshot. Fail-before: `/tmp/nnc65c-review-f2-fail-before-endpoint-lease-membership.out`; corrected matrix: `/tmp/nnc65c-review-f2-server-workload-ingress-matrix.out`. |
| Classified Systemd submission stages exist only in process memory. | Accepted and corrected from the sole full item review. The node provider uses a crash-safe, cross-process-locked, exact-attempt receipt store. It persists prepared, pre-call, unknown, accepted-job, terminal, and terminal-failure stages before returning classified evidence. Fail-before: `/tmp/nnc65c-review-fail-before-systemd-reopen.out`; corrected lane: `/tmp/nnc65c-review-node-systemd-focused.out`. |
| Systemd terminal unit state is accepted before a current job is classified. | Accepted and corrected from the sole full item review. Job-first classification covers drain inspection, stop execute and inspect, post-submission reconciliation, and terminal-response adoption. A stop job is progress; any other job is ambiguity; terminal success requires no current job. Fail-before: `/tmp/nnc65c-review-fail-before-systemd-terminal-job-order.out`; corrected lane: `/tmp/nnc65c-review-node-systemd-focused.out`. |
| Live Systemd inspection reads lifecycle state and current job through separate D-Bus replies. | Accepted from the sole narrow correction review. The live adapter now reads `ActiveState`, `SubState`, and `Job` in one `Properties.GetAll` reply and requires an equal second snapshot around activation-fence reads. A transition becomes ambiguity instead of false terminal success. The exact API shape compiles in `/tmp/nnc65c-narrow-zbus-api-probe.out`; the Linux workspace cross-build remains environment-limited before Nimbus code as recorded above. |
| A late StopUnit submitter can replace a terminal receipt written by concurrent inspection. | Accepted from the sole narrow correction review. Submission result transitions now require `Submitting`; terminal success and terminal failure are absorbing and return their retained exact observation. One deterministic regression proves both terminal variants and exact replay in `/tmp/nnc65c-narrow-terminal-stage.out`. |
| The cross-process lock proof depends on a 100-millisecond scheduling window. | Accepted from the sole narrow correction review. The parent now performs a deterministic nonblocking OS-lock attempt after the child publishes readiness from inside its transaction. It requires `WouldBlock`, releases the child, and then proves acquisition succeeds. Artifact: `/tmp/nnc65c-narrow-cross-process-proof.out`. |

The accepted receipt-store correction promotes two existing third-party
dependencies in `nimbus-node`. It adds no workspace crate edge. No finding
adds product composition, caller cutover, a coarse compatibility adapter, or
a provider effect to `nimbus-network`.

## Verification Commands

Focused behavior:

```text
cargo test -p nimbus-network port_lease::terminal_settlement
cargo test -p nimbus-server workload_ingress::final_withdrawal
cargo test -p nimbus-server listener_lease::terminal_settlement
cargo test -p nimbus-node host_teardown_claim
cargo test -p nimbus-node direct_process_exact
cargo test -p nimbus-node systemd_pending_stop_job
cargo test -p nimbus-node --features systemd-dbus-test-bus systemd_transient
cargo test -p nimbus-compute teardown_node
```

Full affected behavior and quality:

```text
cargo test -p nimbus-network
cargo test -p nimbus-server --lib
cargo test -p nimbus-node --all-features
cargo test -p nimbus-compute
cargo check -p nimbus-network -p nimbus-server -p nimbus-node -p nimbus-compute --all-targets --all-features
cargo clippy -p nimbus-network -p nimbus-server -p nimbus-node -p nimbus-compute --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc -p nimbus-network -p nimbus-server -p nimbus-node -p nimbus-compute --no-deps --all-features
cargo fmt --all --check
git diff --check
```

Static architecture and plan truth:

```text
cargo tree -p nimbus-network --edges normal
rg -n 'TcpListener|TcpStream|UdpSocket|StopUnit|zbus|axum|pingora|netavark|nft|gvproxy|iroh|TeardownProvider|NetworkProvider' crates/nimbus-network/src -g '*.rs'
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --self-test
bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --check
bash scripts/verify-nimbus-network-control-plane.sh
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

The expected NNCV035 direct command remains nonzero at NNC6.5c closeout. Its
exact summary is `0 passed, 7 failed`. This wrapper counts unresolved
diagnostics and does not count a cleared diagnostic as a pass. The aggregate
must report `35 passed, 1 failed` with NNCV035 as the only failure.

## Review And Commit Gate

The sole full item review ran against staged tree
`fb9a8e5a8a042aba84e66448931cae7831ff0e03`. The complete patch SHA-256 was
`d637ae538e21b0c1aa09b8e1c932e4b36056a818325c101ae1f93dd76ad59d7c`.
The executable patch SHA-256 was
`f872c77c955dcfacc598b7b9327a6fe19cc70e90aaea5066401a849315e1dd4e`.
The wrapper confirmed GPT-5.6 Sol, xhigh reasoning, fast service tier, one
pass, a 412,059-byte bundle, and thread
`019fe730-6430-77d1-be9a-8dc2251ef133`. It reported one P1 and three P2
findings at overall confidence 0.98.

Source review and three independent
read-only audits accepted all four findings. The table above owns their
dispositions. This full review must not run again.

Do not run structured autoreview during fail-before, implementation, cleanup,
or acceptance convergence. Run one full review only after every C1-C24 row is
green and the complete item diff is candidate-frozen. Use GPT-5.6 Sol, xhigh
reasoning, and fast mode. Accept a review only when the actual reviewer is Sol.

If the review finds an accepted executable defect, add or confirm its
fail-before proof. Correct the defect and rerun the affected acceptance gates.
The one authorized narrow correction review ran against staged tree
`5eafeecef1177daeec94ceba850d5e21fabed145`, complete patch SHA-256
`60db86bd3ecc69553f871b09905b7351542d94f2e334ece1347604bfd9bb8302`, and
executable patch SHA-256
`f6e8a0bdc7817bb09b381ff4977631b939747663b28a0e41629c2e0b728c73f5`.
It reported the three accepted findings recorded above. Their corrections pass
focused regressions, full node and compute behavior, affected check, strict
Clippy, Rustdoc with `-D warnings`, format, and diff checks.

The review cadence is now exhausted. Documentation, ledger, and final identity
updates do not authorize another review.

Close NNC6.5c with one exact owned commit that contains the code, tests, proof,
and ledger checkpoint. Do not push or open a pull request.
