# NNC6.5e native source retirement cutover

Status: `complete; K1-K32 green; review cadence exhausted`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

Durable start checkpoint: `6b685627c94ea53705019778b108e47f252fd08a`

This record owns native service stop, standalone sandbox stop, and dynamic
service-definition deletion. It is one acceptance-bearing implementation item
and one structured-review unit. It does not own Compose, forwarded-machine
composition, physical-machine stop, tenant retirement, failed-provision
compensation, cleanup finalization, resolver/cache fencing, or coarse-stop
deletion.

## Audit result

The five exact teardown provider capabilities and the compute driver exist,
but no product `ComputeState` composes or retains a teardown runtime. Native
callers still delegate to effectful `ServiceManager` retirement methods. A
definition-mutation guard serializes local definition edits, but provision does
not observe it and request cancellation releases it even after an ambiguous
durable submission.

Implementation requires four prospective-owner corrections:

1. `ComputeState` needs one optional, composition-injected teardown runtime.
   NNC6.5e owns only its composition field, constructor, and accessor in
   `state.rs`. NNC6.5g still owns tenant deletion and legacy convergence there.
2. Native HTTP adapters and the local Krun server composition must pass exact
   tenant context. They must register the already-earned local teardown
   capabilities.
   NNC6.5f owns Forwarded and Compose composition. These paths must fail closed
   until NNC6.5f installs their registries.
3. Joining an in-flight provision or restart is compute lifecycle work, not a
   services method. Services owns only source claims, session policy, and
   final projection/source mutation. The NNCV035 marker ownership must reflect
   this boundary.
4. Services projections currently equate definition/source generation with
   `WorkloadExecutionReference::generation`. Durable stop advances the workload
   generation without changing desired source bytes. The projection must carry
   both values. A session must bind the exact execution generation instead of
   treating a source counter as execution identity.

These corrections do not move effects into `nimbus-network`, add a second saga
store, or change the final NNC6.5g deletion owner.

## Current authority graph

```text
native service stop
  -> nimbus-compute::services
  -> ComputeResourceProvisioner::retire_sandbox_service
  -> ServiceManager::retire_service_for_decision_async
  -> SandboxBackend::{inspect, stop}

native sandbox stop
  -> nimbus-compute::sandboxes
  -> ServiceManager::retire_sandbox_resource_async
  -> SandboxBackend::{inspect, stop}

definition delete
  -> nimbus-compute::services
  -> ServiceManager::delete_service_definition_async
  -> process-local definition mutation guard
  -> ServiceManager::retire_service_for_definition_delete
  -> SandboxBackend::{inspect, stop}
  -> remove observation, definition, and optional sessions

server workload composition
  -> provision registry + retained restart runtime
  -> no teardown registry or retained teardown runtime
```

Current correctness gaps:

- provider effects can occur without a durable stopped successor.
- unresolved saga persistence does not fence direct stop.
- provision can cross the definition mutation guard.
- request cancellation can release the only local deletion guard.
- a late provision projection can race source deletion.
- restart settlement has no product join seam for teardown.
- source and execution generations use one counter.
- service start after a durable stop would submit a stale source generation.
- session channel fencing does not retain exact execution generation.
- native local composition has real providers but does not register teardown.
- missing teardown composition is not a named fail-closed product outcome.

## Target authority graph

```text
ServerWorkloadComposition
  -> exact optional WorkloadTeardownCapabilityRegistry
  -> ComputeState owns one Arc<WorkloadTeardownRuntime>

native caller
  -> ComputeResourceRetirer
       -> WorkloadProvisioner source-fence + in-flight join
       -> WorkloadSagaCoordinator stopped-successor CAS
       -> WorkloadRestartRuntime exact settlement join
       -> WorkloadTeardownRuntime five ordered capabilities
       -> ServiceManager exact finalize

ServiceManager
  -> source bytes and source-generation claim
  -> session admission/closure policy
  -> observed execution projection
  -> no native provider inspect/stop authority
```

The teardown registry remains a set of five small capabilities. There is no
`NetworkProvider`, `TeardownProvider`, or services callback with provider
authority.

## Frozen contracts

### Composition

- `ComputeWorkloadComposition::Managed` carries an optional exact teardown
  registry beside provision and restart registries.
- `ComputeState` constructs at most one teardown runtime from the same
  coordinator, source authority, immutable provider reports, and registry.
