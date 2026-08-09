# NNC6.5a Strict Portable Teardown Protocol And Durable Reducer

Status: `acceptance green; full and narrow review findings corrected; item commit pending`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Scope

NNC6.5 froze the teardown authority, race, failure, and path contract before
product edits. NNC6.5a now adds the inert, workloads-owned protocol that later
compute code will coordinate. It also adds the pure durable reducer, the exact
provision and restart handoff validators, and the strict server codec and
schema representation.

This item does not add an effect trait, dispatcher, provider adapter, runtime,
or product caller. It does not bind a socket, stop a workload, detach a network,
release a lease, publish an endpoint, or select a provider. NNC6.5b owns the
first compute command authority.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| T1 | `nimbus-workloads` owns one closed teardown cause, step, subject, provider-target, attempt, claim, result, inspection, retry, disposition, and pure-decision vocabulary. No duplicate portable authority exists. |
| T2 | Every teardown attempt binds the tenant-qualified workload key, saga ID, active generation, desired digest, assigned node, source digest, execution provider, network-plan digest, capability selection, issuing revision and issuing transition, stable initiating cause, latest successor fence, source phase, target phase, step, and typed subject. |
| T3 | Attempt, command, and claim identities use domain-separated deterministic derivation. A change to any bound field changes the identity. Unknown, empty, malformed, or crossed identity fails closed. |
| T4 | Every claim additionally binds the provider target, claimed revision, dispatch epoch, and dispatch authorization. The attempt binds the issuing transition. The post-CAS command identity binds the confirmed claim transition, which avoids a recursive transition digest. A stale generation, revision, attempt, epoch, target, step, or successor makes zero transition. |
| T5 | The portable reducer is pure and exhaustive. It returns data-only transition or command candidates and never provider authority or an effect-capable value. |
| T6 | Teardown persists `WithdrawalCommitted` with a `Ready` disposition before the first claim can exist. A candidate constructed from an unconfirmed record cannot execute. |
| T7 | Durable step order is exact: withdraw publication, drain execution, stop execution, detach network, release network, then record terminal evidence. No step can reorder, skip, or repeat a completed phase. |
| T8 | A step with no exact provider target or no established owner observation advances as resource-free without a claim and without fabricated provider or terminal evidence. Reference presence alone is not proof of an effect. An established effect requires the exact typed subject and target. |
| T9 | A recovered `DispatchPending` claim produces only `InspectExact`. Ambiguous effect results persist `InspectionRequired` before any inspection. |
| T10 | Each exact `NotCompleted` inspection transition authorizes the same attempt at the next epoch once. Reusing that evidence fails. A later exact inspection can authorize a later epoch. `Satisfied` advances without retry. `InProgress` or ambiguous inspection remains inspection-only. Exact definite failure enters `CleanupPending`. |
| T11 | Counter wire values are canonical unsigned decimal strings. Revision or dispatch-epoch overflow fails closed without a record change. |
| T12 | Replay, duplicate success, reused retry evidence, and a result crossed by attempt, command, epoch, target, step, transition, or provider evidence reject without revision change. |
| T13 | The initiating cause remains stable after teardown starts. A separate latest-successor fence advances when a newer successor arrives. The newer fence invalidates an unconfirmed candidate and converts an issued claim to exact inspection without discarding cleanup identity. |
| T14 | A stopped successor converts pending provision work to inspection before withdrawal. Exact provision success retains the effect for teardown; exact absence cannot retry provision; definite provision failure starts compensation from exact retained references. |
| T15 | Unissued restart work clears before withdrawal. Issued restart work settles through exact terminal inspection; terminal successor-veto state and any exact late-result cleanup obligations are retained before a one-time withdrawal handoff. Neither source nor target execution identity is lost, and restart cannot resume after the teardown fence. |
| T16 | Cleanup state retains the exact failed claim, failure evidence, inspection evidence, and retained resource references. It rejects successor replacement, claim rewrite, or premature reuse. |
| T17 | Failed-provision compensation considers only observed retained resources and chooses their reverse effect order. It never fabricates a resource or releases unobserved capacity. |
| T18 | `WorkloadSagaRecord` and its strict portable wire carry one optional teardown disposition. Format and transition digest domains advance cleanly. The field is absent on valid non-teardown records and required by validation for teardown or teardown-owned cleanup state. Unknown fields, null in place of a present value, missing required teardown state, and legacy formats fail closed. |
| T19 | The transition digest binds the complete teardown disposition and cause. Equal visible phases with different attempts, claims, epochs, results, or cleanup evidence have different transition identities. |
| T20 | The server physical codec carries one optional `teardownDisposition` object. The exact schema adds that field and no index. The field is absent on valid non-teardown records and required for teardown or teardown-owned cleanup state. Strict physical decoding rejects null, unknown, legacy, crossed, or state-required missing values before a record exists. |
| T21 | Workloads and server round trips preserve the exact attempt, claim, command, epoch, cause, provider target, transition, retry, and evidence bytes. Corruption leaves the prior durable record unchanged. |
| T22 | Existing workloads, server, and mechanical compute test fixtures compile against the clean format replacement. The compute fixture paths add no production behavior. |
| T23 | The new portable modules contain no async code, provider trait, socket, transport, runtime, sandbox, service, machine, tenant-policy, or network effect. Cargo manifests and the `nimbus-network -> nimbus-core` edge do not change. |
| T24 | All named behavior tests, affected crate suites, dependency/effect scans, format, strict Clippy, rustdoc, docs, and one candidate-frozen GPT-5.6 Sol/xhigh/fast review pass. NNCV035 remains the exact sole expected-red convergence gate at `0/11`; item-local Rust and codec tests prove the portable portions it cannot yet credit as a whole diagnostic. A narrow correction review runs only if an accepted finding materially changes executable code. |

