#!/usr/bin/env bash
# Verification gate for
# docs/private/plans/node-full-substrate-realm-plan.md.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/private/plans/node-full-substrate-realm-plan.md"
CARGO_MANIFEST="Cargo.toml"
CARGO_LOCK="Cargo.lock"
NFR1_PROOF="docs/private/plans/proof/node-full-substrate-realm/nfr1-profile-keyed-startup-snapshot.md"
NFR2_PROOF="docs/private/plans/proof/node-full-substrate-realm/nfr2-deno-realm-seam-inventory.md"
NFR3_PROOF="docs/private/plans/proof/node-full-substrate-realm/nfr3-realm-lease-state-machine.md"
NFR4_PROOF="docs/private/plans/proof/node-full-substrate-realm/nfr4-realm-scoped-execution.md"
NFR5_PROOF="docs/private/plans/proof/node-full-substrate-realm/nfr5-security-cleanliness.md"
NFR6_PROOF="docs/private/plans/proof/node-full-substrate-realm/nfr6-benchmark-adoption.md"
NFR6_BENCH_ARTIFACT="docs/private/plans/proof/node-full-substrate-realm/artifacts/nfr6-node-full-realm-current-rss.jsonl"
EXECUTION_PLAN="crates/nimbus-runtime/src/execution_plan.rs"
LIMITS_AXES="crates/nimbus-runtime/src/limits/axes.rs"
LIMITS_GRANTS="crates/nimbus-runtime/src/limits/grants.rs"
LIMITS_RESOURCES="crates/nimbus-runtime/src/limits/resources.rs"
LIMITS_TESTS="crates/nimbus-runtime/src/limits/tests.rs"
STARTUP_KEY="crates/nimbus-runtime/src/backends/v8/startup_key.rs"
CONSTRUCTION="crates/nimbus-runtime/src/runtime/driver/construction.rs"
SNAPSHOT_TESTS="crates/nimbus-runtime/src/runtime/tests/snapshot_lifecycle.rs"
POOL_REUSE_TESTS="crates/nimbus-runtime/src/runtime/tests/pool_reuse.rs"
NODE_WATCHPOINTS="crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_extended.rs"
REALM_LEASE="crates/nimbus-runtime/src/runtime/realm_lease.rs"
RUNTIME_BUNDLE="crates/nimbus-runtime/src/runtime/bundle.rs"
RUNTIME_POOL_BENCH="crates/nimbus-runtime/benches/runtime_pool_modes.rs"
WARM_POOL="crates/nimbus-runtime/src/backends/v8/warm_pool.rs"
DRIVER_LOADING="crates/nimbus-runtime/src/runtime/driver/loading.rs"
DRIVER_INVOCATION="crates/nimbus-runtime/src/runtime/driver/invocation.rs"
BOOTSTRAP_STATE="crates/nimbus-runtime/src/runtime/bootstrap/state.rs"
NODE_BOOTSTRAP="crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js"
DENO_JSRUNTIME="/Users/jack/src/github.com/nimbus/deno/libs/core/runtime/jsruntime.rs"
DENO_JSREALM_TESTS="/Users/jack/src/github.com/nimbus/deno/libs/core/runtime/tests/jsrealm.rs"
DENO_PROCESS="/Users/jack/src/github.com/nimbus/deno/ext/node/polyfills/process.ts"
DENO_MESSAGE_PORT="/Users/jack/src/github.com/nimbus/deno/ext/web/13_message_port.js"
DENO_STRUCTURED_CLONE_TEST="/Users/jack/src/github.com/nimbus/deno/tests/unit/structured_clone_test.ts"
MAKEFILE_PATH="Makefile"
CI_WORKFLOW=".github/workflows/ci.yml"

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf '  \033[32mPASS\033[0m  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  if [ $# -ge 2 ]; then
    printf '        %s\n' "$2"
    FAIL_DETAIL+=("$1 -- $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
}

contains() {
  grep -qE -- "$1" "$2" 2>/dev/null
}

contains_all() {
  local file="$1"
  shift
  local missing=()
  local pattern
  for pattern in "$@"; do
    if ! contains "${pattern}" "${file}"; then
      missing+=("${pattern}")
    fi
  done
  if [ "${#missing[@]}" -eq 0 ]; then
    return 0
  fi
  printf '%s' "${missing[*]}"
  return 1
}

not_contains() {
  ! contains "$1" "$2"
}

printf '\033[1mNFR verification gate -- NodeFull substrate realm\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

step 1 "NFR ledger records NFR0-NFR6 done with a benchmark rejection"
if [ -f "${PLAN}" ] &&
  contains '\| NFR0 \| `done`' "${PLAN}" &&
  contains '\| NFR1 \| `done`' "${PLAN}" &&
  contains '\| NFR2 \| `done`' "${PLAN}" &&
  contains '\| NFR3 \| `done`' "${PLAN}" &&
  contains '\| NFR4 \| `done`' "${PLAN}" &&
  contains '\| NFR5 \| `done`' "${PLAN}" &&
  contains '\| NFR6 \| `done`' "${PLAN}" &&
  contains 'NFR1 profile-keyed startup snapshot closeout' "${PLAN}" &&
  contains 'NFR2 Deno realm seam inventory' "${PLAN}" &&
  contains 'NFR3 runtime realm lease state machine' "${PLAN}" &&
  contains 'v2\.8\.3-nimbus\.78' "${PLAN}" &&
  contains 'NFR6 benchmark and adoption decision' "${PLAN}" &&
  contains 'NodeFull realm pooling is rejected' "${PLAN}"; then
  pass "NFR0-NFR6 are closed and NFR6 records a benchmark-backed rejection"
else
  fail "NFR phase ledger is not in the expected completed state" \
    "expected NFR0-NFR6 done and NFR6 rejection recorded"
fi

step 2 "RuntimeStartupSnapshotKey owns profile-keyed snapshot selection"
if [ -f "${STARTUP_KEY}" ] &&
  contains_all "${STARTUP_KEY}" \
    'pub\(crate\) enum RuntimeStartupSnapshotKey' \
    'WebLean' \
    'WebLeanService' \
    'NodeFull' \
    'NodeFullService' \
    'for_limits' \
    'RuntimeProfile::for_compatibility_target' \
    'snapshot_build_target' \
    'service_extension_enabled' \
    'RuntimeCompatibilityTarget::Node22' \
    'startup_snapshot_key_collapses_node_majors_to_node_full' \
    'startup_snapshot_key_keeps_web_and_unsupported_targets_separate' \
    'startup_snapshot_key_partitions_optional_service_extension' >/tmp/nfr1-startup-key-missing.txt; then
  pass "startup snapshot key is a narrow internal module"
else
  fail "RuntimeStartupSnapshotKey module is incomplete" \
    "$(cat /tmp/nfr1-startup-key-missing.txt 2>/dev/null)"
fi