- Protocol-only and not-yet-migrated provider realms expose no teardown
  runtime. Native stop/delete returns a typed missing-capability error and
  makes zero source, store, or provider mutation.
- `ServerWorkloadProviders` uses one explicit optional teardown-capability
  bundle. It does not infer teardown support from provision or restart traits.
- The local Krun server registers `KrunTeardownAdapter`,
  `KrunAttachmentTeardownAdapter`, and the existing
  `ServerIngressPublicationAdapter` final-withdrawal capability.
- Forwarded server and Compose paths stay unchanged until NNC6.5f.

### Source fence and in-flight settlement

- Services issues a replayable, exact source-retirement claim. The claim binds
  tenant-qualified source identity, source generation, resource version, and
  operation kind. It also binds the loaded saga generation and revision that
  derive the stop successor.
- The claim is process-local source policy, not durable lifecycle authority.
  The Engine-backed saga is the only durable stop fence.
- `WorkloadProvisioner` owns the local linearization boundary. While it holds
  its keyed in-flight lock, it invokes the services claim callback. It returns
  the exact existing completion receiver, if any. A source reservation cannot
  pass between claim and keyed insertion.
- A claim survives caller cancellation after a submission can be ambiguous.
  Exact retry adopts it. Definite pre-submission validation can abort it.
- New native provision and explicit restart requests consult the source claim
  and fail before durable running intent or provider effects. A CAS race that
  passed preflight is still fenced by the stopped successor.
- Compute joins an already-tracked provision. It also resumes durable
  provision truth to settle a prior-process or late result before teardown.
- Compute invokes an exact restart-settlement method over the retained restart
  driver. Teardown cannot advance from `RestartSettlementPending` by polling
  or by fabricating absence.

### Generation and projection

- Desired source generation/resource version and workload lifecycle generation
  are separate typed facts at every services projection call.
- A running start after `Recorded` uses `checked_next()` from durable saga
  lifecycle generation while retaining the unchanged source generation and
  resource version.
- A stop successor also uses `checked_next()` and deterministic admitted
  content. Concurrent equal requests compose identical bytes. Overflow fails
  before source or provider mutation.
- `ServiceDefinitionObservation` and `SandboxResourceObservation` retain both
  source generation and observed execution generation. The complete
  `WorkloadExecutionReference` remains the execution identity.
- Session admission cross-checks source generation/resource version and exact
  execution reference under the manager lock. Its target generation is the
  execution generation. An old session cannot attach to a later start when
  source bytes do not change.
- No IP address, port, sandbox display name, or provider handle is workload
  identity.

### Native stop

The exact order is:

```text
load source and durable saga
  -> acquire source fence and join existing provision
  -> persist stopped successor / WithdrawalCommitted
  -> settle issued provision and restart work
  -> withdraw publication
  -> drain execution
  -> stop execution
  -> detach network
  -> release network
  -> persist Recorded
  -> finalize services observation
```

- The source and any sessions remain present until compute confirms the exact
  `Recorded` record.
- `Waiting`, `RestartSettlementPending`, `CleanupPending`, store ambiguity,
  provider ambiguity, or task failure cannot finalize source state.
- Native service stop preserves its definition. Standalone sandbox stop
  preserves its desired source. Each records a terminal stopped observation
  only from exact durable completion.
- A later start derives a new lifecycle generation from the saga. It does not
  increment or rewrite the source generation.
- A missing saga plus no provider observation is an effect-free terminal no-op.
  A missing saga with a retained provider observation is inconsistent and
  fails closed.

### Definition deletion

- Static definitions remain undeletable.
- Non-force deletion rejects open sessions before durable stop submission.
- Force deletion keeps definition, observation, sessions, and source claim
  until exact `Recorded` completion. It then closes the captured still-open
  sessions and removes the dynamic definition and observation under one
  manager lock.
- A definition generation/resource-version change, crossed claim, or changed
  session set fails finalization without deleting current bytes.
- An unstarted sandbox definition with no saga and no observation can finalize
  without a provider command.
- Store unavailability or unresolved ambiguity makes zero stop, detach,
  release, or source-removal effects.

## Failure and recovery matrix

