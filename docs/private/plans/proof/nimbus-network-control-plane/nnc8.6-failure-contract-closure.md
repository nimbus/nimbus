# NNC8.6 Failure-Contract Closure

## Scope

NNC8.6 closes the 22 rows in the canonical failure-contract table. It does
not add a new failure model or repeat completed architecture audits. Each row
below names the exact current test that proves the required behavior and links
the owning item proof. Product paths stay closed unless execution contradicts
the mapped contract.

Starting checkpoint:
`e5a2eeeb4bb8bb1105c9742af94f86d926c0559d`.

## Acceptance contract

| ID | Criterion |
| --- | --- |
| K1 | The matrix contains each canonical failure row exactly once, in canonical order. |
| K2 | Every row names an executable deterministic test or a source-derived static unreachability proof. |
| K3 | Each named test exists in the current source tree and proves the complete row contract, alone or as an explicitly named set. |
| K4 | Ambiguous effects use exact inspection; cleanup failure retains authority; stale work is fenced; projection failure cannot mutate authority. |
| K5 | Grouped owner proofs pass on the candidate tree with exact counts and real exit status. |
| K6 | No product path changes unless one mapped proof fails because the behavior is absent. |
| K7 | The live architecture verifier, strict proof lint, format/diff checks, and private-doc gates pass. |
| K8 | Exactly one GPT-5.6 Sol/xhigh/fast item review runs after K1-K7 are green. A narrow correction review runs only for an accepted executable defect. |
| K9 | The proof, concise ledger transition, and exact item commit form one recovery checkpoint. No push or PR occurs. |

## Canonical closure matrix

Test names are Rust test-function names below their stated source path. Earlier
proofs support ownership and preserve historical fail-before evidence. The
named current tests are the behavioral authority.

