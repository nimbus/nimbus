#!/usr/bin/env bash
# Verifier for docs/private/plans/rustfs-storage-hardening-plan.md (RFS bands).
#
# Conditions (per the plan's RFS0 verifier contract):
#   1. The RFS0 source inventory exists with the pinned SHA, tag context,
#      CVE record, and per-file porting table.
#   2. Every workspace file carrying a rustfs provenance marker preserves the
#      upstream copyright and is listed in its crate's THIRD_PARTY.md.
#   3. Negative self-test: a synthetic provenance-marked file with no
#      copyright/manifest entry makes condition 2's check FAIL.
#   4. A security-review record exists for every provenance-marked file.
#   5. The RFS1 metadata decision memo exists with per-obligation verdicts.
#   6. Band acceptance tests exist (named test functions per RFS2/RFS3);
#      RUN_TESTS=1 additionally executes them.
#
# Proof files live under untracked docs/private/; on a checkout without them
# (e.g. hosted CI) conditions 1/4/5 report FAIL — this verifier is a local
# plan gate, not a hosted-CI gate.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PROOF_DIR="docs/private/plans/proof/rustfs-storage-hardening"
INVENTORY="${PROOF_DIR}/source-inventory.md"
MEMO="${PROOF_DIR}/rfs1-metadata-decision.md"
PIN_SHA="bd5d3c5d92a0aa70a7d92da3e48761d6e61f0dc9"

fail_count=0
pass() { printf '  [pass] %s\n' "$1"; }
fail() { printf '  [FAIL] %s\n' "$1" >&2; fail_count=$((fail_count + 1)); }

# Returns the workspace .rs files carrying a rustfs provenance marker.
provenance_files() {
  grep -rl --include='*.rs' -E '(Lifted|Adapted) from rustfs/rustfs@[0-9a-f]{7,}' crates/ 2>/dev/null || true
}

# Core of condition 2, parameterized so condition 3 can self-test it.
# check_provenance_file <file> <crate_dir> -> 0 ok / 1 violation
check_provenance_file() {
  local file="$1" crate_dir="$2"
  head -n 20 "${file}" | grep -q 'Copyright 2024 RustFS Team' || return 1
  [ -f "${crate_dir}/THIRD_PARTY.md" ] || return 1
  local rel="${file#"${crate_dir}"/}"
  # Literal backtick-wrapped path: single-quote the backticks so the shell
  # cannot command-substitute them, and -F so grep matches literally.
  grep -qF '`'"${rel}"'`' "${crate_dir}/THIRD_PARTY.md" || return 1
  return 0
}

crate_dir_of() {
  local file="$1"
  local dir="${file}"
  while [ "${dir}" != "." ] && [ "${dir}" != "/" ]; do
    dir="$(dirname "${dir}")"
    if [ -f "${dir}/Cargo.toml" ]; then
      printf '%s' "${dir}"
      return 0
    fi
  done
  return 1
}

printf 'rustfs-storage-hardening verifier\n\n'

printf '[1] RFS0 source inventory\n'
if [ -f "${INVENTORY}" ] \
  && grep -q "${PIN_SHA}" "${INVENTORY}" \
  && grep -q '1\.0\.0-beta\.8-879' "${INVENTORY}" \
  && grep -q 'CVE-2025-68926' "${INVENTORY}" \
  && grep -q 'Per-file porting table' "${INVENTORY}"; then
  pass "source-inventory.md present with pin, tag context, CVE, porting table"
else
  fail "missing/incomplete ${INVENTORY}"
fi

printf '[2] provenance headers + THIRD_PARTY manifests\n'
found_any=0
while IFS= read -r file; do
  [ -z "${file}" ] && continue
  found_any=1
  crate_dir="$(crate_dir_of "${file}")" || { fail "${file}: no owning crate"; continue; }
  if check_provenance_file "${file}" "${crate_dir}"; then
    pass "${file}"
  else
    fail "${file}: missing upstream copyright or THIRD_PARTY.md entry"
  fi
done <<< "$(provenance_files)"
[ "${found_any}" -eq 1 ] || pass "no provenance-marked files yet (vacuous)"

printf '[3] negative self-test\n'
selftest_dir="$(mktemp -d)"
mkdir -p "${selftest_dir}/src"
cat > "${selftest_dir}/Cargo.toml" <<'TOML'
[package]
name = "selftest"
TOML
cat > "${selftest_dir}/src/bad.rs" <<'RS'
// Adapted from rustfs/rustfs@bd5d3c5d92a0aa70a7d92da3e48761d6e61f0dc9
RS
if check_provenance_file "${selftest_dir}/src/bad.rs" "${selftest_dir}"; then
  fail "self-test: a marker without copyright/manifest was NOT flagged"
else
  pass "self-test: unattributed provenance marker is flagged"
fi
rm -rf "${selftest_dir}"

printf '[4] security-review records\n'
while IFS= read -r file; do
  [ -z "${file}" ] && continue
  base="$(basename "${file}" .rs)"
  record="${PROOF_DIR}/security-review-${base}-rs.md"
  if [ -f "${record}" ]; then
    pass "${record}"
  else
    fail "missing ${record} for ${file}"
  fi
done <<< "$(provenance_files)"