## Target State Machine

```text
WithdrawalCommitted / Ready
  -> DispatchPending / claim persisted
  -> InspectionRequired / ambiguous or recovered claim
  -> DispatchPending(next epoch) / exact NotCompleted inspection
  -> next durable phase / exact success or Satisfied inspection

withdraw publication
  -> drain execution
  -> stop execution
  -> detach network
  -> release network
  -> record terminal evidence
```

The reducer may return a command candidate only after its claim is part of the
input confirmed durable record. NNC6.5b will convert that candidate into an
executable command after its own confirmed-CAS gate. The portable value itself
grants no effect authority.

## Portable Value Contract

The protocol uses these concept groups:

- Cause: stopped successor or failed-provision compensation.
- Step: withdrawal, drain, stop, detach, or release.
- Subject: publication, execution, or network.
- Attempt: complete active identity plus cause, phase pair, step, and subject.
- Claim: attempt plus exact provider target, durable transition, revision,
  epoch, and authorization.
- Result: exact success, definite failure, or ambiguous outcome for one claim.
- Inspection: satisfied, not completed, in progress, definite failure, or
  ambiguous for the same exact claim.
- Retry: one-use evidence that binds a `NotCompleted` inspection to the same
  attempt and next epoch.
- Disposition: durable ready, pending, inspection-required, completed, or
  cleanup state.
- Decision: pure next-transition, claim, inspection, retry, resource-free
  advance, terminal record, or no-op result.

The protocol never uses an IP address as workload identity. Provider targets
name admitted execution or network-resource handles and the exact retained
reference they operate on.

## Resolved Design Decisions

1. **No recursive transition identity.** An attempt binds the loaded issuing
   transition. Its persisted claim is part of the next transition payload. A
   later `WorkloadTeardownCommandId` binds that confirmed transition after the
   caller proves the CAS result. The claim never contains the digest of a
   payload that contains the claim.
2. **Resource existence comes from evidence.** Network and execution
   references can exist before their provider effects. Step applicability uses
   the ordered owner observations and exact provider selection. It does not use
   reference presence alone.
3. **The cause and successor fence are distinct.** The cause records why
   teardown began and never changes. Each attempt also binds the latest queued
   successor. A newer successor fences pending work without erasing the
   original retirement cause.
4. **Restart settlement retains both execution identities.** The handoff
   retains the exact restart claim, result, and both execution identities. It
   also retains the established owner observations. Later
   compensation can retire the target before or with the normal source
   teardown. NNC6.5a records these inert obligations. NNC6.5g owns their final
   effect convergence.