step 3 "Construction uses one NodeFull startup substrate, split only by service extension"
if [ -f "${CONSTRUCTION}" ] &&
  contains_all "${CONSTRUCTION}" \
    'WEB_STANDARD_SERVICE_BOOTSTRAP_SNAPSHOT' \
    'NODE_FULL_BOOTSTRAP_SNAPSHOT' \
    'NODE_FULL_SERVICE_BOOTSTRAP_SNAPSHOT' \
    'RuntimeStartupSnapshotKey::for_limits' \
    'snapshot_key.snapshot_build_target' \
    'snapshot_key.service_extension_enabled' >/tmp/nfr1-construction-missing.txt &&
  ! contains 'NODE20_BOOTSTRAP_SNAPSHOT' "${CONSTRUCTION}" &&
  ! contains 'NODE22_BOOTSTRAP_SNAPSHOT' "${CONSTRUCTION}" &&
  ! contains 'NODE24_BOOTSTRAP_SNAPSHOT' "${CONSTRUCTION}" &&
  ! contains 'NODE26_BOOTSTRAP_SNAPSHOT' "${CONSTRUCTION}"; then
  pass "construction collapses Node-major startup substrate while partitioning service extension state"
else
  fail "construction still has per-Node-major startup snapshot state" \
    "$(cat /tmp/nfr1-construction-missing.txt 2>/dev/null)"
fi

step 4 "NFR1 tests prove sharing and exact per-target metadata"
if [ -f "${SNAPSHOT_TESTS}" ] &&
  contains_all "${SNAPSHOT_TESTS}" \
    'node_major_startup_snapshots_share_node_full_cell' \
    'node_full_shared_snapshot_keeps_exact_node_target_metadata' \
    'RuntimeLimits::application_node\(target\)' \
    'std::ptr::eq' \
    'processVersion' \
    'versionsNode' \
    'globalMajor' \
    'processMajor' >/tmp/nfr1-tests-missing.txt; then
  pass "focused tests prove Node snapshot sharing without metadata collapse"
else
  fail "NFR1 focused tests are incomplete" \
    "$(cat /tmp/nfr1-tests-missing.txt 2>/dev/null)"
fi

step 5 "NFR1 proof records current PR21 reconciliation verification"
if [ -f "${NFR1_PROOF}" ] &&
  contains_all "${NFR1_PROOF}" \
    'NFR1 Profile-Keyed Startup Snapshot Proof' \
    'PR21 NFR-R1 Reconciliation Verification' \
    'RuntimeStartupSnapshotKey' \
    'NodeFull startup snapshot cell' \
    'node_major_startup_snapshots_share_node_full_cell' \
    'node_full_shared_snapshot_keeps_exact_node_target_metadata' \
    'startup_snapshot_key_partitions_optional_service_extension' \
    'cargo test -p nimbus-runtime startup_snapshot_key --lib -- --nocapture' \
    '3 passed; 0 failed; 0 ignored; 0 measured; 1230 filtered out' \
    'cargo test -p nimbus-runtime node_major_startup_snapshots_share_node_full_cell --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1232 filtered out' \
    'cargo test -p nimbus-runtime node_full_shared_snapshot_keeps_exact_node_target_metadata --lib -- --nocapture' \
    'bash scripts/verify-node-full-substrate-realm.sh' \
    'Summary: 11 passed, 4 failed' \
    'R2-R5 remain open' >/tmp/nfr1-proof-missing.txt; then
  pass "NFR1 proof records exact current commands, counts, and remaining R2-R5 failures"
else
  fail "NFR1 proof artifact is incomplete" \
    "$(cat /tmp/nfr1-proof-missing.txt 2>/dev/null)"
fi

step 6 "NFR verifier is wired into helper syntax gates"
if [ -f "${MAKEFILE_PATH}" ] &&
  [ -f "${CI_WORKFLOW}" ] &&
  contains 'bash -n scripts/verify-node-full-substrate-realm.sh' "${MAKEFILE_PATH}" &&
  contains 'bash -n scripts/verify-node-full-substrate-realm.sh' "${CI_WORKFLOW}"; then
  pass "NFR verifier has Makefile and CI syntax coverage"
else
  fail "NFR verifier is not wired into proof-helper syntax gates" \
    "expected Makefile proof-helpers and CI proof-helpers to run bash -n"
fi

step 7 "NFR2 Deno realm API inventory is precise and source-backed"
if [ -f "${NFR2_PROOF}" ] &&
  [ -f "${DENO_JSRUNTIME}" ] &&
  [ -f "${DENO_JSREALM_TESTS}" ] &&
  contains_all "${NFR2_PROOF}" \
    'NFR2 Deno Realm Seam Inventory' \
    'Status: `done`' \
    '1fdb7a09a8567a827e184361b294f44f91b35e4c' \
    'v2\.8\.3-nimbus\.78' \
    'JsRuntime::create_realm' \
    'init_extension_js_in_realm' \
    'load_main_es_module_in_realm' \
    'load_side_es_module_in_realm' \
    'mod_evaluate_in_realm' \
    'resolve_in_realm' \
    'with_event_loop_promise_in_realm' \
    'poll_event_loop_in_realm' \
    'realm-scoped extension JavaScript replay' \
    'Node `process` / `module` lazy-load cycle' >/tmp/nfr2-proof-api-missing.txt &&
  contains_all "${DENO_JSRUNTIME}" \
    'pub fn create_realm' \
    'pub fn init_extension_js_in_realm' \
    'extension_replay_sources' \
    'extension_replay_js_sources' \
    'extension_replay_esm_sources' \
    'extension_replay_esm_entry_points' \
    'pub async fn load_main_es_module_in_realm' \
    'pub async fn load_side_es_module_in_realm' \
    'pub fn mod_evaluate_in_realm' \
    'pub fn resolve_in_realm' \
    'pub async fn with_event_loop_promise_in_realm' \
    'pub fn poll_event_loop_in_realm' >/tmp/nfr2-deno-api-missing.txt &&
  contains_all "${DENO_JSREALM_TESTS}" \
    'create_realm_produces_fresh_global_context_with_core_ops' \
    'create_realm_loads_modules_in_realm_module_map' \
    'init_extension_js_in_realm_replays_extension_globals' \
    'init_extension_js_in_realm_replays_snapshot_seeded_extension_globals' \
    'init_extension_js_in_realm_replays_snapshot_seeded_file_backed_extension_modules' >/tmp/nfr2-deno-tests-missing.txt &&
  contains_all "${DENO_PROCESS}" \
    'core\.createLazyLoader\("node:module"\)' \
    'function getModule\(\)' \
    'getModule\(\)\._extensions\["\.node"\]' >/tmp/nfr2-deno-process-missing.txt; then
  pass "NFR2 records available Deno realm APIs including extension replay"
else
  fail "NFR2 Deno realm API inventory is incomplete" \
    "$(cat /tmp/nfr2-proof-api-missing.txt 2>/dev/null) $(cat /tmp/nfr2-deno-api-missing.txt 2>/dev/null) $(cat /tmp/nfr2-deno-tests-missing.txt 2>/dev/null) $(cat /tmp/nfr2-deno-process-missing.txt 2>/dev/null)"
fi