printf '[5] RFS1 metadata decision memo\n'
if [ -f "${MEMO}" ] \
  && grep -q 'Decision: \*\*KEEP SPLIT\*\*\|Decision: \*\*ADOPT\|Decision: \*\*HYBRID' "${MEMO}" \
  && grep -q 'PRESERVED-BY-SPLIT' "${MEMO}"; then
  pass "rfs1-metadata-decision.md present with decision + verdicts"
else
  fail "missing/incomplete ${MEMO}"
fi

printf '[6] band acceptance tests\n'
required_tests=(
  local_pack_second_open_shares_live_state
  root_lock_excludes_second_process
  shared_open_still_refuses_foreign_identity
  local_pack_format_marker_roundtrip
  local_pack_rejects_foreign_or_future_marker
  local_pack_startup_cleanup_removes_stale_temp
  local_pack_rejects_symlinked_root
  local_pack_read_only_serves_reads_and_rejects_writes
  durable_write_fsync_order
  durable_write_fsync_error_poisons_store
  crash_bytes_written_index_missing
  crash_index_partially_written
  crash_active_pack_truncated
  crash_temp_file_left_behind
  crash_index_points_at_corrupt_bytes
  crash_index_torn_release_tail_truncated
  crash_index_unknown_tag_fails_closed
  crash_index_unknown_tag_torn_at_eof_still_fails_closed
  read_only_put_stream_refuses_before_consuming_input
  read_only_refuses_unowned_data_bearing_root
  writable_open_refuses_unowned_data_bearing_root
  fresh_root_creation_fsyncs_new_directory_entries
  crash_release_replay_order_preserved
  compaction_crash_replay_prefers_rewritten_records
  local_pack_concurrent_same_hash_dedups_under_mutex
  gc_grace_retains_when_clock_regresses
  scrub_detects_flipped_byte
  scrub_quarantines_corrupt_record
  scrub_rebuilds_index_from_packs
  scrub_rebuilds_corrupt_index_from_packs
  scrub_resumes_from_checkpoint
  scrub_pacing_bounds_io
  scrub_detects_truncated_record
  scrub_encrypted_layer_detects_aead_failure
  scrub_quarantine_survives_compaction_without_poisoning
  scrub_reupload_clears_quarantine
  scrub_stale_snapshot_cannot_quarantine_relocated_blob
  scrub_resume_rescans_growable_pack
  released_quarantined_blob_reads_not_found
  scrub_does_not_quarantine_healthy_records_after_corrupt_segment
  compaction_invalidates_stale_scrub_checkpoint
  scrub_ignores_bytes_past_snapshot_active_length
  scrub_quarantines_records_behind_corrupt_pack_header
  interrupted_checkpoint_records_snapshot_active_pack
  quarantine_reverifies_record_before_inserting
  rebuild_preserves_healthy_records_after_corrupt_segment
  corrupt_index_rebuild_salvages_prefix_offsets
  corrupt_index_rebuild_retains_quarantined_claim
  checkpoint_publication_refused_after_compaction_epoch_moves
  resume_rescans_packs_with_findings
  repeat_scrub_reports_previously_quarantined
  raw_reupload_does_not_clear_aead_quarantine
  rebuild_invalidates_stale_scrub_checkpoint
  reupload_after_pack_header_corruption_heals_into_fresh_pack
  corrupt_index_repair_recovers_claim_when_hash_field_corrupted
  scrub_of_corrupt_active_header_retires_pack
  unconditional_quarantine_skips_released_hash
  open_retires_unreferenced_corrupt_header_pack
  open_fails_closed_on_referenced_corrupt_header_pack
  open_prunes_stale_quarantine_entries
  corrupt_index_repair_retains_truncated_quarantined_claim
  open_retires_corrupt_pack_referenced_only_by_quarantined_claims
  corrupt_checkpoint_is_ignored_and_full_scan_runs
  repeat_scrub_of_corrupt_header_still_retires_active_pack
  quarantine_revalidation_bytes_are_accounted
  scrub_retires_empty_corrupt_active_pack
  rebuild_retains_live_claim_behind_corrupt_pack_header
  scrub_retires_corrupt_active_pack_despite_release_race
  corrupt_index_repair_fails_closed_on_unrecoverable_claim
  record_finding_does_not_downgrade_content_quarantine
  missing_index_open_does_not_prune_quarantine_before_rebuild
  failed_missing_index_rebuild_leaves_no_provisional_index
  failed_open_then_rebuild_removes_provisional_index
  quarantine_revalidation_streams_large_corrupt_record
)
for t in "${required_tests[@]}"; do
  if grep -rq "fn ${t}(" crates/nimbus-blob/src/; then
    pass "test ${t} exists"
  else
    fail "test ${t} missing from crates/nimbus-blob"
  fi
done
if [ "${RUN_TESTS:-0}" = "1" ]; then
  if cargo test -p nimbus-blob -p nimbus-object-storage --quiet; then
    pass "cargo test -p nimbus-blob -p nimbus-object-storage"
  else
    fail "band tests failed"
  fi
fi

printf '\n'
if [ "${fail_count}" -gt 0 ]; then
  printf 'rustfs-storage-hardening verifier: FAIL (%d violation(s))\n' "${fail_count}" >&2
  exit 1
fi
printf 'rustfs-storage-hardening verifier: pass\n'
