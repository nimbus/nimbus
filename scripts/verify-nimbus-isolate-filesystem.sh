#!/usr/bin/env bash
set -u
set -o pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

PASSED=0
FAILED=0

ACTIVE_PLAN="docs/private/plans/nimbus-isolate-filesystem-plan.md"
ARCHIVED_PLAN="docs/private/plans/archive/nimbus-isolate-filesystem-plan.md"
PROOF_DIR="docs/private/plans/proof/nimbus-isolate-filesystem"

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
    && grep_file "nimbus-isolate-filesystem-plan\\.md|nimbus-isolate-filesystem" "AGENTS.md" \
    && grep_file "nimbus-isolate-filesystem-plan\\.md|NimbusFS|NFS0" "docs/private/plans/README.md" \
    && { test -z "$plan" || grep_file "scripts/verify-nimbus-isolate-filesystem\\.sh" "$plan"; }; then
    pass "routing entries name the NFS plan and verifier"
  else
    fail "routing entries missing from AGENTS.md, docs/private/plans/README.md, or the plan"
  fi
}

check_nfs0() {
  local baseline="$PROOF_DIR/nfs0-baseline.md"
  local exemplar="$PROOF_DIR/nfs0-exemplar-comparison.md"
  if has_file "$baseline" \
    && has_file "$exemplar" \
    && grep_file "RealFs.*extensions\\.rs|extensions\\.rs.*RealFs" "$baseline" \
    && grep_file "nimbus-fs" "$baseline" \
    && grep_file "NimbusFsBackend" "$baseline" \
    && grep_file "capability roots" "$exemplar" \
    && grep_file "handle-rooted passthrough" "$exemplar" \
    && grep_file "virtual.*realpath|realpath.*virtual" "$exemplar" \
    && grep_file "symlink policy" "$exemplar" \
    && grep_file "FsCaps.*file-read.*file-write.*directory-read.*directory-mutate.*metadata-mutate.*link-create" "$exemplar" \
    && grep_file "Landlock" "$exemplar"; then
    pass "NFS0 baseline and exemplar decisions recorded"
  else
    fail "NFS0 proof files or required closed decisions are missing"
  fi
}

check_direct_bypass_scan() {
  local findings
  findings="$(rg -n '\\b(std::fs|tokio::fs)::|use std::fs|use tokio::fs' \
    crates/nimbus-fs crates/nimbus-runtime/src/runtime/bootstrap/extensions.rs \
    crates/nimbus-runtime/src/fs crates/nimbus-runtime/src/runtime/bootstrap/ops/runtime_local \
    crates/nimbus-server/src/execution/invocations \
    --glob '*.rs' 2>/dev/null \
    | grep -Ev 'crates/nimbus-fs/src/(passthrough|tests|test_support|memfs|cas_ro|cache)\\.rs|crates/nimbus-fs/src/.*/tests\\.rs' \
    || true)"
  test -z "$findings"
}

check_nfs1() {
  if has_dir "crates/nimbus-runtime/src/fs" \
    && has_file "crates/nimbus-runtime/src/fs/mod.rs" \
    && has_file "crates/nimbus-fs/src/lib.rs" \
    && has_file "crates/nimbus-fs/src/passthrough.rs" \
    && grep_file "trait NimbusFsBackend" "crates/nimbus-runtime/src/fs/mod.rs" \
    && grep_file "struct NimbusFs" "crates/nimbus-fs/src/lib.rs" \
    && grep_file "struct PassthroughBackend" "crates/nimbus-fs/src/passthrough.rs" \
    && grep_file "RootCapability" "crates/nimbus-fs/src/passthrough.rs" \
    && grep_file "cap_std::fs::Dir|Dir as CapDir" "crates/nimbus-fs/src/passthrough.rs" \
    && grep_file "cap-std" "crates/nimbus-fs/Cargo.toml" \
    && grep_file "raw_passthrough_chdir_does_not_touch_process_cwd" "crates/nimbus-fs/src/tests.rs" \
    && grep_file "rooted_passthrough_rejects_parent_escape_before_create" "crates/nimbus-fs/src/tests.rs" \
    && ! grep_file "MaybeArc::new\\(deno_fs::RealFs\\)" "crates/nimbus-runtime/src/runtime/bootstrap/extensions.rs" \
    && grep_file "file_system\\(\\)" "crates/nimbus-runtime/src/runtime/driver/construction.rs" \
    && check_direct_bypass_scan \
    && has_file "$PROOF_DIR/nfs1-shell.md" \
    && grep_file "Node-compat.*passed|node compat.*passed|no-regression.*passed" "$PROOF_DIR/nfs1-shell.md"; then
    pass "NFS1 runtime seam, NimbusFS shell, capability-rooted passthrough, and bypass scan are present"
  else
    fail "NFS1 seam/shell/wiring/proof condition is incomplete"
  fi
}

check_nfs2() {
  if has_file "crates/nimbus-fs/src/mount.rs" \
    && has_file "crates/nimbus-fs/src/resolver.rs" \
    && has_file "crates/nimbus-fs/src/memfs.rs" \
    && grep_file "struct MountTable" "crates/nimbus-fs/src/mount.rs" \
    && grep_file "struct MountResolver" "crates/nimbus-fs/src/resolver.rs" \
    && grep_file "struct MemFsBackend" "crates/nimbus-fs/src/memfs.rs" \
    && grep_file "longest.*prefix" "crates/nimbus-fs/src/resolver.rs" \
    && grep_file "masked|readonly" "crates/nimbus-fs/src/mount.rs" \
    && grep_file "symlink|realpath|readlink|cross-mount|typed" "crates/nimbus-fs/src/resolver.rs" \
    && has_file "$PROOF_DIR/nfs2-mount-table.md"; then
    pass "NFS2 mount table, resolver, memfs, overlays, and resolver proof are present"
  else
    fail "NFS2 mount table/resolver/memfs/proof condition is incomplete"
  fi
}

