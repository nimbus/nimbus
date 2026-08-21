#!/usr/bin/env bash
# Fixed 16-condition verifier for incremental materialized verification.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../../../../.." && pwd)"
cd "$ROOT" || { echo "cannot cd to repository root"; exit 2; }

PROOF="docs/private/plans/proof/incremental-materialized-verification"
PLAN="docs/private/plans/incremental-materialized-verification-plan.md"
BASELINE="137cc632a1c8585545d200ea49f44bd236478175"
pass=0
fail=0

ok() { printf '  PASS  %s\n' "$1"; pass=$((pass + 1)); }
no() { printf '  FAIL  %s  [%s]\n' "$1" "$2"; fail=$((fail + 1)); }
grok() { [ -f "$1" ] && grep -qF -- "$2" "$1"; }

test_ok() {
  local krate="$1" filter="$2"
  shift 2
  if ! rg -q -- "$filter" crates; then
    return 1
  fi
  local out status ran
  out="$(cargo test -p "$krate" "$@" "$filter" -- --nocapture 2>&1)"
  status=$?
  ran="$(printf '%s\n' "$out" | grep -cE "^test .*${filter}.* \.\.\. ok$")"
  [ "$status" -eq 0 ] && [ "$ran" -ge 1 ]
}

ignored_integration_test_ok() {
  local krate="$1" target="$2" filter="$3"
  local out status ran
  out="$(cargo test -p "$krate" --test "$target" "$filter" -- --ignored --nocapture 2>&1)"
  status=$?
  ran="$(printf '%s\n' "$out" | grep -cE "^test .*${filter}.* \.\.\. ok$")"
  [ "$status" -eq 0 ] && [ "$ran" -ge 1 ]
}

streaming_verdict=false
if grok "$PROOF/imv2-verdict.md" "STREAMING_ACCEPTED" \
  && grep -qE '^\| IMV[3-6] \|.*\| `rejected\(IMV2 measurement gate\)` \|' "$PLAN"; then
  rejected_count="$(grep -cE '^\| IMV[3-6] \|.*\| `rejected\(IMV2 measurement gate\)` \|' "$PLAN")"
  [ "$rejected_count" -eq 4 ] && streaming_verdict=true
fi

echo "incremental materialized verification"
echo "====================================="

# 1. The pinned baseline and task ancestry are present.
if git cat-file -e "$BASELINE^{commit}" 2>/dev/null \
  && git merge-base --is-ancestor "$BASELINE" HEAD \
  && grok "$PROOF/README.md" "$BASELINE"; then
  ok "1. pinned baseline exists and is an ancestor of the task branch"
else
  no "1. pinned baseline" "baseline, ancestry, or retained attribution is missing"
fi

# 2. Both resolved serde_json feature graphs are retained.
if grok "$PROOF/imv0.md" 'storage-only graph' \
  && grok "$PROOF/imv0.md" 'does not resolve `preserve_order`' \
  && grok "$PROOF/imv0.md" 'shipped engine graph resolves `serde_json/preserve_order`'; then
  ok "2. storage-only and shipped serde_json feature graphs are recorded"
else
  no "2. Cargo feature graphs" "one resolved graph or its preserve_order result is missing"
fi

# 3. Canonical logical values, total floats, and both graph goldens pass.
if test_ok nimbus-storage canonical_leaf_equivalent_stored_values_hash_identically \
  && test_ok nimbus-storage canonical_leaf_nan_and_positive_infinity_do_not_collide \
  && test_ok nimbus-storage materialized_position_golden_matches_storage_graph \
  && test_ok nimbus-engine materialized_position_golden_matches_shipped_graph; then
  ok "3. canonical values, total floats, and cross-graph goldens pass"
else
  no "3. canonical values and graph goldens" "IMV1 contract tests are absent or failing"
fi

# 4. PITR rejects an invalid position before the first destination write.
if test_ok nimbus-storage pitr_rejects_invalid_target_position_before_first_write; then
  ok "4. PITR position preflight runs before destination writes"
else
  no "4. PITR preflight" "the named preflight regression is absent or failing"
fi

# 5. The streamed digest matches its retained reference implementation.
if test_ok nimbus-storage streaming_materialized_digest_matches_reference; then
  ok "5. streaming materialized digest matches the reference"
else
  no "5. streaming digest equivalence" "the named equivalence test is absent or failing"
fi

