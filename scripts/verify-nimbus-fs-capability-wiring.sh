#!/usr/bin/env bash
set -u
set -o pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

# Private plan/proof docs live in the primary checkout's untracked
# docs/private tree. Override when running from a worktree that does not
# carry the local-only files.
PRIVATE_DOCS="${NIMBUS_PRIVATE_DOCS:-docs/private}"

PASSED=0
FAILED=0

ACTIVE_PLAN="$PRIVATE_DOCS/plans/nimbus-fs-capability-wiring-plan.md"
ARCHIVED_PLAN="$PRIVATE_DOCS/plans/archive/nimbus-fs-capability-wiring-plan.md"
NFS_ARCHIVE="$PRIVATE_DOCS/plans/archive/nimbus-isolate-filesystem-plan.md"
OPERATOR_DOC="$PRIVATE_DOCS/operating/nimbus-isolate-filesystem.md"
PROOF_DIR="$PRIVATE_DOCS/plans/proof/nimbus-fs-capability-wiring"

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
  local plan
  plan="$(any_plan_file 2>/dev/null || true)"
  if test -n "$plan" \
    && grep_file "^## Plan Outcome$" "$plan" \
    && grep_file "^## Ledger$" "$plan" \
    && grep_file "^## Completion Gate$" "$plan"; then
    pass "FCW plan file exists with outcome, ledger, and completion gate"
  else
    fail "FCW plan file missing or lacks outcome/ledger/completion gate"
  fi
}

check_fcw0() {
  if has_file "$PROOF_DIR/fcw0-baseline.md" \
    && grep_file "default_file_system" "$PROOF_DIR/fcw0-baseline.md" \
    && grep_file "crates/nimbus-server/src/execution/invocations" "$PROOF_DIR/fcw0-baseline.md" \
    && grep_file "faf9725b9" "$PROOF_DIR/fcw0-baseline.md" \
    && grep_file "faf9725b9" "$NFS_ARCHIVE"; then
    pass "FCW0 baseline proof records the ungated call site and the merged rebaseline SHA"
  else
    fail "FCW0 baseline proof missing or incomplete"
  fi
}

check_fcw1() {
  local ungated
  ungated="$(grep -rn "default_file_system" crates/nimbus-server/src --include='*.rs' 2>/dev/null \
    | grep -Ev 'test_support|/tests/' || true)"
  if test -z "$ungated" \
    && grep -rqE "resolve_fs_grants|FsGrantResolver|resolved_file_system" crates/nimbus-server/src --include='*.rs' 2>/dev/null \
    && grep -rqE "fn .*no_grant.*deny|fn .*deny.*without_grant|ungranted_substrate_gets_deny_filesystem" crates/nimbus-fs/src crates/nimbus-server/src --include='*.rs' 2>/dev/null \
    && grep -rqE "fn .*tightened_grant|read_only_root_grant_is_enforced" crates/nimbus-fs/src crates/nimbus-server/src --include='*.rs' 2>/dev/null \
    && has_file "$PROOF_DIR/fcw1-grant-wiring.md"; then
    pass "FCW1 grant-resolved construction is wired with deny-by-default and tightened-grant tests"
  else
    fail "FCW1 grant wiring incomplete (ungated call sites remain, seam missing, or tests absent)"
  fi
}

check_fcw2() {
  if ! grep_file "read_to_end" "crates/nimbus-fs/src/cas_ro.rs" \
    && grep_file "get_range" "crates/nimbus-fs/src/cas_ro.rs" \
    && grep_file "get_range" "crates/nimbus-fs/src/object/mod.rs" \
    && ! grep -E "thread::spawn" crates/nimbus-fs/src/cas_ro.rs crates/nimbus-fs/src/object/mod.rs >/dev/null 2>&1 \
    && grep -rqE "fn .*sequential.*chunk.*read|bytes_read_is_o_of_requested" crates/nimbus-fs/src --include='*.rs' 2>/dev/null \
    && has_file "$PROOF_DIR/fcw2-get-range.md"; then
    pass "FCW2 byte-plane reads use get_range through a shared bridge with byte-accounting proof"
  else
    fail "FCW2 get_range read path incomplete"
  fi
}

check_fcw3() {
  if grep -rqE "fn .*resolver_propert|proptest|fn .*property_corpus" crates/nimbus-fs/src --include='*.rs' 2>/dev/null \
    && grep -rqE "fn .*toctou|symlink_parent_swap" crates/nimbus-fs/src --include='*.rs' 2>/dev/null \
    && has_file "$PROOF_DIR/fcw3-resolver-tail.md" \
    && grep_file "absolute symlink" "$PROOF_DIR/fcw3-resolver-tail.md"; then
    pass "FCW3 resolver property corpus, TOCTOU case, and Node-compat symlink evidence exist"
  else
    fail "FCW3 resolver hardening tail incomplete"
  fi
}

check_fcw4() {
  local plan
  plan="$(any_plan_file 2>/dev/null || true)"
  if test -n "$plan" \
    && grep_file "grant" "$OPERATOR_DOC" \
    && ! grep_file "mechanism, not" "$OPERATOR_DOC" \
    && has_file "$PROOF_DIR/fcw4-closeout.md" \
    && ! grep -E '^\| FCW[0-9] .*\| (todo|in_progress|blocked)' "$plan" >/dev/null 2>&1; then
    pass "FCW4 operator doc documents the grant model and the ledger is closed"
  else
    fail "FCW4 closeout incomplete (operator doc caveat, proof, or open ledger rows)"
  fi
}

check_plan_exists
check_fcw0
check_fcw1
check_fcw2
check_fcw3
check_fcw4

printf 'summary: %d passed, %d failed\n' "$PASSED" "$FAILED"
test "$FAILED" -eq 0