check_nfs3() {
  if has_file "crates/nimbus-fs/src/cas_ro.rs" \
    && grep_file "CasReadOnlyBackend" "crates/nimbus-fs/src/cas_ro.rs" \
    && grep_file "BlobStore" "crates/nimbus-fs/src/cas_ro.rs" \
    && grep_file "get_stream" "crates/nimbus-fs/src/cas_ro.rs" \
    && grep_file "EROFS|ReadOnly" "crates/nimbus-fs/src/cas_ro.rs" \
    && has_file "$PROOF_DIR/nfs3-cas-ro.md"; then
    pass "NFS3 CAS read-only backend consumes BlobStore::get_stream and records proof"
  else
    fail "NFS3 CAS read-only backend/proof condition is incomplete"
  fi
}

check_nfs4() {
  if has_file "crates/nimbus-fs/src/caps.rs" \
    && grep_file "file_read" "crates/nimbus-fs/src/caps.rs" \
    && grep_file "file_write" "crates/nimbus-fs/src/caps.rs" \
    && grep_file "directory_read" "crates/nimbus-fs/src/caps.rs" \
    && grep_file "directory_mutate" "crates/nimbus-fs/src/caps.rs" \
    && grep_file "metadata_mutate" "crates/nimbus-fs/src/caps.rs" \
    && grep_file "link_create" "crates/nimbus-fs/src/caps.rs" \
    && grep_file "max_write_size|write-size" "crates/nimbus-fs/src/caps.rs" \
    && grep_file "TRUNCATE|append|rename|copy|chmod|chown|utime|symlink" "crates/nimbus-fs/src/caps.rs" \
    && has_file "$PROOF_DIR/nfs4-fscaps.md"; then
    pass "NFS4 fail-closed FsCaps rights matrix and proof are present"
  else
    fail "NFS4 FsCaps matrix/proof condition is incomplete"
  fi
}

check_nfs5() {
  if has_file "crates/nimbus-fs/src/wasi.rs" \
    && grep_file "DirPerms" "crates/nimbus-fs/src/wasi.rs" \
    && grep_file "MUTATE" "crates/nimbus-fs/src/wasi.rs" \
    && grep_file "FilePerms" "crates/nimbus-fs/src/wasi.rs" \
    && grep_file "WRITE" "crates/nimbus-fs/src/wasi.rs" \
    && grep_file "cross.*binder|binder.*consistency" "crates/nimbus-fs/src/wasi.rs" \
    && has_file "$PROOF_DIR/nfs5-wasi-binder.md"; then
    pass "NFS5 WASI preopen binder maps FsCaps to DirPerms/FilePerms"
  else
    fail "NFS5 WASI binder/proof condition is incomplete"
  fi
}

check_nfs6() {
  if has_file "crates/nimbus-fs/src/backend.rs" \
    && has_file "crates/nimbus-fs/src/cache.rs" \
    && grep_file "BackendRegistry|register" "crates/nimbus-fs/src/backend.rs" \
    && grep_file "ObjectRwBackend" "crates/nimbus-fs/src/backend.rs" \
    && grep_file "random write|hardlink|symlink|directory rename|unsupported" "crates/nimbus-fs/src/backend.rs" \
    && grep_file "evict|cache hit|Cache" "crates/nimbus-fs/src/cache.rs" \
    && has_file "$PROOF_DIR/nfs6-backend-slot.md"; then
    pass "NFS6 registration ABI, cache, object-store slot, and proof are present"
  else
    fail "NFS6 registration/cache/object-slot/proof condition is incomplete"
  fi
}

check_nfs7() {
  local plan
  plan="$(any_plan_file 2>/dev/null || true)"
  if has_file "docs/private/operating/nimbus-isolate-filesystem.md" \
    && has_file "$PROOF_DIR/nfs7-closeout.md" \
    && grep_file "passthrough.*memfs.*CAS.*object" "docs/private/operating/nimbus-isolate-filesystem.md" \
    && grep_file "FsCaps" "docs/private/operating/nimbus-isolate-filesystem.md" \
    && grep_file "container substrates use the sandbox bundle|sandbox bundle" "docs/private/operating/nimbus-isolate-filesystem.md" \
    && test -n "$plan" \
    && ! grep -Eq '\\| NFS[0-7] \\|.*\\| (todo|in_progress|blocked)' "$plan" \
    && grep_file "CI.*green" "$PROOF_DIR/nfs7-closeout.md"; then
    pass "NFS7 operator doc, closed ledger, and closeout proof are present"
  else
    fail "NFS7 operator doc, closed ledger, or closeout proof is incomplete"
  fi
}

check_plan_exists
check_routing_entries
check_nfs0
check_nfs1
check_nfs2
check_nfs3
check_nfs4
check_nfs5
check_nfs6
check_nfs7

printf 'summary: %d passed, %d failed\n' "$PASSED" "$FAILED"

if [ "$FAILED" -ne 0 ]; then
  exit 1
fi