# 6. The retained benchmark target emits a valid quick baseline.
if python3 - "$PROOF/imv0-raw.json" "$BASELINE" <<'PY'
import json
import sys

path, baseline = sys.argv[1:]
try:
    report = json.load(open(path))
    measurement, = report["measurements"]
    valid = (
        report["format_version"] == 1
        and report["baseline_commit"] == baseline
        and report["mode"] == "full"
        and report["quick"] is True
        and measurement["documents"] == 10_000
        and measurement["payload_bytes"] == 1_024
        and measurement["elapsed_ns"] > 0
        and measurement["process_cpu_ns"] > 0
        and measurement["allocation_count"] > 0
        and measurement["allocated_bytes"] > 0
        and measurement["peak_rss_bytes"] > 0
        and measurement["bytes_read"] is None
        and measurement["bytes_read_status"].startswith("UNVERIFIED:")
        and measurement["report_ok"] is True
        and measurement["mismatch_count"] == 0
        and measurement["authoritative_document_count"] == 10_000
    )
except (OSError, KeyError, TypeError, ValueError):
    valid = False
sys.exit(0 if valid else 1)
PY
then
  ok "6. quick full-verifier benchmark emits complete stable JSON"
else
  no "6. benchmark evidence" "imv0-raw.json is missing or invalid"
fi

# 7. IMV2 records a complete literal continuation verdict.
if python3 - "$PROOF/imv2-raw.json" <<'PY' \
  && [ -f "$PROOF/imv2-verdict.md" ] \
  && grep -qE 'STREAMING_ACCEPTED|MERKLE_REQUIRED|NO_ACCEPTABLE_DESIGN' "$PROOF/imv2-verdict.md" \
  && grok "$PROOF/imv2-verdict.md" "measured margin"; then
import itertools
import json
import sys

try:
    report = json.load(open(sys.argv[1]))
    expected = set(itertools.product(
        (10_000, 100_000, 1_000_000),
        (256, 1_024, 8 * 1_024),
        (0, 10, 100, 1_000),
    ))
    matrix = report["matrix"]
    actual = {
        (row["documents"], row["payload_bytes"], row["churn_basis_points"])
        for row in matrix
    }
    decisive = next(
        row for row in matrix
        if (row["documents"], row["payload_bytes"], row["churn_basis_points"])
        == (100_000, 1_024, 10)
    )
    million = next(
        row for row in matrix
        if (row["documents"], row["payload_bytes"], row["churn_basis_points"])
        == (1_000_000, 1_024, 10)
    )
    valid = (
        report["format_version"] == 2
        and report["interval_seconds"] == 60
        and report["churn_setup_budget_seconds"] == 120
        and len(matrix) == 36
        and actual == expected
        and report["write_overhead"] is not None
        and decisive["churn_setup_status"] == "measured"
        and decisive["full"]["summary"]["p95_ns"] > 1_000_000_000
        and decisive["candidate"]["resident_bytes_per_leaf"] <= 192
        and million["churn_setup_status"] == "measured"
        and million["full"]["censored_lower_bound_summary"]["p95_ns"]
            >= 15_000_000_000
    )
except (OSError, KeyError, StopIteration, TypeError, ValueError):
    valid = False
sys.exit(0 if valid else 1)
PY
  ok "7. IMV2 raw measurements and continuation verdict are recorded"
else
  no "7. IMV2 verdict" "raw matrix, literal verdict, or measured margin is missing"
fi

if $streaming_verdict; then
  for number in 8 9 10 11 12 13 14; do
    ok "$number. Merkle-only contract is not applicable (STREAMING_ACCEPTED; IMV3-IMV6 rejected)"
  done
