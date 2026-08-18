#!/usr/bin/env bash
# verify.sh — verifier for the storage integrity contracts campaign (SIC).
#
# Thirteen fixed conditions. The set never grows or shrinks: each task turns a
# known-red condition green, and the campaign closes at 13 passed, 0 failed.
#
# Conditions are non-vacuous by construction. A source condition names a symbol
# that does not exist yet, so it cannot pass by accident. A test condition runs
# the named test and requires that the test actually executed: a filter that
# matches nothing fails rather than reporting success, which is how a renamed or
# deleted test would otherwise pass this gate silently.
#
#   bash docs/private/plans/proof/storage-integrity-contracts/verify.sh
#
# Environment:
#   SIC_SKIP_TESTS=1   report every test condition as failed without running
#                      cargo. Source conditions still run. Use only to inspect
#                      the source half quickly; never as campaign evidence.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../../../../.." && pwd)"
cd "$ROOT" || { echo "cannot cd to repo root"; exit 2; }

STORAGE_OBJECT_META="crates/nimbus-storage/src/traits/object_metadata.rs"
ENGINE_OBJECTS="crates/nimbus-engine/src/engine/objects.rs"
S3_BACKEND="crates/nimbus-s3/src/backend.rs"
S3_SERVICE="crates/nimbus-s3/src/service.rs"
ENGINE_VERIFICATION="crates/nimbus-engine/src/verification.rs"
STORAGE_SNAPSHOT="crates/nimbus-storage/src/store/journal_snapshot.rs"

pass=0
fail=0
ok() { printf '  PASS  %s\n' "$1"; pass=$((pass + 1)); }
no() { printf '  FAIL  %s  [%s]\n' "$1" "$2"; fail=$((fail + 1)); }

# Fixed-string grep inside a file that must exist.
grok() { [ -f "$1" ] && grep -qF -- "$2" "$1"; }
# Extended-regex grep inside a file that must exist.
grer() { [ -f "$1" ] && grep -qE -- "$2" "$1"; }

# run_test <label> <crate> <test-filter> [extra cargo args...]
#
# Requires both that the run succeeded and that at least one test line matching
# the filter reported `ok`. A filter matching zero tests is a failure.
run_test() {
  local label="$1" krate="$2" filter="$3"
  shift 3
  if [ "${SIC_SKIP_TESTS:-0}" = 1 ]; then
    echo "  FAIL  $label  [SIC_SKIP_TESTS=1: not evaluated]"
    fail=$((fail + 1))
    return
  fi
  local out status
  out="$(cargo test -q -p "$krate" "$@" "$filter" -- --nocapture 2>&1)"
  status=$?
  local ran
  ran="$(printf '%s\n' "$out" | grep -cE "^test .*${filter}.* \.\.\. ok$")"
  if [ "$status" -ne 0 ]; then
    no "$label" "cargo test -p $krate $filter exited $status"
  elif [ "$ran" -eq 0 ]; then
    no "$label" "no test matching '$filter' ran in $krate (vacuous filter)"
  else
    ok "$label ($ran test(s))"
  fi
}

echo "storage integrity contracts verification"
echo "========================================"

# ---------------------------------------------------------------- SIC1 (1-5)

# 1. A typed object expected-state condition exists in the storage seam, with a
#    typed committed-or-rejected outcome. No `Default`: an omitted condition
#    must not silently mean "unconditional".
if grok "$STORAGE_OBJECT_META" "pub enum ObjectExpectedState" \
  && grok "$STORAGE_OBJECT_META" "pub enum ObjectConditionOutcome" \
  && ! grok "$STORAGE_OBJECT_META" "impl Default for ObjectExpectedState"; then
  ok "1. typed ObjectExpectedState + ObjectConditionOutcome live in the storage seam"
else
  no "1. typed object condition types" "ObjectExpectedState/ObjectConditionOutcome absent from $STORAGE_OBJECT_META"
fi

# 2. The condition crosses `S3ObjectMeta` rather than being decided above it.
#    The S3 surface must no longer pre-read and then write unconditionally.
if grok "$S3_BACKEND" "ObjectExpectedState" \
  && grok "$S3_BACKEND" "put_manifest_conditional" \
  && ! grok "$S3_SERVICE" "verify_write_preconditions"; then
  ok "2. the condition crosses S3ObjectMeta and the S3 pre-read decision is gone"