step 8 "NFR2 authority-state model and semantic matrix are recorded"
if [ -f "${NFR2_PROOF}" ] &&
  contains_all "${NFR2_PROOF}" \
    'single-active-lease rebinding protocol with generation checks' \
    'RuntimePoolAuthorityKey' \
    'crates/nimbus-runtime/src/runtime/realm_lease.rs' \
    'HostCallOperation' \
    'Node20' \
    'Node22' \
    'Node24' \
    'Node26' \
    'Translator-boundary behavior preserved' \
    'Process metadata rewriting alone is insufficient' \
    'NFR2 is \*\*done\*\*' \
    'NFR4, NFR5, and NFR6 remain' >/tmp/nfr2-authority-missing.txt; then
  pass "NFR2 records the lease authority model, semantic matrix, and done outcome"
else
  fail "NFR2 authority model or blocker outcome is incomplete" \
    "$(cat /tmp/nfr2-authority-missing.txt 2>/dev/null)"
fi

step 9 "NFR2 focused verification, historical Deno tag, current pin, and REC host-session fix are recorded"
if [ -f "${NFR2_PROOF}" ] &&
  [ -f "${POOL_REUSE_TESTS}" ] &&
  contains 'v2\.8\.3-nimbus\.78' "${NFR2_PROOF}" &&
  contains '1fdb7a09a8567a827e184361b294f44f91b35e4c' "${NFR2_PROOF}" &&
  contains 'v2\.8\.3-nimbus\.79#828cd062096fc765d672f8678b8b39f9cca148c6' "${CARGO_LOCK}" &&
  contains_all "${NFR2_PROOF}" \
    "CARGO_ENCODED_RUSTFLAGS='' cargo test -p deno_core create_realm --lib -- --nocapture" \
    '2 passed; 0 failed; 0 ignored; 0 measured; 427 filtered out' \
    "CARGO_ENCODED_RUSTFLAGS='' cargo test -p deno_core init_extension_js_in_realm --lib -- --nocapture" \
    '3 passed; 0 failed' \
    "CARGO_ENCODED_RUSTFLAGS='' cargo test -p deno_node --lib" \
    '84 passed; 0 failed' \
    'cargo test -p nimbus-runtime fresh_realm --lib -- --nocapture' \
    '4 passed; 0 failed; 1 ignored; 0 measured; 1062 filtered out' \
    '`fresh:` host-call session id' \
    '`query:<function_name>` invocation session' >/tmp/nfr2-verification-missing.txt &&
  contains '__nimbusCreateContext\(\{ request \}\)' "${POOL_REUSE_TESTS}" &&
  contains '"host_call_session_id": "query:messages:first"' "${POOL_REUSE_TESTS}" &&
  ! contains 'fresh:\$\{request.function_name\}' "${POOL_REUSE_TESTS}"; then
  pass "NFR2 verification records historical Deno proof, current pin, and REC-aligned session use"
else
  fail "NFR2 verification evidence is incomplete" \
    "$(cat /tmp/nfr2-verification-missing.txt 2>/dev/null)"
fi

step 10 "NFR3 realm lease state machine is concept-owned and behavior-tested"
if [ -f "${REALM_LEASE}" ] &&
  contains_all "${REALM_LEASE}" \
    'RuntimeRealmLeaseController' \
    'RuntimeRealmLeaseContract' \
    'RuntimeRealmLeaseGeneration' \
    'RuntimePoolAuthorityKey' \
    'RuntimeRealmLeaseState' \
    'BlankSubstrate' \
    'ContractInstalled' \
    'RealmReady' \
    'BundleLoaded' \
    'Invoking' \
    'Draining' \
    'Clean' \
    'Condemned' \
    'impl Drop for RuntimeRealmLease' \
    'second_active_lease_is_rejected_per_isolate' \
    'invalid_transitions_and_double_return_are_errors' \
    'cross_tenant_checkout_is_rejected_before_contract_installation' \
    'authority_key_mismatch_is_rejected_before_contract_installation' \
    'stale_generation_return_condemns_substrate' \
    'dirty_timeout_panic_and_pressure_returns_are_non_reusable' \
    'abandoned_in_flight_lease_condemns_on_drop' \
    'owner_caps_reject_checkout_without_changing_authority_contract' \
    'authority_matching_precedes_owner_caps_and_eviction_decisions' \
    'ttl_memory_and_code_cache_budget_hooks_request_eviction' \
    'metric_labels_are_bounded_to_profile_owner_class_reason_and_decision' >/tmp/nfr3-lease-missing.txt; then
  pass "NFR3 lease module owns the state machine and required failure tests"
else
  fail "NFR3 lease module is incomplete" \
    "$(cat /tmp/nfr3-lease-missing.txt 2>/dev/null)"
fi

step 11 "NFR3 proof records exact verification and phase handoff"
if [ -f "${NFR3_PROOF}" ] &&
  contains_all "${NFR3_PROOF}" \
    'NFR3 Realm Lease State Machine Proof' \
    'Status: `done`' \
    'crates/nimbus-runtime/src/runtime/realm_lease.rs' \
    'RuntimeRealmLeaseContract' \
    'RuntimePoolAuthorityKey' \
    'RuntimeRealmLeaseGeneration' \
    'Abandoned in-flight lease condemns on drop' \
    'cargo test -p nimbus-runtime realm_lease --lib -- --nocapture' \
    '14 passed; 0 failed; 0 ignored; 0 measured; 1067 filtered out' \
    'cargo check -p nimbus-runtime --lib' \
    'Finished dev profile' \
    'NFR4 is the next phase' >/tmp/nfr3-proof-missing.txt; then
  pass "NFR3 proof records exact commands, counts, and the NFR4 handoff"
else
  fail "NFR3 proof artifact is incomplete" \
    "$(cat /tmp/nfr3-proof-missing.txt 2>/dev/null)"
fi