else
  if test_ok nimbus-storage batch_and_incremental_verification_roots_match \
    && test_ok nimbus-storage verification_root_is_independent_of_update_order \
    && test_ok nimbus-storage delete_then_reinsert_restores_root \
    && test_ok nimbus-storage verification_root_version_separates_formats \
    && ignored_integration_test_ok nimbus-storage generated_history verification_root \
    && grok "$PROOF/imv3.md" "max_depth=55" \
    && grok "$PROOF/imv3.md" "budgeted_bytes_per_leaf=164" \
    && grok "$PROOF/imv3.md" "Dependency screen"; then
    ok "8. deterministic versioned incremental root contract passes"
  else
    no "8. incremental root" "IMV3 root tests are absent or failing"
  fi

  if test_ok nimbus-storage root_advances_with_applied_sequence \
    && test_ok nimbus-storage failed_apply_does_not_advance_root \
    && test_ok nimbus-storage replay_duplicate_keeps_root \
    && test_ok nimbus-storage apply_gap_invalidates_verification_index \
    && test_ok nimbus-storage local_provider_apply_paths_publish_only_post_apply_deltas \
    && test_ok nimbus-storage document_insert_update_delete_deltas_match_full_rebuilds \
    && test_ok nimbus-storage hidden_lineage_document_write_matches_provider_activation \
    && test_ok nimbus-storage schema_scheduler_and_lifecycle_records_have_safe_verification_effects \
    && test_ok nimbus-storage snapshot_restore_invalidates_local_verification_generations \
    && test_ok nimbus-storage overlapping_replacements_share_one_non_current_generation \
    && test_ok nimbus-engine object_manifest_commit_updates_verification_root \
    && test_ok nimbus-storage durable_head_ahead_of_apply_does_not_advance_verification_root \
    && test_ok nimbus-storage libsql_replica_refresh_invalidates_stale_verification_root --features libsql \
    && test_ok nimbus-storage all_storage_writers_declare_their_materialized_verification_effect \
    && test_ok nimbus-storage materialized_serving_surfaces_have_no_verification_root_authority \
    && grok AGENTS.md 'Object metadata writes use a sequenced internal committer'; then
    ok "9. every materialized writer updates or invalidates after apply"
  else
    no "9. writer-owned deltas" "IMV4 writer tests are absent or failing"
  fi

  if test_ok nimbus-storage materialized_verification_root_is_provider_independent; then
    ok "10. verification root is provider independent"
  else
    no "10. provider parity" "the provider corpus is absent or failing"
  fi

  if test_ok nimbus-engine unchanged_recheck_reads_no_full_snapshot \
    && test_ok nimbus-engine bounded_sessions_evict_least_recently_used \
    && test_ok nimbus-engine verification_session_limit_refuses_when_every_slot_is_active \
    && test_ok nimbus-engine consistency_verification_process_restart_requires_full_scrub; then
    ok "11. verification sessions reuse state and remain bounded"
  else
    no "11. session behavior" "IMV5 reuse or eviction tests are absent or failing"
  fi

  if test_ok nimbus-storage corrupt_index_never_reports_success \
    && test_ok nimbus-storage full_scrub_detects_state_tamper_at_same_sequence \
    && test_ok nimbus-engine \
      consistency_full_scrub_rejects_persistent_same_sequence_provider_tamper; then
    ok "12. corruption and provider tamper fail closed"
  else
    no "12. fail-closed fallback" "IMV6 corruption tests are absent or failing"
  fi

  if test_ok nimbus-engine incremental_verifier_reports_mode_and_anchor \
    && test_ok nimbus-engine root_mismatch_escalates_to_full_scrub \
    && test_ok nimbus-engine retention_gap_rebuilds_session; then
    ok "13. every fast result names an anchor and unsafe states scrub or rebuild"
  else
    no "13. anchor and escalation" "IMV5 anchor or escalation tests are absent or failing"
  fi

  if test_ok nimbus-storage verification_metrics_have_bounded_labels \
    && test_ok nimbus-server tenant_consistency_route_returns_green_report_for_live_state; then
    ok "14. incremental verification metrics are bounded"
  else
    no "14. incremental metrics" "the bounded-label regression is absent or failing"
  fi
fi

# 15. Final matched performance proves the accepted branch still meets budget.
if [ -f "$PROOF/imv7-performance.md" ] \
  && grok "$PROOF/imv7-performance.md" "accepted verdict" \
  && grep -qE 'STREAMING_ACCEPTED|MERKLE_REQUIRED' "$PROOF/imv7-performance.md"; then
  ok "15. final matched performance proves the accepted IMV2 branch"
else
  no "15. closeout performance" "the final matched-run proof is missing"
fi

# 16. Architecture and operating docs state the accepted assurance contract.
if grok "docs/private/operating/verification.md" "full scrub" \
  && grok "docs/private/operating/verification.md" "incremental" \
  && grok "docs/private/operating/verification.md" "shipped binary dependency graph" \
  && grok "docs/private/architecture/time-and-ordering.md" "VerificationPosition"; then
  ok "16. operating and architecture docs state the accepted verification contract"
else
  no "16. documentation" "full scrub, incremental assurance, shipped graph, or position text is missing"
fi

echo "====================================="
echo "Summary: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