else
  no "2. condition crosses S3ObjectMeta" "put_manifest_conditional missing from $S3_BACKEND, or $S3_SERVICE still calls verify_write_preconditions"
fi

# 3. The committer actor evaluates the condition against its own read of the
#    current document, before it assigns a sequence.
if grok "$ENGINE_OBJECTS" "evaluate_object_condition" \
  && python3 - "$ENGINE_OBJECTS" <<'PY'
import sys
src = open(sys.argv[1]).read()
i = src.find("fn commit_object_meta_write_in_actor")
if i < 0:
    sys.exit(1)
body = src[i:]
cond = body.find("evaluate_object_condition")
seq = body.find("let sequence = SequenceNumber")
sys.exit(0 if 0 <= cond < seq else 1)
PY
then
  ok "3. the committer actor decides the condition before sequence assignment"
else
  no "3. actor decides before sequencing" "evaluate_object_condition absent from, or ordered after sequence assignment in, $ENGINE_OBJECTS"
fi

# 4. Sequential and concurrent conditional probes both pass. The sequential
#    probe is celld's four-write shape; the concurrent probe is the one that
#    can see the race at all.
if [ "${SIC_SKIP_TESTS:-0}" = 1 ]; then
  echo "  FAIL  4. sequential + concurrent conditional probes  [SIC_SKIP_TESTS=1: not evaluated]"
  fail=$((fail + 1))
else
  out4="$(cargo test -q -p nimbus-s3 conditional_ -- --nocapture 2>&1)"
  st4=$?
  seq_ok="$(printf '%s\n' "$out4" | grep -cE '^test .*conditional_put_probe_create_reject_update_reject_stale.* \.\.\. ok$')"
  lin_ok="$(printf '%s\n' "$out4" | grep -cE '^test .*conditional_put_if_none_match_is_linearizable.* \.\.\. ok$')"
  if [ "$st4" -eq 0 ] && [ "$seq_ok" -ge 1 ] && [ "$lin_ok" -ge 1 ]; then
    ok "4. sequential + concurrent conditional probes pass"
  else
    no "4. sequential + concurrent conditional probes" "conditional_put_probe_create_reject_update_reject_stale=$seq_ok conditional_put_if_none_match_is_linearizable=$lin_ok exit=$st4"
  fi
fi

# 5. A rejected condition has no sequence, journal, fan-out, or retained-blob
#    effect, and cleanup never deletes bytes a committed manifest still holds.
run_test "5. rejection has no commit or blob effect" \
  nimbus-s3 rejected_object_condition_has_no_commit_or_blob_effect

# ------------------------------------------------------------------ SIC2 (6)

# 6. Parallel UploadPart of distinct part numbers loses no accepted part.
run_test "6. concurrent multipart writes preserve every accepted part" \
  nimbus-s3 concurrent_upload_parts_preserve_all_accepted_parts

# ---------------------------------------------------------------- SIC3 (7-8)

# 7. Every client and internal storage writer is inventoried in one checked
#    ownership matrix, not just the three composite SQL commit paths.
run_test "7. all storage writers are inventoried" \
  nimbus-storage all_storage_writers_declare_their_commit_effects

# 8. An effect cannot be omitted through a default, an Option, or an opaque
#    callback: omitting one in a fixture fails the ownership gate.
run_test "8. an omitted effect fails the ownership gate" \
  nimbus-storage omitted_commit_effect_fails_the_ownership_matrix

# --------------------------------------------------------------- SIC4 (9-11)

# 9. Storage owns one canonical digest. The engine's parallel canonicalizer is
#    gone, and the materialized position type exists in storage.
if grok "$STORAGE_SNAPSHOT" "pub struct MaterializedPosition" \
  && ! grok "$ENGINE_VERIFICATION" "fn canonicalize_materialized_journal_snapshot"; then
  ok "9. storage owns the one canonical digest implementation"
else
  no "9. one canonical digest" "MaterializedPosition missing from $STORAGE_SNAPSHOT, or the engine canonicalizer still exists in $ENGINE_VERIFICATION"
fi