step 12 "NFR4 realm execution is wired through the lease contract"
if [ -f "${EXECUTION_PLAN}" ] &&
  [ -f "${DRIVER_LOADING}" ] &&
  [ -f "${DRIVER_INVOCATION}" ] &&
  [ -f "${BOOTSTRAP_STATE}" ] &&
  [ -f "${WARM_POOL}" ] &&
  [ -f "${POOL_REUSE_TESTS}" ] &&
  contains_all "${EXECUTION_PLAN}" \
    'for_realm_lease_invocation' \
    'RuntimePoolStrictAuthorityFacts' \
    'RuntimePoolBundleAuthorityFacts' \
    'RuntimeGrantsAuthorityFacts' \
    'RuntimeHostBridgeAuthorityContract::ReboundPerInvocation' \
    'realm_lease_authority_key_partitions_target_bundle_and_construction_mode' >/tmp/nfr4-execution-plan-missing.txt &&
  contains_all "${WARM_POOL}" \
    'realm_lease_controller: RuntimeRealmLeaseController' \
    'RuntimeRealmLeaseRetentionPolicy::default' >/tmp/nfr4-warm-pool-missing.txt &&
  contains_all "${BOOTSTRAP_STATE}" \
    'pub\(crate\) fn reset_runtime_contract' \
    'state\.put\(InstalledRuntimeContract \{ limits \}\)' >/tmp/nfr4-bootstrap-missing.txt &&
  contains_all "${DRIVER_LOADING}" \
    'checkout_fresh_realm_lease' \
    'RuntimeExecutionPlan::for_realm_lease_invocation' \
    'start_fresh_realm_bundle_invocation_with_lease_and_trace' \
    'reset_runtime_contract\(runtime, self, bundle\)' \
    'mark_realm_ready' \
    'mark_bundle_loaded' \
    'mark_invoking' \
    'mark_draining' \
    'return_clean_fresh_realm_lease' \
    'condemn_dirty_fresh_realm_lease' >/tmp/nfr4-loading-missing.txt &&
  contains_all "${DRIVER_INVOCATION}" \
    'start_fresh_realm_bundle_invocation_with_lease_and_reason_trace' \
    'resolve_fresh_realm_invocation_response_with_lease_and_trace' \
    'take_runtime_wait_until_pending' \
    'return_clean_fresh_realm_lease' \
    'condemn_fresh_realm_lease_with_reason' >/tmp/nfr4-invocation-missing.txt &&
  contains_all "${POOL_REUSE_TESTS}" \
    'node_full_fresh_realm_replays_extension_js_before_bundle_load' \
    'node_full_fresh_realm_lease_returns_clean_and_rejects_cross_tenant' \
    'node_full_fresh_realm_lease_enforces_target_authority_and_metadata' \
    'node_full_fresh_realm_lease_rejects_cross_bundle_reuse' \
    'node_full_fresh_realm_lease_preserves_translator_mode_boundary_per_target' \
    'node_full_fresh_realm_lease_requires_exact_service_authority_on_retained_substrate' \
    'node_full_fresh_realm_lease_applies_side_channel_hardening_per_realm' \
    'node_full_fresh_realm_lease_denies_inspector_and_repl_in_production' \
    'node_full_fresh_realm_lease_denies_query_host_effects_before_dispatch' \
    'node_full_fresh_realm_lease_matches_startup_snapshot_for_node_fixture' \
    'mainRealmSentinelType' \
    'module\.exports' \
    'authority key mismatch' \
    'runtime service grant denied for `cache`' \
    'SharedArrayBuffer' \
    'Atomics\.wait' \
    'node:inspector' \
    'node:repl' \
    'not available for query handlers' \
    'RuntimeCompatibilityTarget::Node20' \
    'RuntimeCompatibilityTarget::Node22' \
    'RuntimeCompatibilityTarget::Node24' \
    'RuntimeCompatibilityTarget::Node26' >/tmp/nfr4-tests-missing.txt; then
  pass "NFR4 lease-routed realm execution has code and focused tests"
else
  fail "NFR4 lease-routed realm execution is incomplete" \
    "$(cat /tmp/nfr4-execution-plan-missing.txt 2>/dev/null) $(cat /tmp/nfr4-warm-pool-missing.txt 2>/dev/null) $(cat /tmp/nfr4-bootstrap-missing.txt 2>/dev/null) $(cat /tmp/nfr4-loading-missing.txt 2>/dev/null) $(cat /tmp/nfr4-invocation-missing.txt 2>/dev/null) $(cat /tmp/nfr4-tests-missing.txt 2>/dev/null)"
fi

step 13 "NFR4 proof records implemented slice and remaining gates"
if [ -f "${NFR4_PROOF}" ] &&
  contains_all "${NFR4_PROOF}" \
    'NFR4 Realm-Scoped Execution Proof' \
    'Status: `done`' \
    'RuntimeRealmLeaseController' \
    'RuntimeExecutionPlan::for_realm_lease_invocation' \
    'strict reuse authority key' \
    'reset_runtime_contract' \
    'node_full_fresh_realm_lease_returns_clean_and_rejects_cross_tenant' \
    'node_full_fresh_realm_lease_enforces_target_authority_and_metadata' \
    'node_full_fresh_realm_lease_rejects_cross_bundle_reuse' \
    'node_full_fresh_realm_lease_preserves_translator_mode_boundary_per_target' \
    'node_full_fresh_realm_lease_requires_exact_service_authority_on_retained_substrate' \
    'node_full_fresh_realm_lease_applies_side_channel_hardening_per_realm' \
    'node_full_fresh_realm_lease_denies_inspector_and_repl_in_production' \
    'node_full_fresh_realm_lease_denies_query_host_effects_before_dispatch' \
    'node_full_fresh_realm_lease_matches_startup_snapshot_for_node_fixture' \
    'cargo test -p nimbus-runtime execution_plan --lib -- --nocapture' \
    '15 passed; 0 failed; 0 ignored; 0 measured; 1077 filtered out' \
    'cargo test -p nimbus-runtime node_full_fresh_realm --lib -- --nocapture' \
    '10 passed; 0 failed; 0 ignored; 0 measured; 1082 filtered out' \
    'cargo test -p nimbus-runtime warm_context_recycle --lib -- --nocapture' \
    '7 passed; 0 failed; 1 ignored; 0 measured; 1084 filtered out' \
    'cargo check -p nimbus-runtime --lib' \
    'cargo fmt --all --check' \
    'Handoff' \
    'NFR4 is done' \
    'OpState and module-cache cleanliness' \
    'NFR6 remains blocked until NFR5 closes' >/tmp/nfr4-proof-missing.txt; then
  pass "NFR4 proof records exact verification and the NFR5 handoff"
else
  fail "NFR4 proof artifact is incomplete" \
    "$(cat /tmp/nfr4-proof-missing.txt 2>/dev/null)"
fi