| # | Failure row | Exact current proof | Owning evidence | Status |
| ---: | --- | --- | --- | --- |
| 1 | Admission reject | `crates/nimbus-compute/src/workload_network_plan/tests.rs`: `every_compile_failure_precedes_store_lease_provider_manager_and_sandbox_effects` exercises source, sovereignty, provider selection, and payload rejection while all five effect counters remain zero. | `nnc6.2-admitted-network-plan-compiler.md` C14 | mapped |
| 2 | Saga-intent commit ambiguous | `crates/nimbus-compute/src/workload_saga/ingress/tests.rs`: `ambiguous_exact_next_uses_one_fresh_read` and `ambiguous_nonconfirming_outcomes_use_one_fresh_read`; `crates/nimbus-server/src/workload_saga_store/tests/ingress.rs`: `crash_before_and_after_durability_reopens_exact_decision`. Only exact fresh truth exposes a decision; the pre-durability cut exposes none. | `nnc6.1e1-durable-workload-saga-ingress.md` I9/I12 | mapped |
| 3 | Partial reservation | `crates/nimbus-network/src/port_lease/tests.rs`: `reservation_batch_is_all_or_nothing_and_replays_in_order` and `foreign_claim_conflict_rolls_back_new_batch_siblings`; `crates/nimbus-compute/src/workload_saga/provision_compensation/tests.rs`: `eight_provision_failures_compensate_only_proven_resources_in_reverse_order`. A failed durable group publishes no subset, and later failure compensates only established resources. | `nnc3.1-atomic-port-lease-lifecycle.md`; `nnc6.5g-final-teardown-convergence.md` | mapped |
| 4 | Start failure | `crates/nimbus-compute/src/workload_saga/provision_driver/tests.rs`: `definite_failure_never_dispatches_a_later_step`; `provision_compensation/tests.rs`: `eight_provision_failures_compensate_only_proven_resources_in_reverse_order`. Prepare failure never reaches attach/publish and releases only the reserved network. | `nnc6.4-atomic-provision-caller-cutover.md`; `nnc6.5g-final-teardown-convergence.md` | mapped |
| 5 | Attach create ambiguous | `crates/nimbus-sandbox/src/backends/oci/network/netavark/recovery_tests.rs`: `fresh_process_converges_netavark_response_loss_matrix`; `attachment_lifecycle/tests/crash_recovery.rs`: `fresh_process_shared_attachment_crash_cuts_converge_without_duplicate_effects`. Exact attempts and handles are reopened; no duplicate setup or early segment release occurs. | `nnc5.4-partial-attachment-outcomes.md` R3/R6 | mapped |
| 6 | Netns exists but firewall/pin absent | `crates/nimbus-sandbox/src/backends/container/runtime/tests/attachment_readiness.rs`: `nnc0_6_container_is_not_ready_at_partial_attachment_boundary`; `crates/nimbus-sandbox/src/backends/krun/vm/tests.rs`: `nnc0_6_krun_rejects_netns_path_without_complete_attachment_evidence`; `oci/network/attachment_lifecycle/tests/attachment_readiness.rs`: `pin_false_unknown_missing_assignment_and_pep_failure_are_named_and_read_only`. Namespace existence never substitutes for status, pin, lifetime, or PEP evidence. | `nnc5.3-complete-attachment-readiness.md` R5/R11/R12 | mapped |
| 7 | Required PEP not ready | `crates/nimbus-sandbox/src/backends/oci/egress/tests/readiness.rs`: `authenticated_readiness_rejects_missing_or_substituted_pep_evidence`; Container/Krun tests `container_ready_rejects_active_pep_for_prior_desired_policy_attempt` and `krun_inspect_withdraws_ready_projection_when_pep_dependency_is_absent_or_not_ready`; `crates/nimbus-services/src/registry.rs`: `not_ready_endpoint_withdrawal_cannot_materialize_a_service_binding`. Missing/stale PEP evidence produces no endpoint or logical binding. | `nnc4.5-egress-readiness-dependency.md` E7/E10-E12 | mapped |
| 8 | Listener bind fails | `crates/nimbus-testing/tests/network_port_binding.rs`: `external_addr_in_use_is_durable_and_cannot_publish`; `crates/nimbus-network/src/port_lease/reservation_lifetime/tests.rs`: `substituted_lifetime_and_provider_ambiguity_stay_fenced`. Real collision records terminal no-effect failure; a may-exist claim cannot activate or release as never-bound. | `nnc3.3-provider-bind-adoption.md`; `nnc3.8-restart-cleanup-pending-reconciliation.md` A3/A6 | mapped |
| 9 | Partial forwarding/publish | `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication/tests/fault_matrix.rs`: `nnc5_4a_fail_nth_mutation_preserves_siblings_and_publishes_only_complete_batches`, `nnc5_4a_response_loss_at_every_slot_converges_from_current_observation`, and `nnc5_4a_unknown_conflict_and_ambiguous_diagnostics_fail_closed`. Complete batches publish once; every possibly visible slot is retained for inspection/withdrawal. | `nnc5.4a-machine-forwarded-batch-convergence.md` R8-R11 | mapped |
| 10 | Crash after attach | `attachment_lifecycle/tests/crash_recovery.rs`: `fresh_process_shared_attachment_crash_cuts_converge_without_duplicate_effects`; `crates/nimbus-server/src/workload_saga_store/tests/provision_driver_process.rs`: `fresh_process_reopens_engine_without_snapshot_handoff`. Fresh recovery reconstructs the exact generation and either completes once or retains named cleanup. | `nnc5.4-partial-attachment-outcomes.md`; `nnc6.4-atomic-provision-caller-cutover.md` | mapped |
| 11 | Crash after publish | `provision_driver_process.rs`: `fresh_process_reopens_engine_without_snapshot_handoff`; `crates/nimbus-sandbox/src/backends/container/runtime/tests/status_callbacks.rs`: `delayed_ready_callback_preserves_published_terminal_execute_bytes` and `delayed_ready_callback_preserves_cleanup_pending_execute_bytes`. Recovery does not republish, and delayed callbacks cannot reactivate terminal or cleanup-pending state. | `nnc8.1-persisted-phase-recovery.md`; `nnc8.4-stale-generation-restart-eligibility.md` | mapped |
| 12 | Exit inspection races withdrawal | Container/Krun tests `nnc0_6a_container_inspect_must_not_restart_after_withdrawal` and `nnc0_6a_krun_inspect_must_not_restart_after_withdrawal`. The semantic barrier makes inspection effect-free and the persisted fence vetoes restart. | `nnc0.6a-inspect-restart-withdrawal-baselines.md`; `nnc5.6-side-effect-free-sandbox-inspection.md` | mapped |
| 13 | Withdrawal fails | `crates/nimbus-services/src/manager/tests/source_projection.rs`: `restart_resolution_withdrawal_is_attempt_fenced_and_replay_safe`; `crates/nimbus-network/src/port_lease/lifetime/batch_reservation/tests.rs`: `active_and_ambiguous_plan_withdrawal_preserve_exact_cleanup_evidence`. Resolution stays closed and leases remain non-reusable until exact target readiness or terminal absence. | `nnc6.6-service-resolution-fencing.md`; `nnc3.8-restart-cleanup-pending-reconciliation.md` | mapped |
| 14 | Stop/detach ambiguous | `crates/nimbus-compute/src/workload_saga/teardown_driver/tests.rs`: `ambiguous_effect_result_persists_inspection_required`; `attachment_lifecycle/tests/crash_recovery.rs`: `fresh_process_shared_attachment_crash_cuts_converge_without_duplicate_effects`; `netavark/recovery_tests.rs`: `fresh_process_converges_netavark_response_loss_matrix`. Delete response loss is inspected, quarantined, and never reused early. | `nnc5.4-partial-attachment-outcomes.md` R7-R10; `nnc8.2-provider-command-live-claims.md` | mapped |
| 15 | Cluster lease expires | `crates/nimbus-sandbox/src/backends/oci/network/cluster.rs`: `expired_lease_must_fence_creation_but_allow_cleanup_of_a_durable_hold` and `expired_lease_fences_claim_adoption_but_retains_exact_compensation`. The fake clock revokes create/grow/adopt while retaining old-epoch inspection and cleanup. | `nnc2.4-stable-segment-identity-lease-epoch.md`; `nnc2.6-expired-lease-cleanup-authority.md` | mapped |
| 16 | Stale epoch callback | `crates/nimbus-compute/src/workload_saga/restart_dispatcher/tests.rs`: `every_restart_provider_callback_fence_is_checked_before_result`; `crates/nimbus-sandbox/src/backends/oci/network/segment/tests.rs`: `wrong_or_stale_cleanup_fence_cannot_release_an_allocation`. Stale callbacks cannot publish or release; exact bounded cleanup remains authenticated. | `nnc8.4-stale-generation-restart-eligibility.md` | mapped |
| 17 | Projection failure | `crates/nimbus-system/src/tests/connectivity.rs`: `projection_rebuild_restores_deleted_rows_without_touching_authority` and `projection_retry_coalesces_backs_off_and_cancels_with_engine`; `crates/nimbus-system/src/projection/tests.rs`: `projection_permanent_failure_retains_scope_without_hot_loop`. Projection repair is independent, retained, bounded, and effect-free. | `nnc7.5-projection-independence.md` | mapped |
| 18 | Torn/corrupt state | `crates/nimbus-network/src/state_store.rs`: `truncated_state_fails_closed_with_authority_path`, `checksum_rejects_semantically_valid_tampering`, and `incompatible_version_is_distinct_from_corruption`; attachment crash/process proofs reopen only checksummed current truth. No allocation is guessed. | `nnc0.4-torn-corrupt-state-baselines.md`; `nnc2.1-crash-safe-local-state.md` | mapped |
| 19 | Unsupported state-root semantics | `crates/nimbus-network/src/state_store.rs`: `known_network_filesystems_are_rejected_and_local_types_are_accepted`, `signed_ilp32_linux_magic_preserves_cifs_and_smb2_bit_patterns`, and `windows_verbatim_drive_and_unc_roots_are_classified_fail_closed`. `LocalNetworkStateStore::open` validates the nearest existing ancestor before `establish_root` opens authority. | `nnc2.1-crash-safe-local-state.md`, Filesystem And Permission Contract | mapped |
| 20 | Lock unavailable | `crates/nimbus-network/src/state_store.rs`: `contended_lock_times_out_without_an_unlocked_read`; `crates/nimbus-testing/tests/network_port_lease.rs`: `two_real_processes_same_request_get_exactly_one_bind_attempt_claim`. Lock failure is bounded and cannot fall through to an unlocked read/mutation. | `nnc0.1a-process-contention-harness.md`; `nnc8.5-bounded-retries-backoff-cancellation.md` | mapped |
| 21 | Provider cleanup fails | `crates/nimbus-sandbox/src/backends/oci/network/reaper/tests.rs`: `failed_bridge_cleanup_must_fence_segment_from_reuse`; `segment/tests.rs`: `cleanup_pending_survives_restart_and_reuses_only_after_fenced_finalize`; port test `active_and_ambiguous_plan_withdrawal_preserve_exact_cleanup_evidence`. Failed cleanup retains segment and port authority until exact finalization. | `nnc0.3-segment-cleanup-reuse-baseline.md`; `nnc2.5-two-phase-detach-release-quarantine.md` | mapped |
| 22 | Orphan evidence incomplete/unknown | `crates/nimbus-sandbox/src/backends/oci/network/startup_reconciliation/tests.rs`: `artifact_scan_unknown_is_preserved_and_fences_deterministically`, `unmatched_provider_without_a_hold_remains_durable_and_fences_every_restart`, and `exact_adoption_is_byte_preserving_across_every_durable_authority`; Container/Krun `nnc8_3_*exact_quarantined_orphan_converges_before_capacity_reuse`. Unknown stays quarantined, exact current state is adopted without effects, and filenames never become identity. | `nnc8.3-orphan-resource-convergence.md` K3/K7-K16 | mapped |

