# NNC8.4 Stale Generation And Restart Eligibility

Status: `complete`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

Dependency checkpoint: `fa10498777d5b44b5c698b7a91bb127b2aa23db4`

## Outcome

NNC8.4 proves that delayed actors cannot publish, reactivate, restart, or
retire authority after a newer fence wins. The read-only audit found the
required product fences in their existing owners. It found one test-coverage
gap: the compute restart dispatcher dynamically crossed only the execution
attempt, although a provider callback carries ten independent correlation
fields.

This item adds one table-driven behavior test through the real dispatcher. It
does not add a provider, callback API, state machine, authority, or effect.

## Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| K1 | The source census names every in-scope publication, activation, restart, and cleanup callback seam. A surface with no callback is classified instead of receiving a speculative symmetric API. |
| K2 | Portable network state rejects stale/future generations, crossed digests, and stale/future lease epochs without mutation. Released and failed resources cannot reactivate. |
| K3 | Port, segment, and IPAM cleanup authenticate stable identity, generation, lease epoch, and provider evidence. Old cleanup can finish only its retained handle and cannot mutate or release replacement authority. |
| K4 | The sandbox provider-command journal rejects stale workload generations and dispatch epochs before callback I/O or durable mutation. A delayed execution token cannot cross a successor claim. |
| K5 | Delayed Container Ready callbacks cannot reactivate terminal or cleanup-pending state. A callback that waited for lifecycle authority cannot overwrite a changed manifest. Krun has no equivalent external runner-status callback and gains none. |
| K6 | Real Container and Krun cleanup callbacks reject stale claims while the adjacent successor remains byte-stable and claimed. |
| K7 | Container and Krun inspection remain side-effect-free. A durable withdrawal reports restart ineligible and produces zero restart effects. |
| K8 | Withdrawal or a successor that wins before restart admission makes zero provider effects. A withdrawal after admission vetoes unissued commands. |
| K9 | A successor that wins after an effect but before result CAS permits only exact inspection, retains the issued evidence, and cannot start a new effect. |
| K10 | One table-driven dispatcher test crosses command ID, transition ID, desired generation, desired digest, request ID, source attempt, target attempt, restart epoch, dispatch epoch, and provider selection. Each callback is rejected as `CrossedProviderObservation` before a result can reach the saga reducer. |
| K11 | Services rejects stale endpoint generations and old execution-attempt projections without changing desired or observed bytes. Resolution remains withdrawn until exact restarted publication is observed. |
| K12 | Server listener adoption, machine forwarding, node status, and forwarded-machine stop/restart seams reject stale or crossed provider generations before authority mutation or provider effects. |
| K13 | NNCV024 and NNCV034 remain green, including all 86 NNCV034 fail-closed mutations. The live architecture verifier remains green and `nimbus-network -> nimbus-core` remains its only workspace edge. |
| K14 | Focused behavior, complete affected suites, strict Clippy, warning-denied Rustdoc, format, diff, docs, site, and proof-lint gates pass. Exactly one Sol/xhigh/fast item review runs after K1-K14 are candidate-green; one narrow review is allowed only for an accepted executable correction. |

## Bounded Callback Matrix

| Effect at risk | Owning seam | Required fences | Executable proof |
| --- | --- | --- | --- |
| Publish or re-publish ingress | Compute restart driver and services projection | desired generation, execution attempt, endpoint identity, restart fence | `publication_waits_for_new_attempt_readiness`; `service_projection_rejects_stale_endpoint_generation_before_mutation`; `sandbox_projection_rejects_delayed_attempts_and_preserves_target_snapshot` |
| Reactivate portable network state | `nimbus-network` state/status reducers | plan/resource identity, desired generation, plan digest, lease epoch, terminal phase | `terminal_phases_cannot_move_or_reactivate`; `version_validation_rejects_wrong_identity_generation_digest_and_epoch`; `stale_future_conflicting_and_wrong_identity_observations_fail_without_mutation` |
| Reactivate a terminal provider workload | Container runner callback | durable manifest snapshot, shutdown/cleanup finality, lifecycle lock | `delayed_ready_callback_preserves_published_terminal_execute_bytes`; `delayed_ready_callback_preserves_cleanup_pending_execute_bytes`; `execute_status_callback_rejects_a_manifest_changed_while_waiting_for_lifecycle_authority` |
| Restart from an exited observation | Sandbox inspection and compute admission | side-effect-free inspection, source/desire generation, saga revision, withdrawal/successor | both `nnc0_6a_*_inspect_must_not_restart_after_withdrawal`; `withdrawal_winning_before_admission_vetoes_cas`; `successor_winning_before_admission_vetoes_cas` |
| Continue an admitted restart | Compute driver | exact durable command claim, successor veto, inspection-only ambiguity | `withdrawal_after_admission_vetoes_unissued_command`; `successor_after_effect_before_result_cas_allows_inspection_only` |
| Accept a provider callback result | Compute restart dispatcher | full ten-field callback tuple | new `every_restart_provider_callback_fence_is_checked_before_result` |
| Apply delayed provider-command work | Sandbox provider journal | workload generation, restart ordinal, dispatch epoch, live claim | `higher_generation_requires_resolved_prior_effect_and_fences_stale_generation`; `stale_async_inspection_fails_before_polling_its_callback`; `claimed_restart_inspection_fences_a_delayed_async_token_before_io` |
| Retire replacement network authority | Segment/IPAM and backend cleanup owners | tenant, attachment/segment ID, generation, lease epoch, reservation/provider claim | `wrong_or_stale_cleanup_fence_cannot_release_an_allocation`; `stale_claim_cannot_load_or_delete_reallocated_same_attachment_ipam`; both `stale_*_cleanup_cannot_mutate_replacement_network_generation` |
| Adopt a stale listener or forwarder | Server, machine, node, CLI effect owners | provider handle/incarnation, generation, execution reference | `external_listener_recovery_rejects_stale_provider_generation`; `forwarder_authority_authenticates_only_the_exact_incarnation`; `status_evidence_write_rejects_mismatched_generation_before_persistence`; `machine_stop_stale_or_crossed_machine_generation_makes_zero_effects` |