step 14 "NFR5 cleanup gates are started with concrete lease tests"
if [ -f "${NFR5_PROOF}" ] &&
  contains_all "${POOL_REUSE_TESTS}" \
    'node_full_fresh_realm_lease_resets_opstate_auth_host_session_and_globals' \
    'node_full_fresh_realm_lease_condemns_dirty_invocation_before_reuse' \
    'node_full_fresh_realm_lease_condemns_rejected_wait_until_before_reuse' \
    'node_full_fresh_realm_lease_drops_untracked_timer_host_work' \
    'node_full_fresh_realm_lease_denies_process_and_worker_resource_surfaces_cleanly' \
    'node_full_fresh_realm_lease_rejects_direct_core_host_op_forgery' \
    'node_full_fresh_realm_lease_condemns_live_deno_resource_table_entries' \
    'node_full_fresh_realm_lease_condemns_execution_timeout_before_reuse' \
    'node_full_fresh_realm_lease_condemns_external_cancellation_before_reuse' \
    'node_full_fresh_realm_lease_condemns_heap_limit_before_reuse' \
    'max_heap_mb = 64' \
    'node_full_fresh_realm_lease_resets_env_path_and_load_env_file_state' \
    'node_full_fresh_realm_lease_resets_arraybuffer_and_structured_clone_state' \
    'node_full_fresh_realm_lease_resets_shared_worker_env_helper_state' \
    'node_full_fresh_realm_lease_rebuilds_dynamic_module_map_per_realm' \
    'node_full_fresh_realm_lease_code_cache_reloads_changed_dependency_source' \
    'node_full_fresh_realm_lease_host_pressure_eviction_preserves_authority_partition' \
    'node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse' \
    'node_full_fresh_realm_lease_abandons_uncertain_cleanup_before_reuse' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' \
    'RuntimeMemoryPressureLevel::High' \
    'external_memory_bytes' \
    'NodeFull retained realm accounting must include V8-reported external memory' \
    'tenant-a must cold-miss instead of taking tenant-b' \
    'tenant-b should warm-hit because high pressure only evicted tenant-a' \
    'stalled waitUntil background work must not return a reusable NodeFull substrate' \
    'abandoned uncertain-cleanup substrate must reject later checkout' \
    'bootstrap::take_runtime_wait_until_pending' \
    'drain_wait_until_with_trace' \
    'previousToken' \
    'hostSession' \
    'dirty invocation failure' \
    'Nimbus waitUntil background drain rejected 1 promise' \
    'realm substrate is condemned: Dirty' \
    'late-timer-host-call' \
    'untracked timer from a destroyed lease realm must not dispatch host effects later' \
    'node:child_process' \
    'node:worker_threads' \
    'processFatalControls' \
    '__nimbusDeniedProcessFatalOperation' \
    'process.{control_name} must be guarded before it can abort or signal the host process' \
    'runtime worker grant denied for `thread`' \
    '__nimbusHiddenDenoGlobals\.core\.ops' \
    'op_nimbus_runtime_host_call_session_id' \
    'op_cancel_handle' \
    'changed Deno resource table entries' \
    'cancellation' \
    'TimedOut' \
    'ExternalPressure' \
    'NFR5_DOTENV_VALUE' \
    'process\.loadEnvFile\("\./first\.env"\)' \
    'clean retained NodeFull lease must not carry env, dotenv, or global state' \
    'sourceBufferDetached' \
    '"detachedLength": 0' \
    '"sourceViewLength": 0' \
    'ArrayBuffer backing-store state and realm globals must not leak' \
    'NFR5_SHARED_WORKER_ENV' \
    'shared worker env helper state must be reseeded' \
    'dynamic module-map entries and module-scoped state must be rebuilt' \
    'fresh-realm module code cache must not serve stale dependency bytecode' >/tmp/nfr5-tests-missing.txt &&
  contains_all "${NODE_BOOTSTRAP}" \
    'seedNodeProcessLoadEnvFile' \
    'loadEnvFileThroughNimbusHost\(nodeProcess, resolvedPath, displayPath\)' \
    'seedNodeProcessFatalGuards' \
    'nimbusProcessFatalGuardsInstalled' \
    '__nimbusDeniedProcessFatalOperation' \
    'Nimbus denies process.\${name}\(\) in embedded Node runtime' >/tmp/nfr5-node-bootstrap-missing.txt &&
  contains_all "${BOOTSTRAP_STATE}" \
    'RuntimeResourceTableSnapshot' \
    'RuntimeResourceTableBaseline' \
    'runtime_resource_table_delta' \
    'resource_table_baseline' >/tmp/nfr5-bootstrap-state-missing.txt &&
  contains_all "${DRIVER_LOADING}" \
    'start_fresh_realm_bundle_invocation_with_lease_and_reason_trace' \
    'runtime_resource_table_delta' \
    'changed Deno resource table entries' \
    'format_resource_table_delta' \
    'RuntimeRealmLeaseCondemnationReason::Dirty' >/tmp/nfr5-driver-loading-missing.txt &&
  contains_all "${DRIVER_INVOCATION}" \
    'realm_lease_condemnation_reason_classifier' \
    'RuntimeRealmLeaseCondemnationReason::TimedOut' \
    'RuntimeRealmLeaseCondemnationReason::ExternalPressure' \
    'start_fresh_realm_bundle_invocation_with_lease_and_reason_trace' >/tmp/nfr5-driver-invocation-missing.txt &&
  contains_all "${EXECUTION_PLAN}" \
    'realm_lease_authority_key_partitions_permission_grants_and_node_conditions' \
    'runtime_execution_plan_keeps_node_full_ineligible_until_realm_proof' \
    'runtime_execution_plan_for_invocation_admits_node_full_with_same_owner_realm_proof' \
    'runtime_execution_plan_keeps_node_full_ineligible_for_uv_handle_grants' \
    'runtime_execution_plan_for_realm_lease_admits_node_full_with_strict_authority' \
    'NodeFullUnproven' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' \
    'node_full_realm_reuse_policy' \
    'permits_same_process_realm_reuse' \
    'uv/native host handles' \
    'strict_reuse.is_some' \
    'NIMBUS_TOKEN_A' \
    'nimbus-custom' >/tmp/nfr5-execution-plan-missing.txt &&
  contains_all "${LIMITS_GRANTS}" \
    'permits_same_process_realm_reuse' \
    'self.net_connect.is_empty' \
    'self.net_listen.is_empty' \
    'self.run.is_empty' \
    'self.ffi.is_empty' \
    'self.worker.is_empty' \
    'grant == "inspector"' >/tmp/nfr5-limits-grants-missing.txt &&
  contains_all "${LIMITS_AXES}" \
    'pub enum RuntimeNodeFullRealmReusePolicy' \
    'Unproven' \
    'SameOwnerExactAuthority' \
    'requires same-owner exact-authority realm reuse proof' >/tmp/nfr5-limits-axes-missing.txt &&
  contains_all "${LIMITS_RESOURCES}" \
    'pub node_full_realm_reuse_policy: RuntimeNodeFullRealmReusePolicy' \
    'node_full_realm_reuse_policy: self.node_full_realm_reuse_policy' \
    'node_full_realm_reuse_policy: RuntimeNodeFullRealmReusePolicy::Unproven' >/tmp/nfr5-limits-resources-missing.txt &&
  contains_all "${LIMITS_TESTS}" \
    'warm_context_recycle_rejects_node_without_same_owner_exact_authority_realm_reuse_proof' \
    'warm_context_recycle_accepts_node_with_same_owner_exact_authority_realm_reuse_proof' \
    'requires same-owner exact-authority realm reuse proof' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' >/tmp/nfr5-limits-tests-missing.txt &&
  contains_all "crates/nimbus-runtime/src/runtime/tests/bundle_integrity.rs" \
    'RuntimeExecutionModel' \
    'V8RuntimeConstructionMode::Unsnapshotted' \
    'node_read_limits.grants.read' \
    'node_env_limits.grants.env_read' \
    'node_run_limits.grants.run' \
    'node_pool_limits.runtime_pool_kind = RuntimePoolKind::WarmContextRecycle' \
    'node_memory_limits.memory_enforcement = RuntimeMemoryEnforcement::OuterQuotaRequired' \
    'node_routing_limits.routing_affinity = RuntimeRoutingAffinity::Function' \
    'node_timeout_limits.execution_timeout' \
    'module code cache partitions must not cross authority dimension' \
    'startup_snapshot_module_code_cache_reloads_when_dependency_source_hash_changes' \
    'module code cache must not serve stale bytecode after a source-hash change' \
    'source changes should not fragment the engine cache partition' >/tmp/nfr5-code-cache-missing.txt &&
  contains_all "${RUNTIME_BUNDLE}" \
    'RuntimeBundleEngineCacheKey' \
    'construction_mode: V8RuntimeConstructionMode' \
    'read_grants: Vec<String>' \
    'env_read_grants: Vec<String>' \
    'run_grants: Vec<String>' \
    'node_full_realm_reuse_policy: RuntimeNodeFullRealmReusePolicy' \
    'routing_affinity: RuntimeRoutingAffinity' \
    'RuntimeBundleEngineCacheKey::for_limits\(limits, construction_mode\)' >/tmp/nfr5-runtime-bundle-missing.txt &&
  contains_all "${CARGO_MANIFEST}" \
    'v2\.8\.3-nimbus\.79' >/tmp/nfr5-cargo-manifest-missing.txt &&
  contains_all "${CARGO_LOCK}" \
    'v2\.8\.3-nimbus\.79#828cd062096fc765d672f8678b8b39f9cca148c6' >/tmp/nfr5-cargo-lock-missing.txt &&
  contains_all "${DENO_MESSAGE_PORT}" \
    'ArrayBufferPrototypeTransferToFixedLength' \
    'detachTransferredArrayBuffersIfNeeded' \
    'const cloned = deserializeJsMessageData\(messageData\)\[0\]' \
    'detachTransferredArrayBuffersIfNeeded\(options\.transfer\)' >/tmp/nfr5-deno-message-port-missing.txt &&
  contains_all "${DENO_STRUCTURED_CLONE_TEST}" \
    'structuredClone detaches transferred typed array backing store' \
    'assertEquals\(buffer\.byteLength, 0\)' \
    'assertEquals\(view\.byteLength, 0\)' \
    'assertEquals\(cloned\.view\.buffer\.byteLength, 16\)' >/tmp/nfr5-deno-structured-test-missing.txt &&
  contains_all "${NODE_WATCHPOINTS}" \
    'node22_buffer_isascii_watchpoint' \
    'node20_buffer_isascii_watchpoint' \
    'node22_buffer_isutf8_watchpoint' \
    'node20_buffer_isutf8_watchpoint' >/tmp/nfr5-node-watchpoints-missing.txt &&
  not_contains 'Pinned shared runtime gap: structuredClone transfer' "${NODE_WATCHPOINTS}" &&
  contains_all "${NFR5_PROOF}" \
    'NFR5 Security And Cleanliness Proof' \
    'Status: `done`' \
    'v2\.8\.3-nimbus\.79' \
    '828cd062096fc765d672f8678b8b39f9cca148c6' \
    'structured-clone transfer detachment' \
    'take_runtime_wait_until_pending' \
    'drain_wait_until_with_trace' \
    'node_full_fresh_realm_lease_resets_opstate_auth_host_session_and_globals' \
    'node_full_fresh_realm_lease_condemns_dirty_invocation_before_reuse' \
    'node_full_fresh_realm_lease_condemns_rejected_wait_until_before_reuse' \
    'node_full_fresh_realm_lease_drops_untracked_timer_host_work' \
    'node_full_fresh_realm_lease_denies_process_and_worker_resource_surfaces_cleanly' \
    'node_full_fresh_realm_lease_rejects_direct_core_host_op_forgery' \
    'node_full_fresh_realm_lease_condemns_live_deno_resource_table_entries' \
    'node_full_fresh_realm_lease_condemns_execution_timeout_before_reuse' \
    'node_full_fresh_realm_lease_condemns_external_cancellation_before_reuse' \
    'node_full_fresh_realm_lease_condemns_heap_limit_before_reuse' \
    'node_full_fresh_realm_lease_resets_env_path_and_load_env_file_state' \
    'node_full_fresh_realm_lease_resets_arraybuffer_and_structured_clone_state' \
    'node_full_fresh_realm_lease_resets_shared_worker_env_helper_state' \
    'node_full_fresh_realm_lease_rebuilds_dynamic_module_map_per_realm' \
    'node_full_fresh_realm_lease_code_cache_reloads_changed_dependency_source' \
    'node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse' \
    'node_full_fresh_realm_lease_abandons_uncertain_cleanup_before_reuse' \
    'warm_context_recycle_rejects_node_without_same_owner_exact_authority_realm_reuse_proof' \
    'runtime_execution_plan_keeps_node_full_ineligible_until_realm_proof' \
    'warm_context_recycle_accepts_node_with_same_owner_exact_authority_realm_reuse_proof' \
    'runtime_execution_plan_for_invocation_admits_node_full_with_same_owner_realm_proof' \
    'runtime_execution_plan_for_realm_lease_admits_node_full_with_strict_authority' \
    'runtime_execution_plan_keeps_node_full_ineligible_for_uv_handle_grants' \
    'node_full_fresh_realm_lease_host_pressure_eviction_preserves_authority_partition' \
    'runtime_bundle_module_code_cache_is_partitioned_by_engine_config' \
    'SameOwnerExactAuthority' \
    'operator-facing `SameOwnerExactAuthority` placement proof axis' \
    'strict bundle, construction-mode, grants, conditions' \
    'evicted tenant cold-misses' \
    'V8-reported external memory in the retained' \
    'never-settling `waitUntil` background drain' \
    'SystemTimeout' \
    'self-identifying Nimbus denial functions' \
    'V8 fatal abort is process-wide' \
    'Abandoned' \
    'uv/native host handles' \
    'same-process realm reuse is not admitted for net connect/listen' \
    'NodeFullUnproven' \
    'realm_lease_authority_key_partitions_permission_grants_and_node_conditions' \
    'startup_snapshot_module_code_cache_reloads_when_dependency_source_hash_changes' \
    'cargo test -p nimbus-runtime warm_context_recycle_rejects_node_without_same_owner_exact_authority_realm_reuse_proof --lib -- --nocapture' \
    'cargo test -p nimbus-runtime runtime_execution_plan_keeps_node_full_ineligible_until_realm_proof --lib -- --nocapture' \
    'cargo test -p nimbus-runtime warm_context_recycle_accepts_node_with_same_owner_exact_authority_realm_reuse_proof --lib -- --nocapture' \
    'cargo test -p nimbus-runtime runtime_execution_plan_for_invocation_admits_node_full_with_same_owner_realm_proof --lib -- --nocapture' \
    'cargo test -p nimbus-runtime runtime_execution_plan_for_realm_lease_admits_node_full_with_strict_authority --lib -- --nocapture' \
    'cargo test -p nimbus-runtime runtime_execution_plan_keeps_node_full_ineligible_for_uv_handle_grants --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1115 filtered out' \
    'cargo test -p nimbus-runtime node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1113 filtered out' \
    'cargo test -p nimbus-runtime node_full_fresh_realm_lease_abandons_uncertain_cleanup_before_reuse --lib -- --nocapture' \
    'cargo test -p nimbus-runtime node_full_fresh_realm_lease_denies_process_and_worker_resource_surfaces_cleanly --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1114 filtered out' \
    'cargo test -p nimbus-runtime node_full_fresh_realm_lease_host_pressure_eviction_preserves_authority_partition --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1112 filtered out' \
    '28 passed; 0 failed; 0 ignored; 0 measured; 1087 filtered out' \
    'cargo test -p nimbus-runtime node_full_fresh_realm_lease_rejects_direct_core_host_op_forgery --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1105 filtered out' \
    'cargo test -p nimbus-runtime node_full_fresh_realm_lease_condemns_live_deno_resource_table_entries --lib -- --nocapture' \
    'cargo test -p nimbus-runtime node_full_fresh_realm_lease_condemns_execution_timeout_before_reuse --lib -- --nocapture' \
    'cargo test -p nimbus-runtime node_full_fresh_realm_lease_condemns_external_cancellation_before_reuse --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1107 filtered out' \
    'cargo test -p nimbus-runtime node_full_fresh_realm_lease_condemns_heap_limit_before_reuse --lib -- --nocapture' \
    '1 passed; 0 failed; 0 ignored; 0 measured; 1108 filtered out' \
    'cargo test -p nimbus-runtime node_full_fresh_realm_lease_resets_arraybuffer_and_structured_clone_state --lib -- --nocapture' \
    'cargo test -p nimbus-runtime node_full_fresh_realm --lib -- --nocapture' \
    '25 passed; 0 failed; 0 ignored; 0 measured; 1084 filtered out' \
    'cargo test -p nimbus-runtime buffer_isascii_watchpoint --lib -- --nocapture' \
    'cargo test -p nimbus-runtime buffer_isutf8_watchpoint --lib -- --nocapture' \
    '2 passed; 0 failed; 0 ignored; 0 measured; 1107 filtered out' \
    'cargo test -p nimbus-runtime load_env_file --lib -- --nocapture' \
    '4 passed; 0 failed; 0 ignored; 0 measured; 1100 filtered out' \
    'cargo fmt --all --check' \
    'cargo check -p nimbus-runtime --lib' \
    'Finished `dev` profile' \
    'cargo test -p nimbus-runtime realm_lease_authority_key_partitions --lib -- --nocapture' \
    '2 passed; 0 failed; 0 ignored; 0 measured; 1114 filtered out' \
    'cargo test -p nimbus-runtime module_code_cache --lib -- --nocapture' \
    '3 passed; 0 failed; 0 ignored; 0 measured; 1112 filtered out' \
    'Threat Model Gate' \
    'same-owner / same-tenant' \
    'mutually hostile tenants' \
    'process or microVM isolation' \
    'Closed Gates' \
    'NFR5 is done' \
    'Code-cache safety is closed by expansion' \
    'invalidation still protects changed dependency bytes' \
    'ArrayBuffer structured-clone transfer detachment is now fixed' \
    'External-memory accounting is covered' \
    'Uv/native host-handle cleanup is closed by admission policy' \
    'host-pressure eviction are proved' \
    'Public Node `WarmContextRecycle` remains fail-closed by default' \
    'explicit `SameOwnerExactAuthority` proof axis' >/tmp/nfr5-proof-missing.txt &&
  contains_all "${PLAN}" \
    'v2\.8\.3-nimbus\.79' \
    '828cd062096fc765d672f8678b8b39f9cca148c6' \
    'NFR5 initial security/cleanliness slice' \
    'child-process plus worker-thread resource surfaces are denied cleanly' \
    'dependency source change reloads fresh cached data' \
    'NFR5 env/path and structured-clone cleanup slice' \
    '`process.loadEnvFile\(\)` values persisted into the next fresh realm' \
    'capability-checked file reader/parser' \
    'shared-worker-env helper test' \
    'untracked timer-host-work test' \
    'dynamic module-map test' \
    '20 passed, 0 failed, 0 ignored, 0 measured, 1084 filtered out' \
    'NFR5 Deno resource-table and direct-op cleanup slice' \
    '__nimbusHiddenDenoGlobals.core.ops' \
    'op_cancel_handle\(\)' \
    'cancellation' \
    '22 passed, 0 failed, 0 ignored, 0 measured, 1084 filtered out' \
    'NFR5 placement fail-closed evidence' \
    'warm_context_recycle_rejects_node_without_same_owner_exact_authority_realm_reuse_proof' \
    'runtime_execution_plan_keeps_node_full_ineligible_until_realm_proof' \
    'NodeFullUnproven' \
    '`WarmContextRecycle` remains rejected while NFR5 is still open' \
    'NFR5 timeout and cancellation condemnation slice' \
    'driver-owned failure' \
    'TimedOut' \
    'ExternalPressure' \
    'node_full_fresh_realm_lease_condemns_execution_timeout_before_reuse' \
    'node_full_fresh_realm_lease_condemns_external_cancellation_before_reuse' \
    'NFR5 heap-limit condemnation slice' \
    'node_full_fresh_realm_lease_condemns_heap_limit_before_reuse' \
    '64 MB cap' \
    'HeapLimitExceeded' \
    '1 passed, 0 failed, 0 ignored, 0 measured, 1108 filtered out' \
    'NFR5 structured-clone transfer detachment closure' \
    'source buffer and source typed-array view detach to length `0`' \
    'buffer_isascii_watchpoint' \
    'buffer_isutf8_watchpoint' \
    '25 passed, 0 failed, 0 ignored, 0 measured, 1084 filtered out' \
    '4 passed, 0 failed, 0 ignored, 0 measured, 1100 filtered out' \
    'cargo check -p nimbus-runtime --lib` also passed' \
    '2 passed, 0 failed, 0 ignored, 0 measured, 1102 filtered out' \
    '3 passed, 0 failed, 0 ignored, 0 measured, 1101 filtered out' \
    'source-hash guarded against stale dependency bytecode' \
    'same-owner / same-tenant exact-authority reuse' \
    'hostile tenant placement must route to process or microVM isolation' \
    'NFR5 remains in progress until full realm-path code-cache' \
    'NFR5 host-pressure and same-owner placement admission slice' \
    'NFR5 fatal-control and uncertain-cleanup closure slice' \
    'NFR5 code-cache and uv/native-handle admission closure' \
    'RuntimeNodeFullRealmReusePolicy' \
    'SameOwnerExactAuthority' \
    'node_full_fresh_realm_lease_host_pressure_eviction_preserves_authority_partition' \
    'node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse' \
    'node_full_fresh_realm_lease_abandons_uncertain_cleanup_before_reuse' \
    'never-settling `waitUntil` drain' \
    'SystemTimeout' \
    'process.abort\(\)' \
    'process.kill\(\)' \
    'self-identifying Nimbus denial' \
    'V8 fatal abort remains' \
    'Abandoned' \
    'RuntimeBundleEngineCacheKey' \
    'V8RuntimeConstructionMode' \
    'RuntimeGrants::permits_same_process_realm_reuse' \
    'uv/native-handle-producing workloads route away from' \
    'runtime_execution_plan_keeps_node_full_ineligible_for_uv_handle_grants' \
    '1115 filtered out' \
    'older tenant-affine NodeFull' \
    'evicted tenant cold-misses instead of borrowing' \
    'non-evicted exact-authority tenant' \
    'warm-hits' \
    'V8-reported external memory in the retained-memory budget' \
    '1113 filtered out' \
    '1114 filtered out' \
    '1112 filtered out' \
    '28 passed, 0 failed, 0 ignored, 0 measured, 1087 filtered out' \
    'NFR5 is now done and promoted NFR6' >/tmp/nfr5-plan-missing.txt; then
  pass "NFR5 cleanup proof is closed and backed by focused tests"