| Cut or failure | Required result | Forbidden result |
| --- | --- | --- |
| Missing teardown registry | Typed fail-closed result. Source unchanged. | Direct services/backend fallback. |
| Cancellation before source claim | No claim, store call, or effect. | Retained mutation. |
| Cancellation after claim, before known CAS | Replayable claim remains for inspection. | Definition/source deletion or direct stop. |
| Saga submission unavailable | Claim/source remain. Zero provider calls. | Best-effort stop. |
| Saga submission ambiguous | Inspect exact durable record on retry. | New generation or in-memory Execute. |
| Provision was already tracked | Join exact completion, retain late success, then retire it. | Drop result or start a second provision. |
| Prior-process provision is unresolved | Resume durable truth and inspect exact claim. | Infer absence from no local task. |
| Restart is active | Settle exact veto/result before withdrawal. | Poll-only advance or new restart effect. |
| Provider phase returns waiting/ambiguous | Keep source claim and durable fences. | Finalize observed/source state. |
| Teardown reaches cleanup pending | Keep all source/session bytes and fences. | Record stopped or delete definition. |
| Process dies after stopped-successor CAS | Fresh request adopts saga and source claim state when available. | Direct provider cleanup. |
| Late provision projection | Join before finalization, then retire exact generation. | Re-publish after source deletion. |
| Generation overflow | Typed precondition failure with zero mutation/effect. | Wrap to zero or reuse an old generation. |
| Old session after later start | Exact execution-generation conflict. | Attach to the new execution. |

## Implementation checkpoint

The native cutover now has one compute-owned retirement coordinator and one
optional exact teardown runtime. It uses the existing saga coordinator,
provisioner, restart runtime, source authority, provider reports, and five
small capability registry. There is no direct services or backend stop
fallback.

Implementation convergence before the item review found and corrected four
directly related defects:

1. The terminal stopped successor initially rebuilt a network plan from the
   display workload ID.
   - `compile_terminal_empty_successor` now retains the tenant-qualified
     incarnation key, plan identity, and exact sovereignty.
   - It advances only lifecycle generation and clears terminal resources.
2. The prior-process path inspected an issued provision claim twice after
   exact settlement.
   - `SuccessorSettlementReady` now gives the settled durable record directly
     to teardown.
   - Teardown inspects the exact claim once.
   - Teardown releases only the network reservation that the claim retained.
3. The definition-retirement test fixture used a process-lifetime `OnceLock`
   for `LocalNetworkManager`. The manager is now harness-owned and releases the
   process-global authority when each test ends.
4. The service lifecycle dispatcher embedded the complete provision and
   retirement state machines in one stack future. The dispatcher boxes both
   concept operations at that boundary. The default-stack HTTP lifecycle proof
   now passes.

The one full item review ran against staged tree
`90e07c2cfb658a08f449514002f2446d40442a99` and patch SHA-256
`807111a1de586a89309f1cf6c5213e886f4b827bd186fa0275eb6a547f3aac68`.
It used GPT-5.6 Sol with xhigh reasoning and fast mode in thread
`019ff04b-cf09-7592-ac2a-61a3169d5f42`. The implementation owner accepted all
four findings:

1. A start or public resume could cross retirement before the first saga CAS.
   One supervisor lock now claims the services source fence, records a
   retiring fence, and joins the exact in-flight completion. Raw provision and
   public resume reject while retirement owns that fence. The fence remains
   through exact services finalization after any ambiguous or progressed
   outcome.
2. Public compute composition accepted an arbitrary teardown registry. An
   `ExactWorkloadTeardownCapabilityRealm` now validates the exact five roles
   and rejects missing or extra registrations before `ComputeState`
   construction.
3. The native static scan did not inspect the services-owned source finalizer.
   NNCV035 now scans that source directly and rejects backend stop authority
   there.
4. Two race tests used fixed-count `yield_now` polling. The tests now wait on
   the semantic source-claim event and contain no scheduler-count polling.

The sole narrow correction review ran against staged tree
`ac5d48c3063a7ef9b29aaf2f825ea6c6d8c16d19` and patch SHA-256
`1b80b3b831b226599b2a41198be7e2a6ebf56f1beb8de0a1df8b19b2c624e04f`.
The Nimbus wrapper invoked GPT-5.6 Sol with xhigh reasoning and fast mode once.
It split the large input into two internal passes, threads
`019ff0c6-bde6-7da3-b87c-5dfd4b4f863a` and
`019ff0cc-6065-76d3-afc5-c79b8f986048`. TruffleHog was clean. The two internal
passes are one correction-review invocation, not two review cycles.