5. **Running and stopped successors share one neutral cause.** `Successor`
   means that the active generation must retire. It does not imply the desired
   state of the queued generation.
6. **Retry is evidence-scoped.** One exact not-completed inspection can
   authorize one next epoch. A later inspection can authorize another next
   epoch. There is no unsupported global one-retry limit.

## Provision And Restart Handoff

Provision and restart effects can finish after a stopped successor arrives.
The handoff rules are part of the portable state machine because they decide
which durable facts a later coordinator may act on.

1. The reducer vetoes an unissued provision or restart candidate without an
   effect.
2. An issued claim is never assumed absent. Recovery first requests exact
   inspection.
3. Exact success retains the resulting resource reference. Teardown starts at
   the first applicable reverse-order step.
4. Exact absence after a stopped successor never authorizes provision or
   restart retry.
5. Definite provision failure preserves all prior successful references and
   starts compensation from the latest observed resource.
6. The reducer can hand off a terminal successor-veto restart once. It cannot
   remove or reissue the restart before the teardown disposition records the
   fence.

## Failure And Reconciliation Matrix

| Boundary | Required durable outcome |
| --- | --- |
| Before withdrawal commit | No claim and no effect authority. |
| After withdrawal commit, before claim | `Ready`; recovery can propose the exact first claim. |
| After claim persistence, before effect | `DispatchPending`; recovery returns `InspectExact`. |
| Effect returns exact success | Persist exact success and advance one phase. |
| Effect returns definite failure | Persist failure and enter `CleanupPending`. |
| Effect outcome is ambiguous | Persist `InspectionRequired`; no retry. |
| Inspection reports satisfied | Advance without re-executing the effect. |
| Inspection reports not completed | Persist one same-attempt, next-epoch retry authorization. |
| Inspection reports in progress or ambiguous | Remain inspection-only. |
| Inspection reports definite failure | Retain claim and evidence in `CleanupPending`. |
| Process stops after any durable write | Fresh decode produces the same pure decision. |
| Process stops before a durable write | The prior record remains authoritative. |
| Later successor arrives | Fence every unconfirmed candidate; inspect every issued claim. |
| Resource reference is absent | Advance the matching resource-free phase without evidence. |
| Counter would overflow | Reject without transition or effect authority. |

## Named Behavior Matrix

### Happy path and boundaries

1. `teardown_successor_commits_withdrawal_before_first_claim`
2. `teardown_claim_binds_complete_active_and_successor_identity`
3. `teardown_happy_path_orders_withdraw_drain_stop_detach_release_record`
4. `resource_free_teardown_advances_without_claim_or_terminal_observation`
5. `teardown_step_requires_exact_phase_and_subject`
6. `teardown_provider_target_matches_exact_step_role`
7. `teardown_counter_boundaries_round_trip_canonical_decimal`
8. `teardown_dispatch_epoch_overflow_fails_closed`
9. `teardown_revision_overflow_fails_closed`

### Tamper, stale, and crossed identity

10. `teardown_attempt_id_rejects_each_tampered_identity_field`
11. `teardown_claim_rejects_unknown_and_noncanonical_wire_fields`
12. `teardown_transition_digest_binds_disposition_and_cause`
13. `teardown_claim_rejects_stale_generation_revision_and_successor`
14. `teardown_result_rejects_crossed_attempt_epoch_target_and_step`
15. `teardown_inspection_rejects_crossed_transition_and_provider_target`
16. `later_successor_vetoes_pending_teardown_execute_and_requires_inspection`

### Replay and duplicate outcomes

17. `replayed_teardown_claim_is_inspection_only`
18. `duplicate_teardown_success_is_rejected_without_revision_change`
19. `reused_teardown_retry_evidence_is_rejected`
20. `reordered_or_skipped_teardown_phase_is_rejected`

### Ambiguity and next epoch

