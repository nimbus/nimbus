#!/usr/bin/env bash
set -u
set -o pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

PRIVATE_DOCS="${NIMBUS_PRIVATE_DOCS:-docs/private}"

PASSED=0
FAILED=0

PLAN="$PRIVATE_DOCS/plans/nimbus-blob-range-reads-hardening-plan.md"
if [ ! -f "$PLAN" ]; then
  PLAN="$PRIVATE_DOCS/plans/archive/nimbus-blob-range-reads-hardening-plan.md"
fi
PROOF_DIR="$PRIVATE_DOCS/plans/proof/nimbus-blob-range-reads-hardening"

pass() { printf 'PASS: %s\n' "$1"; PASSED=$((PASSED + 1)); }
fail() { printf 'FAIL: %s\n' "$1"; FAILED=$((FAILED + 1)); }
has_file() { test -f "$1"; }
grep_file() { grep -Eiq "$1" "$2" 2>/dev/null; }

check_brh0() {
  if has_file "$PLAN" \
    && grep_file "^## Plan Outcome$" "$PLAN" \
    && grep_file "^## Ledger$" "$PLAN" \
    && has_file "$PROOF_DIR/brh0-baseline.md" \
    && grep_file "whole-get" "$PROOF_DIR/brh0-baseline.md"; then
    pass "BRH0 plan and baseline exist"
  else
    fail "BRH0 plan/baseline missing"
  fi
}

check_brh1() {
  if grep_file "store\.get_range|\.get_range\(&self\.location|get_range\(&path" "crates/nimbus-blob/src/object_store.rs" \
    && ! grep_file "self\.get\(hash\)\.await\?;" "crates/nimbus-blob/src/object_store.rs" \
    && ! grep_file "self\.get\(hash\)\.await\?;" "crates/nimbus-blob/src/local.rs" \
    && grep_file "frame" "crates/nimbus-blob/src/encrypted.rs" \
    && grep -rqE "fn .*range_read_transfers_only|underlying_bytes|inner_bytes_served" crates/nimbus-blob/src --include='*.rs' 2>/dev/null \
    && has_file "$PROOF_DIR/brh1-range-reads.md"; then
    pass "BRH1 real range reads with per-impl byte accounting"
  else
    fail "BRH1 range reads incomplete"
  fi
}

check_brh2() {
  if grep -c "ambient_root_delegate!" crates/nimbus-fs/src/passthrough.rs 2>/dev/null | awk '{exit !($1 >= 30)}' \
    && grep -rqE "fn .*lying_manifest|short.*get_range.*error|manifest_overclaims" crates/nimbus-fs/src --include='*.rs' 2>/dev/null \
    && grep -rqE "fn .*bridge.*concurren|concurrent.*byte_plane" crates/nimbus-fs/src --include='*.rs' 2>/dev/null \
    && grep -rqE "empty.*prefix|reject.*empty" crates/nimbus-fs/src/caps.rs 2>/dev/null \
    && has_file "$PROOF_DIR/brh2-fs-hardening.md"; then
    pass "BRH2 gating unification and hardening present"
  else
    fail "BRH2 hardening incomplete"
  fi
}

check_brh3() {
  if grep_file "read_manifest_range" "crates/nimbus-fs/src/object/mod.rs" \
    && grep -rqE "fn .*object.*lazy|object_file_reads_are_windowed|open_does_not_materialize" crates/nimbus-fs/src --include='*.rs' 2>/dev/null \
    && has_file "$PROOF_DIR/brh3-lazy-object-reads.md"; then
    pass "BRH3 lazy in-isolate object reads present"
  else
    fail "BRH3 lazy object reads incomplete"
  fi
}

check_brh4() {
  if has_file "$PROOF_DIR/brh4-closeout.md" \
    && ! grep -E '^\| BRH[0-9] .*\| (todo|in_progress|blocked)' "$PLAN" >/dev/null 2>&1; then
    pass "BRH4 closeout complete and ledger closed"
  else
    fail "BRH4 closeout incomplete"
  fi
}

check_brh0
check_brh1
check_brh2
check_brh3
check_brh4

printf 'summary: %d passed, %d failed\n' "$PASSED" "$FAILED"
test "$FAILED" -eq 0