The narrow review reported seven findings. We accepted and corrected six:

1. The exact teardown realm now retains and authenticates the network
   capability selection and execution-provider identity. The coordinator
   rejects a crossed realm before source, store, projection, authority, or
   provider effects.
2. Dynamic BuiltIn and External definition deletion uses the effect-free
   services finalizer directly. It does not require sandbox teardown
   composition when no workload saga can exist.
3. Unstarted source and definition finalizers accept only an unadvanced
   source-retirement claim at saga generation/revision `0/0`. Replayed
   advanced claims cannot enter an unstarted finalizer.
4. NNCV035 rejects aliased and UFCS backend stop calls in the services-owned
   source finalizer.
5. NNCV035 requires exact-realm identity dataflow through managed compute and
   the private server composition. A raw registry field or unused realm cannot
   satisfy the contract.
6. NNCV035 scans the semantic wait helper implementations, so hidden
   scheduler-yield polling cannot satisfy the race-test proof.

We rejected the remaining P3 claim. The server definition-retirement test does
not use a fixed-count poll. It uses a two-second deadline and probes the exact
source reservation after each scheduler yield. It stops only on the exact
`retirement claim` rejection and fails immediately on any other result. The
exact test passes `1/1`. The written acceptance contract permits this bounded
semantic observation loop.

Strict Clippy then found that the authenticated fields made the public managed
composition enum too large. The exact capability selection remains owned by
the same variant, and one `Box` stores it. No authority, identity, or lifecycle
behavior changed. Strict Clippy, warning-denied rustdoc, and the full compute,
server, and CLI suites pass after this cleanup. We exhausted the review
cadence. The plan neither permits nor needs a third structured review.

The same correction audit also made dynamic non-sandbox definition deletion
use an exact services-only finalizer when no workload saga can exist. The final
CLI suite exposed one directly related test-isolation defect: one
`LocalNetworkManager` fixture test lacked the serial guard used by the other
process-global manager tests. Its fail-before full run reported `994` passed,
one failed, and three ignored. The exact test then passed `1/1`, and the normal
parallel full CLI rerun passed `995` with three ignores.

The server fresh-process phase matrix still validates all 30 phase, target,
and action expectations. Its exact digest snapshot changed deterministically
with the canonical terminal empty successor and now passes in a new recovery
process.

Behavioral evidence at this checkpoint:

- the exact named roster is `23/23`.
- compute retirement tests are `17/17`.
- provision driver tests are `12/12`.
- teardown registry tests are `6/6`.
- services source-retirement tests are `4/4`.
- definition-retirement tests are `10/10`.
- the exact session fence test is `1/1`.
- workloads passes `219`.
- compute passes `403` with one ignore.
- services passes `89`.
- serialized server passes `719` across all targets with `33` ignores.
- CLI passes `995` with `3` ignores.
- the first serialized sandbox run passed `1,152`, failed two unchanged
  readiness-probe timeout tests, and ignored `32` library tests. The exact four
  readiness tests passed `4/4` in `0.18` seconds without source changes. The
  final serialized full rerun passes `1,164` across all targets with `48`
  ignores.
- network passes `274` with one ignore.
- the NNCV035 mutation helper passes `88/88`. Its native source stage is
  `1/1`.
- the first live aggregate exposed stale recovery/routing text and four shifted
  CLI census anchors. It also exposed an over-broad service backend check. The
  changes correct those proof defects.
- the corrected aggregate self-test passes `502/502`.
- the final live aggregate is `35/36`. NNCV035 is its only red condition.
  NNCV035 reports the exact six later-owner groups and no native-cutover group.

Fail-before evidence includes the terminal-plan identity test that failed on
the raw display workload ID. It also includes the prior-process settlement test
that failed on a duplicate exact inspection. Both tests now pass after the
root-cause corrections. The 23 canonical tests retain the direct-authority
failure assertions in the frozen roster. They pass without sleeps,
fixed-count scheduler-yield polling, or source-only substitutes.

The one server definition-retirement loop observes the exact source reservation until
its deadline. It does not use count-based polling.

## Exact path ownership

Primary product paths for NNC6.5e:

- `crates/nimbus-compute/src/resource_retirement.rs` and concept-owned tests.
- `crates/nimbus-compute/src/services.rs`, `sandboxes.rs`, and
  `resource_provision.rs` plus their tests.
- narrow source-fence/join additions in
  `crates/nimbus-compute/src/workload_provisioner.rs` and tests.
- narrow restart-settlement additions in
  `crates/nimbus-compute/src/workload_saga/restart_runtime.rs`,
  `restart_driver.rs`, and tests. The exact settled-provision handoff is in
  `provision_driver.rs`.
- narrow teardown composition fields/construction/accessors only in
  `crates/nimbus-compute/src/state.rs`, `workload_saga.rs`,
  `workload_saga/teardown_driver.rs`, `teardown_registry.rs`, its tests, and
  `lib.rs`.
- `crates/nimbus-compute/src/workload_network_plan.rs` only for the canonical
  empty stopped-successor compiler. `workload_projection.rs` only passes
  distinct source and execution generations to services.
- `crates/nimbus-server/src/workload_composition.rs` and tests.
- `crates/nimbus-server/src/http/services.rs` and `http/sandboxes.rs` only to
  pass the already-authorized exact tenant context.
- `crates/nimbus-server/src/tests/managed_workload.rs`,
  `tests/service_manager.rs`,
  `tests/service_manager/definition_retirement.rs`, and
  `tests/tenant_isolation_harness.rs` for exact native behavioral proof.
  `workload_saga_store/tests/composition.rs` only for the canonical changed
  fresh-process digest.
- `crates/nimbus-services/src/catalog.rs`, `lib.rs`, `manager.rs`,
  `manager/types.rs`, `manager/definitions.rs`,
  `manager/source.rs`, `manager/source_retirement.rs`,
  `manager/handles.rs`, `manager/sandboxes.rs`, `manager/sessions.rs`, and
  their non-tenant-retirement manager tests.
- `crates/nimbus-cli/src/network_composition.rs` and its directly attributed
  local-composition tests only to register real local Krun teardown
  capabilities.
- `crates/nimbus-cli/src/compose/lifecycle.rs` and its tests only for the
  mechanical `observed_generation` to `source_generation` projection rename.
  The mechanical rename does not change Compose teardown authority.
- the canonical plan, plan index, and this proof.
- the existing NNCV035 source-contract, fixture, and assertion helpers.
- the production composition census anchors that the native-cutover stage
  needs.

The narrow `state.rs`, `lib.rs`, `manager.rs`, provisioner, restart-runtime,
projection, HTTP, and local-composition ownership supersedes the broader
prospective path list only for NNC6.5e. NNC6.5g retains tenant deletion,
failed-provision compensation, legacy declarations, and deletion-only cleanup.

Forbidden paths and seams:

- `crates/nimbus-services/src/manager/retirement.rs` and
  `manager/tests/tenant_teardown.rs` except NNC6.5g deletion/convergence.
- `crates/nimbus-cli/src/network_composition/forwarded.rs`, Compose lifecycle,
  machine API/backend/manager teardown, and their product tests.
- `crates/nimbus-compute/src/state.rs::delete_tenant`, provision failure
  compensation, and node reconciliation.
- `crates/nimbus-sandbox/src/backend.rs` coarse trait deletion.
- NNC6.6 resolver/cache fencing and NNC8.3 cleanup finalization.
- any `nimbus-network` dependency or provider/effect growth.

## Fail-before proof roster

The following named tests must fail for the audited direct-authority reason
before product correction and pass afterward:

1. `service_stop_persists_then_observes_complete_teardown_order`.
2. `sandbox_stop_persists_then_observes_complete_teardown_order`.
3. `native_stop_without_teardown_composition_fails_before_source_or_effect`.
4. `native_stop_unresolved_submission_makes_zero_provider_calls`.
5. `service_stop_joins_inflight_provision_and_retires_late_success`.
6. `sandbox_stop_joins_inflight_provision_and_retires_late_success`.
7. `definition_delete_keeps_source_and_sessions_until_recorded_teardown`.
8. `definition_delete_fences_and_joins_inflight_provision_before_removing_source`.
9. `force_delete_unresolved_submission_keeps_definition_and_makes_zero_stop_effects`.
10. `late_provision_result_after_force_delete_is_retired_before_definition_removal`.
11. `definition_delete_cleanup_pending_keeps_definition_observation_and_sessions`.
12. `definition_delete_cancellation_after_submission_is_replayable`.
13. `service_start_after_recorded_stop_uses_next_lifecycle_generation`.
14. `sandbox_start_after_recorded_stop_uses_next_lifecycle_generation`.
15. `source_generation_remains_stable_across_stop_and_later_start`.
16. `session_binding_rejects_a_later_execution_with_the_same_source_generation`.
17. `concurrent_start_and_stop_linearize_at_the_source_fence`.
18. `active_restart_settles_before_withdrawal_committed`.
19. `generation_overflow_fails_before_source_store_or_provider_effect`.
20. `missing_saga_with_provider_observation_fails_closed`.
21. `service_stop_fences_start_before_its_first_saga_commit`.
22. `sandbox_stop_fences_start_before_its_first_saga_commit`.
23. `definition_delete_fences_start_before_its_first_saga_commit`.

Tests must use semantic barriers, injected stores/capabilities, and exact call
logs. They must not use sleeps, source-only assertions, or `SandboxBackend`
mock behavior as proof of the compute capability contract.

## Acceptance ledger