21. `ambiguous_teardown_effect_persists_inspection_required`
22. `ambiguous_teardown_inspection_stays_inspection_required`
23. `in_progress_teardown_inspection_stays_inspection_required`
24. `teardown_inspection_not_completed_authorizes_same_attempt_next_epoch_once`
25. `teardown_inspection_satisfied_advances_without_retry`
26. `teardown_inspection_definite_failure_enters_cleanup_pending`

### Cancellation state

27. `cancel_before_teardown_claim_leaves_source_record_unchanged`
28. `cancel_after_teardown_claim_reopens_as_exact_inspection`

These are portable abandoned-candidate and persisted-claim proofs. Runtime
`CancellationToken` behavior belongs to NNC6.5b.

### Provision and restart handoff

29. `pending_provision_successor_converts_dispatch_to_inspection_before_teardown`
30. `provision_inspection_success_retains_effect_then_commits_withdrawal`
31. `provision_inspection_absence_never_retries_after_stopped_successor`
32. `provision_definite_failure_starts_compensation_from_exact_retained_references`
33. `unissued_restart_is_cleared_before_withdrawal`
34. `issued_restart_successor_waits_for_exact_terminal_inspection`
35. `restart_result_is_settled_before_withdrawal_committed`
36. `later_successor_rebinds_restart_and_teardown_fences_before_effect`

### Cleanup and strict portable wire

37. `teardown_failure_retains_exact_claim_failure_references_and_inspections`
38. `cleanup_pending_rejects_successor_replacement_claim_rewrite_and_reuse`
39. `failed_provision_compensation_releases_only_observed_resources_in_reverse_order`
40. `teardown_record_round_trips_strict_portable_wire`
41. `teardown_wire_rejects_unknown_missing_null_and_legacy_disposition_fields`
42. `teardown_wire_rejects_tampered_attempt_claim_epoch_cause_and_digest`

### Strict server persistence

43. `teardown_disposition_round_trips_through_physical_codec`
44. `strict_codec_rejects_missing_null_unknown_and_crossed_teardown_disposition`
45. `exact_schema_includes_optional_teardown_disposition_without_new_index`
46. `teardown_codec_preserves_exact_attempt_claim_and_transition_bytes`
47. `legacy_format_without_teardown_disposition_fails_closed`

Table-driven tamper cases must report their case count separately. A filtered
test command is not evidence unless test discovery proves the named test ran.

## Path Boundary

The exact primary product paths are frozen under "Prospective product path
ownership" in the NNC6.5 proof. NNC6.5a owns:

- the workloads teardown modules, state integration, provision/restart
  validators, exports, and named saga/store fixtures.
- the server workload-saga codec, schema, and named strict persistence
  fixtures.
- mechanical fixture conversion only in the three named compute test files.
- this proof, the canonical plan and index, and NNCV035 evidence paths.

The item must not edit compute production behavior or provider adapters. It
must not edit service, sandbox, node, or machine effects. Compose, tenant
retirement, logical naming, policy, TLS, proxy forwarding, cluster transport,
and system projections stay unchanged. The item must not change a Cargo
manifest.

The format replacement advances the saga format from v5 to v6. It advances the
complete transition identity from v4 to v5.

This item also owns the mechanical version-token update in
`scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh`.
The update preserves NNCV029's fail-closed checks and changes no authority.

## Candidate Implementation

The candidate adds one workloads-owned protocol and reducer with no product
effect. Its main properties are:

- stable, domain-separated attempt and command identities bind the complete
  lifecycle payload and confirmed transition.
- exact claims bind provider target, durable revision, dispatch epoch, and
  retry authorization.
- a pure reducer returns only data for claim persistence, exact inspection,
  resource-free progress, cleanup, or terminal recording.
- provision and restart handoffs retain exact issued-work evidence before
  withdrawal.
- strict format-v6 wire state rejects unknown, crossed, missing, null, and
  legacy teardown state.
- the server codec carries one optional `teardownDisposition` object and adds
  no index.
- compute changes are test-fixture conversions only. No compute production
  behavior or provider effect changed.

The candidate uses heap indirection for sparse teardown state and the largest
closed-enum payloads. This keeps provision and restart futures below the
default test-thread stack limit. Serde keeps the exact JSON shape.

## Complexity And Gate Findings

