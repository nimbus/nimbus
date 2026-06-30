#!/usr/bin/env bash
set -u
set -o pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

PASSED=0
FAILED=0

ACTIVE_PLAN="docs/private/plans/nimbus-s3-object-storage-plan.md"
ARCHIVED_PLAN="docs/private/plans/archive/nimbus-s3-object-storage-plan.md"
PROOF_DIR="docs/private/plans/proof/nimbus-s3-object-storage"
BASELINE_PROOF="${PROOF_DIR}/nos0-baseline.md"
OPERATOR_DOC="docs/private/operating/nimbus-s3-object-storage.md"

pass() {
  printf 'PASS: %s\n' "$1"
  PASSED=$((PASSED + 1))
}

fail() {
  printf 'FAIL: %s\n' "$1"
  FAILED=$((FAILED + 1))
}

has_file() {
  test -f "$1"
}

has_dir() {
  test -d "$1"
}

grep_file() {
  local pattern="$1"
  local file="$2"
  grep -Eiq "$pattern" "$file" 2>/dev/null
}

grep_rs() {
  local pattern="$1"
  shift
  local paths=()
  local path
  for path in "$@"; do
    if test -e "$path"; then
      paths+=("$path")
    fi
  done
  test "${#paths[@]}" -gt 0 || return 1
  rg -q "$pattern" "${paths[@]}" --glob '*.rs' 2>/dev/null
}

any_plan_file() {
  if has_file "$ACTIVE_PLAN"; then
    printf '%s\n' "$ACTIVE_PLAN"
    return 0
  fi
  if has_file "$ARCHIVED_PLAN"; then
    printf '%s\n' "$ARCHIVED_PLAN"
    return 0
  fi
  return 1
}