## Verification ledger

| Gate | Result |
| --- | --- |
| Source/test existence and 22-row uniqueness | Pass: canonical rows `22`, proof rows `22`, unique proof rows `22`, and order exact. Every named behavioral test executed on the candidate tree. |
| Network state, port, lock, and corruption owners | Pass: network library `248/248`; real external collision `1/1`; real two-process claim contention `1/1`. |
| Compute admission/provision/restart/teardown owners | Pass: workload-saga owner set `284/284`; admitted-plan no-effect boundary `1/1`. |
| Sandbox attachment, readiness, forwarding, lease-expiry, cleanup, and orphan owners | Pass: mapped sets total `47` passed and `2` declared child-only ignores. The fresh-process attachment and Netavark matrices, fail-Nth forwarding matrix, NNC0.6 barriers, PEP readiness, lease expiry, stale callback, cleanup reuse, and orphan convergence all pass. |
| Server fresh-process durability owners | Pass: ingress durability crash cuts `1/1`; provision-driver fresh-process recovery `1/1`. |
| Services withdrawal and fail-closed binding owners | Pass: endpoint withdrawal `1/1`; restart resolution fencing `1/1`. |
| System projection-independence owners | Pass: ordinary local lane `84/84` with implicit external-provider fixtures disabled. |
| Architecture/static/docs gates | Pass: the first verifier run passed `36/38` and found only missing recovery literals plus stale index routing. The corrected run passes `38/38`. Rustfmt, Prettier, diff checks, and strict proof lint pass. Docs pass `108` pages, and the site passes `17/17` conditions. |
| Candidate-frozen item review | Pass after one accepted documentation correction. The actual reviewer was GPT-5.6 Sol/xhigh/fast, thread `019fff81-f05f-7ef3-a79a-e5ed2c88d272`, with one P3 finding at `0.99`. The plan's dirty-state row omitted the staged routing index. The row now names all three paths. No executable code changed, so no narrow review ran. The earlier configured item-gate command skipped before any model call and does not count as a review. |
| Exact item commit | The commit that contains this proof, its ledger transition, and routing status is the exact NNC8.6 checkpoint. It contains no product or verifier path and does not push or open a PR. |

Environment: macOS Darwin `24.6.0` on `aarch64`. Rust is `1.96.1`. Cargo is
`1.96.1`. All commands used the shared repository target and retained their
real exit status.

Non-counted runs:

- An unqualified exact compute filter selected zero tests. The corrected
  qualified filter passed `1/1` and supplies the recorded admission evidence.
- The raw system library command passed `81` tests and failed three external
  provider fixtures because their pinned environments were absent. The
  documented ordinary lane passed `84/84`. External databases do not own the
  projection contract in row 17.
- The first piped server capture detached while the test target compiled and
  produced no result. Both exact tests then ran from the shared target and
  passed `1/1` each.

K1-K9 are green. No mapped execution contradicted the source-derived contract,
so NNC8.6 changes no product or verifier path.

## Ownership conclusion

The mapping introduces no new authority. `nimbus-network` remains a durable,
transport-free control-plane contract. `nimbus-core` remains its only workspace
dependency. Compute remains the workload saga coordinator. Sandbox, server,
KV, machine, proxy, and node retain provider effects. Services retains logical
naming. System retains observed projections. Cluster lease expiry remains
allocation fencing and does not introduce cluster transport.