| Finding | Resolution and evidence |
| --- | --- |
| A teardown disposition stored inline enlarged every `WorkloadSagaRecord` and caused the existing `concurrent_dispatchers_create_one_provider_effect` test to overflow its default stack. | Store the optional disposition as `Option<Box<WorkloadTeardownDisposition>>`. The exact default-stack test and full compute suite now pass. An 8 MiB diagnostic run also confirmed stack size as the cause before the correction. |
| Strict Clippy found three large new enum variants. | Box the failed-provision claim, teardown success evidence, and proposed claim attempt. This preserves wire bytes and reduces stack pressure. Strict affected-crate Clippy now passes. |
| `saga/tests.rs` crossed 2,000 lines after the new tests. | Move the intact wire primitive test group to the concept-owned `saga/tests/wire_primitives.rs` child. The parent is now 1,898 lines. |
| NNCV029 still required saga v5 and transition v4. | Advance its exact checks to v6 and v5 and reject transition domains v1-v4. NNCV029 passes 24/24 and its mutation self-test passes 10/10. |
| The default-parallel server suite contends on deliberate process-global network composition authority. | Keep the NNC6.4a test boundary. The diagnostic parallel run reported 43 cascading `DuplicateProcessComposition` failures. The required serialized suite passes 602 tests with 32 declared child-entrypoint ignores. |
| Full-review corrections grew `saga/tests/teardown_state.rs` to 1,527 lines. | Move the strict portable-wire group intact to the concept-owned `saga/tests/teardown_state/wire.rs` child. Narrow-review inspection proofs use `teardown_state/inspection.rs`. Final lines are parent 1,434, handoff 427, inspection 101, and wire 162. Every new item file is below 1,500 lines. |

Two changed handwritten roots are in the explicit 1,500-1,999 line band:

- `saga/state.rs` is 1,755 lines. It remains the concept-owned record lifecycle
  and transition-validation composition root. Provision, restart, and teardown
  state logic already lives in separate children. Another split would divide
  the shared transition identity and validation contract.
- `saga/tests.rs` is 1,898 lines. It remains the shared saga fixture and test
  composition root. Concept groups use child modules, including the new wire
  primitive and teardown groups.

Every new teardown implementation or test child is below 1,500 lines. No new
generic helper or effect switchboard exists.

## Full Review And Corrections

The sole full item review used GPT-5.6 Sol, xhigh reasoning, and fast mode on
staged tree `e111a0afaf227ec9114b11c96625c645e1651e50`. The complete patch
SHA-256 was `9be80c24246d423f0db418563bd8f9b9ab35ca736963cf281dd021315f81cb8c`.
The executable/script patch SHA-256 was
`42db215e43abf096a095aeb861c14886ce2f5a5edca1e9917b28c5feb25503fb`.
The 29-path review ran in thread
`019fe5cb-75f5-7db3-aea3-5ec695cc3a22` and recorded its exact output in
`/tmp/nnc65a-autoreview.out` and `/tmp/nnc65a-autoreview.json`.

| Finding | Disposition and correction proof |
| --- | --- |
| P1, 0.98: terminal recording could discard an unsettled restart-target obligation. | Accepted. `NetworkReleased` now yields `RestartSettlementPending` while the retained settlement exists, and terminal recording rejects that state. The restart handoff test drives the record to this boundary, proves terminal rejection, and proves exact wire retention. Provider settlement remains with its later owner. |
| P2, 0.98: state-changing inspection results were not bound to the confirmed current inspection command. | Accepted. Every inspection result now carries the command identity. Validation derives and compares the exact current revision, transition, mode, and command before any advance. A stale satisfied result after a successor-fence change is rejected. |
| P2, 0.99: a retry claim could use evidence from a different inspection transition. | Accepted. `InspectionRequired -> DispatchPending` now requires `RetryAfterNotCompleted` evidence that matches the exact current inspection transition and command. A rehashed, internally consistent crossed-transition candidate is rejected. |
| P2, 0.98: valid-looking successors could rewrite or drop immutable teardown context. | Accepted. Success permits one exact receipt and observation append; fence-only transitions permit only the exact successor-fence change. Tests reject dropped restart settlement and rewritten prior receipt/observation prefixes. |
| P2, 0.97: recovered receipts were not cross-checked against complete active identity and terminal observations. | Accepted. Record validation now checks key, saga, generation, desired and source digests, node, execution and network providers, capability selection, cause, successor fence, revision bounds, and exact observation correspondence. Strict wire tests reject crossed provider identity and receipt/observation mismatch. |