## Source-Derived Substitution Result

- `nimbus-network` owns only portable identity, reducer, status, segment, and
  lease fencing. It has no socket, provider, restart, or callback effect.
- Container owns the external service-runner callback. The callback reloads
  and authenticates durable manifest state while holding the lifecycle lock.
- Krun has no external service-runner callback. Its state changes use the
  provider journal and lifecycle methods. A symmetric callback would create a
  new authority.
- Sandbox provider adapters own exact execution claims and reject stale tokens
  before invoking the supplied callback.
- Compute owns restart admission, command confirmation, dispatch, result CAS,
  and successor resolution. The provider observation is evidence only.
- Services owns logical resolution and endpoint projection. It authenticates
  source generation, execution attempt, endpoint identity, and network
  generation before replacing observed state.
- Server, machine, node, and CLI retain their concrete listener, forwarding,
  status-write, and guest-effect boundaries. Their provider-generation checks
  do not move into `nimbus-network`.

## Expected-Red And Change Boundary

The audit found no product defect and therefore does not invent a product
fail-before. Before this item, source inspection found only
`old_attempt_provider_observation_is_rejected_before_result` as dynamic
dispatcher callback-correlation coverage. The product matcher already checked
all ten fields, and NNCV034 statically guarded the tuple.

The missing acceptance evidence is a table-driven test that crosses every
field through `WorkloadRestartDispatcher::dispatch_confirmed`. The only
executable path opened by this audit is:

- `crates/nimbus-compute/src/workload_saga/restart_dispatcher/tests.rs`

The plan, this proof, and final ledger row are the only documentation paths.
Production Rust, `nimbus-network`, provider adapters, schemas, dependencies,
and public APIs remain closed.

## Non-Goals

- Do not add a general callback interface or a network provider interface.
- Do not move effects, logical naming, policy, restart coordination, or
  projection authority.
- Do not add a Krun runner callback that has no production caller.
- Do not reopen NNC6.4a restart choreography or NNC8.3 orphan convergence.
- Do not add another static verifier condition when existing NNCV024, NNCV034,
  and the behavioral matrix prove the contract.
- Do not run workspace-wide gates before the focused matrix is green.

## Verification Ledger

| Gate | Result |
| --- | --- |
| Read-only source census | Complete; one dynamic callback-tuple coverage gap, no product defect. |
| Focused callback tuple | Pass: the real dispatcher rejects all ten crossed fields, `1/1`. |
| Focused matrix | Pass: `145` tests across network, sandbox, compute, services, server, machine, node, and CLI. |
| Complete affected suites | Pass: compute `477 passed / 1 ignored`. The only executable change is a compute test. |
| NNCV024 / NNCV034 / live architecture | Pass: side-effect-free inspection; restart contract; `86/86` mutations; live verifier `38/38`. |
| Strict quality and docs | Pass: all-target/all-feature compute check and Clippy, warning-denied Rustdoc, Rustfmt, Prettier, diff, strict proof lint with zero diagnostics, docs `108`, and site `17/17`. |
| Sol/xhigh/fast item review | Pass with one accepted P3 recovery-text defect at `0.98`. The implementation and callback test are accepted. The P3 is corrected without executable changes, so no narrow review is authorized. |

The focused matrix uses existing tests for every unchanged owner. It does not
repeat complete sandbox, services, server, machine, node, or CLI suites. The
complete affected suite is compute because this item changes only compute test
code. The prior NNC8.3 full owner suites remain the clean dependency checkpoint.

## Item Review

The one full item review used GPT-5.6 Sol with xhigh reasoning and fast mode.
The configured `item` cadence skipped without contacting a reviewer because
the repository cadence is `pre-pr`. The manual gate then reviewed one local
bundle. TruffleHog was clean, and the review reported one finding at confidence
`0.98`.

| Finding | Disposition |
| --- | --- |
| P3: the plan header and NNC8.4 row still directed recovery to add the test and run gates that were already complete. | Accepted. The header and row now state the candidate-complete review result and exact closeout action. This changes no executable code, so the review cadence forbids a narrow review. |

The review input used staged tree
`683e68763289e3a706e63168abe7fcf611cb780d` and binary patch SHA-256
`3841c550b9f29dddb1d1870e1c643505acf64c775b3f7fdbf27888d5023a42a2`.