# 10. The position separates equal sequences with different state, and is
#     stable under logical reordering.
if [ "${SIC_SKIP_TESTS:-0}" = 1 ]; then
  echo "  FAIL  10. divergence + ordering tests  [SIC_SKIP_TESTS=1: not evaluated]"
  fail=$((fail + 1))
else
  out10="$(cargo test -q -p nimbus-storage materialized_position -- --nocapture 2>&1)"
  st10=$?
  div_ok="$(printf '%s\n' "$out10" | grep -cE '^test .*same_sequence_different_state_has_different_materialized_position.* \.\.\. ok$')"
  ord_ok="$(printf '%s\n' "$out10" | grep -cE '^test .*logical_order_does_not_change_materialized_position.* \.\.\. ok$')"
  if [ "$st10" -eq 0 ] && [ "$div_ok" -ge 1 ] && [ "$ord_ok" -ge 1 ]; then
    ok "10. divergence + ordering tests pass"
  else
    no "10. divergence + ordering tests" "divergence=$div_ok ordering=$ord_ok exit=$st10"
  fi
fi

# 11. Every materialized-artifact consumer binds to the position, not to a bare
#     sequence: shadow recovery and PITR import both reject a wrong digest.
if [ "${SIC_SKIP_TESTS:-0}" = 1 ]; then
  echo "  FAIL  11. materialized consumers bind the position  [SIC_SKIP_TESTS=1: not evaluated]"
  fail=$((fail + 1))
else
  out11a="$(cargo test -q -p nimbus-engine shadow_recovery_rejects_wrong_checkpoint_digest -- --nocapture 2>&1)"
  st11a=$?
  out11b="$(cargo test -q -p nimbus-storage pitr_import_rejects_wrong_target_digest -- --nocapture 2>&1)"
  st11b=$?
  a_ok="$(printf '%s\n' "$out11a" | grep -cE '^test .*shadow_recovery_rejects_wrong_checkpoint_digest.* \.\.\. ok$')"
  b_ok="$(printf '%s\n' "$out11b" | grep -cE '^test .*pitr_import_rejects_wrong_target_digest.* \.\.\. ok$')"
  if [ "$st11a" -eq 0 ] && [ "$st11b" -eq 0 ] && [ "$a_ok" -ge 1 ] && [ "$b_ok" -ge 1 ]; then
    ok "11. shadow recovery and PITR import both bind the position"
  else
    no "11. materialized consumers bind the position" "shadow=$a_ok pitr=$b_ok exits=$st11a/$st11b"
  fi
fi

# ----------------------------------------------------------------- SIC5 (12)

# 12. Every provider has a complete, non-skipping semantic qualification row.
#     Run with all provider features so the matrix cannot be complete only
#     because a provider was compiled out.
run_test "12. provider qualification matrix is complete" \
  nimbus-storage provider_contract_matrix_is_complete --features libsql,mysql,postgres

# ----------------------------------------------------------------- SIC6 (13)

# 13. Test-only physical SQLite durability faults preserve the last
#     acknowledged position across disk-full, sync/WAL failure, and process
#     loss, and the production binary gains no fault seam.
if [ "${SIC_SKIP_TESTS:-0}" = 1 ]; then
  echo "  FAIL  13. physical SQLite durability faults  [SIC_SKIP_TESTS=1: not evaluated]"
  fail=$((fail + 1))
else
  out13="$(cargo test -q -p nimbus-storage sqlite_physical_durability -- --nocapture 2>&1)"
  st13=$?
  cases=0
  for case_name in \
    sqlite_disk_full_preserves_last_acknowledged_position \
    sqlite_sync_failure_is_not_acknowledged \
    sqlite_crash_after_durable_commit_recovers_matching_position \
    sqlite_wal_failure_never_exposes_partial_effects; do
    if printf '%s\n' "$out13" | grep -qE "^test .*${case_name}.* \.\.\. ok$"; then
      cases=$((cases + 1))
    fi
  done
  if [ "$st13" -eq 0 ] && [ "$cases" -eq 4 ]; then
    ok "13. physical SQLite durability faults pass (4 cases)"
  else
    no "13. physical SQLite durability faults" "$cases/4 cases passed, exit=$st13"
  fi
fi

echo "========================================"
echo "Summary: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