These accepted findings materially changed executable code. The review cadence
therefore permitted one narrow correction review after the complete corrected
candidate was frozen and every written acceptance gate was green.

The narrow review used GPT-5.6 Sol, xhigh reasoning, and fast mode on staged
tree `cc87e8c38f6fceab558c863010b8f81df0b84de8`. The complete patch SHA-256
was `0de7f439a37b87d8a5063b0ba46363351c475cb66490435132f9527a1f507cad`.
The executable/script patch SHA-256 was
`3f4b893c1cdf78e03254c1ec2a06e56b68077704b3b29d93e434520ad661a1b6`.
The 31-path review output is `/tmp/nnc65a-narrow-autoreview.out` and
`/tmp/nnc65a-narrow-autoreview.json`.

| Narrow-review finding | Disposition and correction proof |
| --- | --- |
| P1, 0.99: a forged `NetworkReleased -> Recorded` successor could remove a pending restart settlement. | Accepted. Disposition removal now also requires no retained restart settlement. The exact forged terminal candidate failed before correction and is rejected after correction. Normal reducer behavior remains `RestartSettlementPending`; no provider effect moved into this item. |
| P2, 0.97: inspection-driven success and failure did not durably retain their exact confirmed command. | Accepted. A closed `WorkloadTeardownResultConfirmation` is mandatory on success receipts and definite failure. Inspection confirmation binds revision, transition, command, and claim; dispatch and inspection origins cannot substitute for each other. Crossed command/transition and forged-dispatch candidates are rejected for both success and failure. |
| P2, 0.98: cleanup recovery no longer had an independent receipt-to-observation comparison. | Accepted. Teardown-owned definite failure retains exact prior terminal observations. Recovery rejects a rewritten receipt, and predecessor validation rejects a coordinated receipt/observation rewrite. Generic cleanup state does not gain teardown authority. |

The exact narrow fail-before run passed 39 of 43 focused tests and failed only
the four new assertions. The corrected focused run passes 44 of 44. The
implementation corrects all three findings. The one full and one narrow review
exhaust the review cadence. Do not run a third review for executable closeout,
proof wording, formatting, or the item commit.

## Static Proof Obligations

1. `git diff --exit-code` over the workspace and relevant crate manifests.
2. No async, provider trait, socket, transport, vendor, or upper-layer effect
   token in the new workloads teardown modules.
3. No direct teardown-phase advance outside the workloads-owned reducer after
   fixture conversion.
4. No new `nimbus-network` dependency or effect. Its only workspace edge stays
   `nimbus-core`.
5. The NNC6.5 frozen audit path range remains valid after this new product
   candidate.
6. NNCV035 remains exact `0/11`, and the aggregate remains `35/36` with only
   NNCV035 red. Item-local Rust and codec tests prove the portable reducer,
   failed-provision cause, ordered candidate, and restart-settlement portions.
   The whole diagnostic groups stay red until all named later owners converge.

## Fail-Before Evidence

The fail-before baseline ran from clean recovery commit
`c0b5e8b4f8f3c3176af6ecaf1c6a29ea396d9fca` before this proof or any product
source edit.

| Check | Expected and observed result | Evidence |
| --- | --- | --- |
| Portable module absence | Exit `1`; `crates/nimbus-workloads/src/saga/teardown.rs` does not exist. | `/tmp/nnc65a-fail-before-module.out` |
| Portable type absence | Exit `1`; no `WorkloadTeardownAttempt`, `Disposition`, `DispatchClaim`, or `Decision` match exists in the current public/state roots. | `/tmp/nnc65a-fail-before-types.out` |
| Strict physical field absence | Exit `1`; `teardownDisposition` is absent from the server codec, schema, and codec test. | `/tmp/nnc65a-fail-before-codec.out` |
| Named test absence | With `pipefail`, Cargo test discovery succeeds and the exact acceptance test search exits `1`. | `/tmp/nnc65a-fail-before-test.out` |
| Named compile red | After the first test module lands, the exact `--no-run` command exits `101` with two missing-method `E0599` diagnostics and three missing-type `E0433` diagnostics. | `/tmp/nnc65a-compile-red.out` |