else
  fail "NFR5 cleanup proof is incomplete" \
    "$(cat /tmp/nfr5-tests-missing.txt 2>/dev/null) $(cat /tmp/nfr5-node-bootstrap-missing.txt 2>/dev/null) $(cat /tmp/nfr5-bootstrap-state-missing.txt 2>/dev/null) $(cat /tmp/nfr5-driver-loading-missing.txt 2>/dev/null) $(cat /tmp/nfr5-driver-invocation-missing.txt 2>/dev/null) $(cat /tmp/nfr5-execution-plan-missing.txt 2>/dev/null) $(cat /tmp/nfr5-limits-grants-missing.txt 2>/dev/null) $(cat /tmp/nfr5-limits-axes-missing.txt 2>/dev/null) $(cat /tmp/nfr5-limits-resources-missing.txt 2>/dev/null) $(cat /tmp/nfr5-limits-tests-missing.txt 2>/dev/null) $(cat /tmp/nfr5-code-cache-missing.txt 2>/dev/null) $(cat /tmp/nfr5-runtime-bundle-missing.txt 2>/dev/null) $(cat /tmp/nfr5-cargo-manifest-missing.txt 2>/dev/null) $(cat /tmp/nfr5-cargo-lock-missing.txt 2>/dev/null) $(cat /tmp/nfr5-deno-message-port-missing.txt 2>/dev/null) $(cat /tmp/nfr5-deno-structured-test-missing.txt 2>/dev/null) $(cat /tmp/nfr5-node-watchpoints-missing.txt 2>/dev/null) $(cat /tmp/nfr5-proof-missing.txt 2>/dev/null) $(cat /tmp/nfr5-plan-missing.txt 2>/dev/null)"