| Criterion | Status | Evidence required |
| --- | --- | --- |
| K1 audit and path freeze | `pass` | This record maps current/target authority, corrections, exclusions, and exact owned paths. |
| K2 one teardown runtime authority | `pass` | One optional runtime uses the existing coordinator, source authority, immutable reports, and exact registry. |
| K3 protocol-only/missing registry fail closed | `pass` | Missing composition makes zero source, store, or provider mutation. |
| K4 local Krun composition | `pass` | The local composition registers exact Krun execution, attachment, and ingress capabilities. Exact-realm validation rejects crossed registries. Linux-only runtime construction remains honestly platform-gated. |
| K5 source/provision linearization | `pass` | The semantic-barrier contender proof admits no provision between claim and keyed fence. |
| K6 cancellation-safe source claim | `pass` | Unpolled pre-submit cancellation makes zero mutation. Post-ambiguity retry adopts the retained exact claim. |
| K7 stopped-successor durability | `pass` | Both native stop logs place `WithdrawalCommitted` before the first provider call. |
| K8 provision settlement | `pass` | Local late success and prior-process issued work settle once. The latter inspects once and releases only the retained reservation. |
| K9 restart settlement | `pass` | Exact restart settlement precedes `WithdrawalCommitted`. No poll-only transition exists. |
| K10 exact five-step dispatch | `pass` | Service and sandbox logs are exactly withdraw, drain, stop, detach, release. |
| K11 terminal finalization gate | `pass` | Only exact `Recorded` commits terminal projection or source mutation. |
| K12 ambiguity/cleanup retention | `pass` | Waiting, provider ambiguity, store ambiguity, and cleanup-pending retain source, observation, sessions, and claims. |
| K13 service stop projection | `pass` | The definition remains and the exact stopped execution is projected truthfully. |
| K14 sandbox stop projection | `pass` | Desired sandbox source remains and the exact stopped execution is projected truthfully. |
| K15 definition delete safety | `pass` | Force, non-force, unstarted, crossed source, changed session set, and retained-session cases pass. |
| K16 source/lifecycle generation split | `pass` | Stop/start proofs retain source generation, advance execution generation, and fail before effects at `u64::MAX`. |
| K17 exact session execution fence | `pass` | An old session rejects a later execution while unchanged source bytes remain observable. |
| K18 no native direct effect calls | `pass` | The source-derived native stage is `1/1`. Direct services/backend stop calls are absent. |
| K19 legacy authority not expanded | `pass` | Retirement, tenant, and coarse authority declarations have no new caller and remain NNC6.5g-owned. |
| K20 later owners untouched | `pass` | Forwarded, machine, tenant, compensation, cleanup, and resolver authority are unchanged. Compose has only the mechanical projection-field rename. Its lifecycle authority is unchanged. |
| K21 dependency/effect invariants | `pass` | All `42` workspace packages remain acyclic. `nimbus-network -> nimbus-core` is its only workspace edge. Forbidden effect/import scans are empty. |
| K22 focused behavior | `pass` | The fully qualified canonical roster is `23/23`. Directly affected concept suites also pass. The three added tests fence start before the first teardown saga CAS by using the semantic source-claim event. |
| K23 full affected suites | `pass` | Workloads `219`. Compute `403 + 1 ignore`. Services `89`. Server `719 + 33 ignores`. CLI `995 + 3 ignores`. Sandbox `1,164 + 48 ignores`. Network `274 + 1 ignore`. The first sandbox run had two load-sensitive readiness timeouts; exact `4/4` and a clean full rerun disprove a product regression. Compute, server, and CLI pass again after the final representation cleanup. |
| K24 strict quality | `pass` | Format, strict Clippy, warning-denied rustdoc, and diff checks pass. Strict Clippy exposed the enlarged managed-composition enum; boxing its already-owned capability selection removed the representation defect without changing behavior. |
| K25 static contract | `pass` | The aggregate self-test is `502/502`. The NNCV035 helper is `88/88`, and the native stage is `1/1`. The live aggregate is `35/36` with only NNCV035 red. Its direct result is the expected later-owner `0/6`. |
| K26 docs and recovery | `pass` | Proof lint reports zero diagnostics. Docs are `108`, and the site is `17/17`. NNCV008 passes. The plan and ledger each contain `112` unique items with one `in_progress` item. The pre-freeze owned census contains `58` paths, including this ignored proof. |
| K27 complexity review | `pass` | New files are concept-owned. `definition_retirement.rs` (`1,779`) and `service_manager.rs` (`1,761`) are test-only concept/fixture roots. CLI `network_composition.rs` (`1,547`) ends production logic at line `831` and retains its prior composition-root exception. `state.rs` is `1,579` lines, but production state/composition logic ends at line `587`; the remaining inline tests validate that same concept, so this is an explicit coherent-owner exception below the `2,000`-line decomposition gate. Production `resource_retirement.rs` (`661`), `workload_provisioner.rs` (`743`), teardown registry (`569`), and services source retirement (`859`) remain coherent concept owners below the threshold. |
| K28 candidate identity | `pass` | The final corrected pre-ledger-closeout staged tree is `550bc50cbed9e18f9d7672abef65bee0b6d07a93`. Its patch SHA-256 is `7e21c7d9310ac4530defb1cf22780da2ef32c6af4df9352b768b6aa8b0c9830c`. The candidate has `59` paths, including `50` Rust paths, with `9,198` insertions and `554` deletions. |
| K29 one item autoreview | `pass` | The one full GPT-5.6 Sol/xhigh/fast review ran after the original K1-K28 freeze. It reported four accepted findings. No second full review is authorized. |
| K30 review findings | `pass` | The full review's four findings are accepted and corrected. The sole narrow review's six accepted findings are corrected and its bounded-semantic-wait claim is rejected with exact source/test evidence. Review cadence is exhausted. |
| K31 final rerun | `pass` | Focused regressions, all seven affected suites, quality gates, `88/88` helper mutations, `502/502` aggregate mutations, native `1/1`, and live expected-red `35/36` pass after correction. Compute, server, and CLI pass again after the Clippy-driven representation cleanup. |
| K32 durable item checkpoint | `pass` | The exact owned diff, proof, ledger, and routing transition commit together. The commit containing this row is the self-authenticating NNC6.5e item checkpoint. The user did not authorize a push or PR. |

## Verification cadence

During implementation, use focused fail-before tests, full affected owner
suites, static scans, and manual seam inspection. Do not run structured
autoreview on partial work. Run one full GPT-5.6 Sol/xhigh/fast item review only
after K1-K28 are green and the staged candidate is frozen. A material accepted
executable finding permits one narrow correction review after the correction
restores its proofs. Documentation wording, formatting, elapsed time, or
internal review chunking do not permit another review.

## Closeout verification before candidate freeze

Quality commands:

```text
cargo fmt --all --check
cargo clippy -p nimbus-workloads -p nimbus-network -p nimbus-compute -p nimbus-services -p nimbus-server -p nimbus-cli -p nimbus-sandbox --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps -p nimbus-workloads -p nimbus-network -p nimbus-compute -p nimbus-services -p nimbus-server -p nimbus-cli -p nimbus-sandbox
git diff --check
```

All four commands exit `0`. Strict Clippy found one redundant identity map in
the definition-retirement tests. The cleanup removed that map, and the exact
provider-ambiguity regression remained `1/1` afterward.

Static commands and results:

```text
bash scripts/verify-nimbus-network-control-plane.sh --self-test
self-test: 502 passed, 0 failed

bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --self-test
NNC6.5 teardown contract self-test: 88 passed, 0 failed

bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh --native-stage
Summary: 1 passed, 0 failed

bash scripts/nimbus-network-control-plane/workload-teardown-contract.sh
Summary: 0 passed, 6 failed

bash scripts/verify-nimbus-network-control-plane.sh
Summary: 35 passed, 1 failed
```

The two direct commands return the planned later-owner red state. The six
NNCV035 groups are services legacy convergence, Compose, machine, tenant,
compensation, and final behavior convergence. NNC6.5f and NNC6.5g own them.

Documentation and recovery results:

- proof lint: `PASS: 1 file(s), 0 diagnostic(s)`.
- documentation: `108` pages pass.
- documentation site: `17/17` conditions pass.
- NNCV008 and NNCV009 pass.
- the implementation-band and item-ledger counts are both `112`.
- both sets contain `112` unique IDs, and NNC6.5e is the only `in_progress`
  item.
- the pre-freeze owned census contains `58` visible paths plus this ignored
  proof. K28 records the exact staged candidate count and digest.

## Frozen candidate identities

The full review used tree `90e07c2cfb658a08f449514002f2446d40442a99`
and patch SHA-256
`807111a1de586a89309f1cf6c5213e886f4b827bd186fa0275eb6a547f3aac68`.
The corrected candidate before the narrow review used tree
`ec92d6ebd024fb1a59727e99abada9b931972852` and patch SHA-256
`34c98134205cbafd39c4217a7027cdafe82ac5971cc06cde04451641677aa69c`.
The actual narrow-review input, including its pre-review ledger closeout, used
tree `ac5d48c3063a7ef9b29aaf2f825ea6c6d8c16d19` and patch SHA-256
`1b80b3b831b226599b2a41198be7e2a6ebf56f1beb8de0a1df8b19b2c624e04f`.

K28 uses the final corrected staged tree before this final identity and ledger
closeout edit. This convention avoids a self-referential digest. The final
item commit is the self-authenticating durable identity.

```text
tree: 550bc50cbed9e18f9d7672abef65bee0b6d07a93
patch-sha256: 7e21c7d9310ac4530defb1cf22780da2ef32c6af4df9352b768b6aa8b0c9830c
paths: 59
Rust paths: 50
insertions: 9198
deletions: 554
unstaged paths: 0
```

The original full-review input was the `58`-path staged tree
`90e07c2cfb658a08f449514002f2446d40442a99`, with patch SHA-256
`807111a1de586a89309f1cf6c5213e886f4b827bd186fa0275eb6a547f3aac68`.
The narrow review used the separately recorded `ac5d48c...` staged input. The
final corrected identity above includes every accepted narrow-review fix and
the strict-Clippy representation cleanup.

## Initial fail-before evidence

At the durable start commit
`6b685627c94ea53705019778b108e47f252fd08a`:

- post-commit static verification is exact `35/36`. NNCV035 is the sole red
  condition with the seven planned later caller/convergence groups.
- the existing NNCV035 parser now has one native-cutover stage. Its corrected
  markers assign provision and restart settlement to compute. They assign
  source claim, finalization, and distinct source/execution projection to
  services.
- the expanded mutation suite passes `80/80`.
- the native stage is the expected `0/1`.
- the aggregate remains the expected `0/7` with no extra diagnostic.
- exact source slices now prove the optional server/compute runtime dataflow.
- the slices prove local Krun execution, attachment, and ingress registry
  construction.
- the slices prove native HTTP tenant-context forwarding and compute-owned
  settlement.
- the slices prove services-owned claim, finalization, and generation
  projection.
- the slices prove direct-effect absence and concept-owned test attribution.
  They do not accept whole-crate token joins.
- NNC6.5d4 K1-K35 and its review cadence are complete.
- this audit record started from a clean worktree.
- no product source changed during the read-only audit.
- the original checkout and all user-owned changes remain untouched.
- blocker: none.

The implementation checkpoint above supersedes the original audit state.
Both authorized review invocations are complete and fully dispositioned.
K1-K32 are green. The commit containing this record is the durable NNC6.5e
item checkpoint. NNC6.5f starts with a read-only substitution audit. The user
did not authorize a push or PR.