The Cargo discovery command reports only existing vendored Brotli warnings.
Those warnings do not change the expected missing-test result.

## Verification Ledger

| Checkpoint | Result |
| --- | --- |
| Durable base | NNC6.5 item commit `94b52356ec79ae678f970911c3f82efec44f46b0`; recovery checkpoint `c0b5e8b4f8f3c3176af6ecaf1c6a29ea396d9fca`; owner worktree clean before fail-before; original checkout unchanged. |
| Acceptance freeze | T1-T24, the failure matrix, 47 named tests, path boundary, and static proof obligations are frozen before product edits. |
| Fail-before | Four exact absence checks exit `1` as specified. The first named test then fails compilation at exit `101` only on the absent teardown disposition accessor and types. |
| Implementation | Complete. Workloads owns format v6, transition domain v5, the strict teardown values, pure reducer, provision/restart handoffs, and validation. Server owns strict physical persistence. Compute production source is unchanged. |
| Focused behavior | Test discovery proves `47` expected, `47` matched, `0` missing, and `0` duplicate names. All named tests pass inside the full affected suites. The final roster is `/tmp/nnc65a-test-roster-summary.out`. |
| Affected behavior | Workloads passes `216/216`. Compute passes `303` with one declared ignore. Serialized server passes `602` with `32` declared child-entrypoint ignores. Narrow focused behavior passes `44/44`; its exact fail-before was `39/43`. The corrected default-stack concurrency regression passes. |
| Static and seams | Corrected manifest, forbidden-effect-token, direct-teardown-advance, and network-edge scans pass. NNCV029 passes `24/24` plus `10/10`. NNCV035 passes `55/55` mutation tests and remains exact `0/11`. The corrected aggregate is `35/36` with only NNCV035 red. Strict Clippy and warning-denied Rustdoc pass for all three affected crates. Rustfmt, Bash syntax, scoped ShellCheck, diff, and proof lint with zero diagnostics pass. Docs pass `108` pages, and the site passes `17/17`. |
| Candidate review | The sole full GPT-5.6 Sol/xhigh/fast review completed at overall confidence `0.98`; its one P1 and four P2 findings are corrected. The one permitted narrow review completed at overall confidence `0.98`; its one P1 and two P2 findings are corrected. Review cadence is exhausted; no third review ran. |
| Final candidate identity | Acceptance-green staged tree before identity-only ledger insertion `9fb35e90d7f3fdcf09931dd822650dccd8c1c1d0`; complete patch SHA-256 `0d82c4566734884b09f610c35e2ea526669bf09895e221eb88ebe079857dbb86`; final executable/script patch SHA-256 `af284c1f7687c801a13aec5528fb3c03e9116828e828f1a54ef2edf7443fcb91`; 32 paths. |
| Final commit | Pending one reviewed, evidence-backed NNC6.5a item commit. No push or PR. |

## Acceptance Status

| Criteria | Status | Evidence |
| --- | --- | --- |
| T1-T23 | `green` | The strict protocol, reducer, handoffs, wire format, physical codec, schema, and mechanical fixture replacement pass the named and affected behavior suites. Static scans prove the path and effect boundaries. |
| T24 fail-before and freeze | `green` | The written contract, four absence checks, and first named compile-red proof are recorded before implementation. |
| T24 implementation and review closeout | `green` | The exact 47-name roster, final affected suites, static scans, expected-red verifier state, strict Clippy, Rustdoc, format, and both required reviews pass. All eight accepted findings have exact correction evidence. Review cadence is exhausted. |
| T24 documentation closeout | `green` | Proof lint passes with zero diagnostics. Docs pass `108` pages, and the site passes `17/17`. |
| T24 item commit | `red` | Exact final tree/patch identities and the NNC6.5a item commit remain. |