fi

step 15 "NFR6 benchmark artifact records an evidence-backed rejection"
NFR6_ARTIFACT_ROWS=0
if [ -f "${NFR6_BENCH_ARTIFACT}" ]; then
  NFR6_ARTIFACT_ROWS=$(grep -c '"schema":"nimbus.node_full_substrate_realm.nfr6.benchmark.v1"' "${NFR6_BENCH_ARTIFACT}" 2>/dev/null || true)
fi
if [ -f "${RUNTIME_POOL_BENCH}" ] &&
  [ -f "${NFR6_PROOF}" ] &&
  [ -f "${NFR6_BENCH_ARTIFACT}" ] &&
  [ "${NFR6_ARTIFACT_ROWS}" -ge 36 ] &&
  contains_all "${RUNTIME_POOL_BENCH}" \
    'NFR6_TRACE_SCHEMA' \
    'nimbus.node_full_substrate_realm.nfr6.benchmark.v1' \
    'NodeFullNfr6WorkloadKind' \
    'runtime_pool_modes_nfr6_node_full_realm' \
    'NIMBUS_NFR6_TRACE_PATH' \
    'NIMBUS_NFR6_PROFILE' \
    'NIMBUS_NFR6_POOL_MODE' \
    'NIMBUS_NFR6_WORKLOAD' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' \
    'node24_cjs_translator_boundary' \
    'latency_p50_nanos' \
    'latency_p95_nanos' \
    'latency_p99_nanos' \
    'observed_dirty_return_count' \
    'observed_condemn_count' >/tmp/nfr6-bench-missing.txt &&
  contains_all "${NFR6_PROOF}" \
    'NFR6 Benchmark And Adoption Decision' \
    'Status: `done`' \
    'Node `WarmContextRecycle` / NodeFull realm pooling is rejected' \
    'RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority' \
    'runtime_pool_modes_nfr6_node_full_realm' \
    'nimbus.node_full_substrate_realm.nfr6.benchmark.v1' \
    'nfr6-node-full-realm-current-rss.jsonl' \
    'raw file has 199 rows' \
    '36 final rows' \
    'cargo check -p nimbus-runtime --benches' \
    'cargo bench -p nimbus-runtime --bench runtime_pool_modes -- runtime_pool_modes_nfr6_node_full_realm --sample-size 10 --measurement-time 1 --warm-up-time 1' \
    'Finished `bench` profile \[optimized\] target\(s\) in 5m 44s' \
    '5\.38x\.\.10\.01x' \
    '13\.35x\.\.16\.13x' \
    '44\.28\.\.69\.33 MiB' \
    'Dirty-return count was `0`' \
    'Observed condemnation count was `0`' \
    'single-host macOS ARM64' \
    'do not make `WarmContextRecycle` the NodeFull default' >/tmp/nfr6-proof-missing.txt &&
  contains_all "${NFR6_BENCH_ARTIFACT}" \
    '"profile":"node20"' \
    '"profile":"node22"' \
    '"profile":"node24"' \
    '"profile":"node26"' \
    '"workload":"setup_heavy_large_module"' \
    '"workload":"loader_hook_dynamic_builtin"' \
    '"workload":"node24_cjs_translator_boundary"' \
    '"pool_kind":"startup_snapshot_cache"' \
    '"pool_kind":"warm_pool"' \
    '"pool_kind":"warm_context_recycle"' \
    '"observed_dirty_return_count":0' \
    '"observed_condemn_count":0' >/tmp/nfr6-artifact-missing.txt &&
  contains_all "${PLAN}" \
    '\| NFR6 \| `done`' \
    'NFR6 benchmark and adoption decision' \
    'runtime_pool_modes_nfr6_node_full_realm' \
    'nfr6-node-full-realm-current-rss.jsonl' \
    '5\.38x\.\.10\.01x' \
    '13\.35x\.\.16\.13x' \
    '44\.28\.\.69\.33 MiB' \
    'NFR6 rejects NodeFull realm pooling' \
    'do not make' >/tmp/nfr6-plan-missing.txt; then
  pass "NFR6 benchmark matrix is recorded and rejects adoption"
else
  fail "NFR6 benchmark/adoption proof is incomplete" \
    "rows=${NFR6_ARTIFACT_ROWS}; $(cat /tmp/nfr6-bench-missing.txt 2>/dev/null) $(cat /tmp/nfr6-proof-missing.txt 2>/dev/null) $(cat /tmp/nfr6-artifact-missing.txt 2>/dev/null) $(cat /tmp/nfr6-plan-missing.txt 2>/dev/null)"
fi

printf '\nSummary: %d passed, %d failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailing conditions:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