latest_main_ci_green() {
  command -v gh >/dev/null 2>&1 || return 1
  local latest conclusion status
  latest="$(gh run list --branch main --workflow ci.yml --limit 1 --json conclusion,status 2>/dev/null || true)"
  test -n "$latest" && test "$latest" != "[]" || return 1
  conclusion="$(printf '%s\n' "$latest" | grep -oE '"conclusion":"[^"]*"' | head -n 1 | cut -d: -f2 | tr -d '"')"
  status="$(printf '%s\n' "$latest" | grep -oE '"status":"[^"]*"' | head -n 1 | cut -d: -f2 | tr -d '"')"
  test "$status" = "completed" && test "$conclusion" = "success"
}

check_plan_exists() {
  if any_plan_file >/dev/null; then
    pass "plan file exists at active or archived path"
  else
    fail "plan file missing from active and archived paths"
  fi
}

check_routing_entries() {
  local plan
  plan="$(any_plan_file 2>/dev/null || true)"
  if has_file "AGENTS.md" \
    && has_file "docs/private/plans/README.md" \
    && grep_file "nimbus-s3-object-storage-plan\\.md|nimbus-s3-object-storage|NOS0" "AGENTS.md" \
    && grep_file "nimbus-s3-object-storage-plan\\.md|nimbus-s3-object-storage|NOS-A0" "docs/private/plans/README.md" \
    && test -n "$plan" \
    && grep_file "scripts/verify-nimbus-s3-object-storage\\.sh" "$plan"; then
    pass "routing entries name the NOS plan and verifier"
  else
    fail "routing entries missing from AGENTS.md, docs/private/plans/README.md, or the plan"
  fi
}

check_nos0() {
  if has_file "$BASELINE_PROOF" \
    && grep_file "MemoryBlobStore" "$BASELINE_PROOF" \
    && grep_file "LocalPackStore.*NOS-A1|NOS-A1.*LocalPackStore" "$BASELINE_PROOF" \
    && grep_file "crates/nimbus-s3.*absent|absent.*crates/nimbus-s3" "$BASELINE_PROOF" \
    && grep_file "ObjectMetaStore.*absent|absent.*ObjectMetaStore" "$BASELINE_PROOF"; then
    pass "NOS0 baseline proof records the current rebaseline"
  else
    fail "NOS0 baseline proof is missing or does not record the rebaseline"
  fi
}

check_nos1_object_meta_and_blob_store() {
  local object_meta=0 storage_engine=0 per_store_test=0 blob_crate=0 blob_trait=0 local_pack=0 no_per_store_blob_impl=0

  grep_rs "trait ObjectMetaStore" "crates/nimbus-storage/src/traits/object_metadata.rs" \
    && grep_rs "ObjectMetaStore" "crates/nimbus-storage/src/traits/mod.rs" \
    && object_meta=1
  grep_rs "trait StorageEngine" "crates/nimbus-storage/src/traits/core.rs" \
    && grep_rs "ObjectMetaStore" "crates/nimbus-storage/src/traits/core.rs" \
    && grep_rs "impl<T> StorageEngine" "crates/nimbus-storage/src/traits/provider_impls.rs" \
    && grep_rs "ObjectMetaStore" "crates/nimbus-storage/src/traits/provider_impls.rs" \
    && storage_engine=1
  grep_rs "ObjectMetaStore" "crates/nimbus-storage/src/tests" "crates/nimbus-storage/tests" && per_store_test=1
  has_dir "crates/nimbus-blob" && blob_crate=1
  grep_rs "trait BlobStore" "crates/nimbus-blob/src" && grep_rs "struct MemoryBlobStore" "crates/nimbus-blob/src" && blob_trait=1
  grep_rs "struct LocalPackStore" "crates/nimbus-blob/src" && local_pack=1
  if ! grep_rs "impl .*BlobStore for .*TenantStore|impl .*BlobStore for .*SqliteTenantStore|impl .*BlobStore for .*PostgresTenantStore|impl .*BlobStore for .*MySqlTenantStore|impl .*BlobStore for .*LibsqlReplicaTenantStore" "crates/nimbus-storage/src"; then
    no_per_store_blob_impl=1
  fi

  if [ "$object_meta" = 1 ] && [ "$storage_engine" = 1 ] && [ "$per_store_test" = 1 ] \
    && [ "$blob_crate" = 1 ] && [ "$blob_trait" = 1 ] && [ "$local_pack" = 1 ] \
    && [ "$no_per_store_blob_impl" = 1 ]; then
    pass "ObjectMetaStore and BlobStore/LocalPackStore seams are present"
  else
    fail "NOS1 metadata/blob seam incomplete (ObjectMetaStore=${object_meta} StorageEngine=${storage_engine} tests=${per_store_test} blob_crate=${blob_crate} blob_trait=${blob_trait} LocalPackStore=${local_pack} no_per_store_blob_impl=${no_per_store_blob_impl})"
  fi
}

check_nos1_blob_behavior() {
  if grep_rs "put_is_idempotent" "crates/nimbus-blob/src" \
    && grep_rs "manifest.*atomic|atomic.*manifest" "crates/nimbus-blob/src" "crates/nimbus-storage/src" "crates/nimbus-storage/tests" \
    && grep_rs "compaction|compact" "crates/nimbus-blob/src" \
    && grep_rs "EncryptedBlobStore" "crates/nimbus-blob/src" \
    && grep_rs "EncryptedBlobStore" "crates/nimbus-object-storage/src" \
    && grep_rs "FramedBlobAes256GcmSiv|object-storage master key" "crates/nimbus-object-storage/src" "crates/nimbus-crypto/src" \
    && grep_rs "different_keys_yield_different_ciphertext|tenant.*isolated" "crates/nimbus-blob/src"; then
    pass "NOS1 encrypted blob behavior and resolver composition are present"
  else
    fail "NOS1 blob behavior/resolver encryption tests are incomplete"
  fi
}

check_nos2_lifecycle_gc() {
  if has_file "crates/nimbus-blob/src/gc.rs" \
    && grep_rs "grace.*window|Gc|garbage|mark.*sweep|sweep.*mark" "crates/nimbus-blob/src/gc.rs" \
    && grep_rs "partial.*upload|referenced.*never.*swept|crypto.*shred|compaction" "crates/nimbus-blob/src" "crates/nimbus-blob/tests"; then
    pass "NOS2 lifecycle/GC/crypto-shred code is present"
  else
    fail "NOS2 lifecycle/GC/crypto-shred code is missing"
  fi
}

check_nos3_s3_surface() {
  if has_dir "crates/nimbus-s3" \
    && grep_rs "impl .*s3s::S3|s3s::S3" "crates/nimbus-s3/src" \
    && has_dir "crates/nimbus-server/src/adapters/s3" \
    && grep_rs "s3|no_s3" "crates/nimbus-bin/src" "crates/nimbus-server/src" \
    && grep_rs "ETag|MD5|CRC64|SigV4|multipart|ListObjectsV2" "crates/nimbus-s3" "crates/nimbus-server"; then
    pass "NOS3 S3 surface is present"
  else
    fail "NOS3 S3 surface is missing"
  fi
}

check_nos4_placement_backup() {
  if grep_rs "PlacementBlobStore" "crates/nimbus-blob/src" \
    && grep_rs "ObjectStoreBlobStore|impl .*object_store|object_store::" "crates/nimbus-blob/src" "crates/nimbus-s3/src" \
    && grep_rs "object_store" "crates/nimbus-blob" "crates/nimbus-s3" "crates/nimbus-server" \
    && grep_rs "BackupBundle|ObjectBackup|restore.*bundle|commit_log.*segment|key.*escrow|ObjectLock" "crates/nimbus-blob/src" "crates/nimbus-s3/src" "crates/nimbus-bin/src"; then
    pass "NOS4 placement and backup bundle are present"
  else
    fail "NOS4 placement/cloud/backup bundle is incomplete"
  fi
}

check_nos5_config_operator() {
  if grep_rs "ObjectPlacement|PlacementPolicy|ObjectPlacementStore" "crates/nimbus-storage/src" "crates/nimbus-engine/src" \
    && grep_rs "set-placement|gc-status|tenant.*rm|restore-object-store|backup-object-store" "crates/nimbus-bin/src" \
    && grep_rs "0600|master key" "crates/nimbus-bin/src" "crates/nimbus-crypto/src"; then
    pass "NOS5 config seams and operator verbs are present"
  else
    fail "NOS5 config seams and operator verbs are incomplete"
  fi
}

check_nos6_filesystem_binder() {
  if grep_rs "ObjectBlobFsBackend|BlobFsBackend|ObjectRwBackend" "crates/nimbus-blob/src" "crates/nimbus-fs/src/object" \
    && grep_rs "impl .*FileSystem.*ObjectRwBackend|impl .*ObjectRwBackend.*FileSystem" "crates/nimbus-fs/src/object" \
    && grep_rs "fuse3|fuser" "crates/nimbus-blob" "crates/nimbus-fs" "Cargo.toml"; then
    pass "NOS6 filesystem binder is present"
  else
    fail "NOS6 filesystem binder is incomplete"
  fi
}

check_garage_and_license() {
  local doc=0 deny=0
  has_file "$OPERATOR_DOC" && grep_file "Garage.*AGPL|AGPL.*Garage" "$OPERATOR_DOC" && doc=1
  if command -v cargo >/dev/null 2>&1; then
    if ! cargo metadata --format-version 1 --no-deps 2>/dev/null | grep -Eiq '"license":"[^"]*AGPL|garage'; then
      deny=1
    fi
  fi
  if [ "$doc" = 1 ] && [ "$deny" = 1 ]; then
    pass "Garage exclusion is documented and no AGPL dependency is visible"
  else
    fail "Garage exclusion/license condition incomplete (doc=${doc} dependency_scan=${deny})"
  fi
}

check_nos7_closeout() {
  local plan
  plan="$(any_plan_file 2>/dev/null || true)"
  if has_file "$OPERATOR_DOC" \
    && test -n "$plan" \
    && ! grep -Eq '\| NOS[0-7] \|.*\| (todo|in_progress|blocked)' "$plan" \
    && latest_main_ci_green; then
    pass "NOS7 operator doc, closed ledger, and green main CI are present"
  else
    fail "NOS7 closeout is incomplete"
  fi
}

check_plan_exists
check_routing_entries
check_nos0
check_nos1_object_meta_and_blob_store
check_nos1_blob_behavior
check_nos2_lifecycle_gc
check_nos3_s3_surface
check_nos4_placement_backup
check_nos5_config_operator
check_nos6_filesystem_binder
check_garage_and_license
check_nos7_closeout

printf 'summary: %d passed, %d failed\n' "$PASSED" "$FAILED"
test "$FAILED" -eq 0
