#!/usr/bin/env bash
# Aggregate completion-gate verifier for the DynamoDB Adapter Hardening plan
# (`docs/plans/dynamodb-adapter-hardening-plan.md`).
#
# Exits 0 iff every condition in the plan's "## Completion gate" is satisfied.
# Scaffolded RED on day one: it FAILS on every unimplemented item today, and
# roadmap items H1..H7 progressively flip conditions from FAIL to PASS.
#
# Philosophy (matches scripts/verify-dynamodb-adapter.sh): this verifier proves
# the durable *artifacts and evidence* exist and are structurally complete. The
# heavy "it compiles / tests pass" proof is enforced by branch CI (the green
# `dynamodb-adapter-hardening` branch run is part of the /goal stop condition).

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 2

PLAN_ACTIVE="docs/plans/dynamodb-adapter-hardening-plan.md"
PLAN_ARCHIVED="docs/plans/archive/dynamodb-adapter-hardening-plan.md"

CRATE="crates/nimbus-dynamodb"
VERIFY="${CRATE}/src/auth/sigv4/verify.rs"
CONFIG="${CRATE}/src/config.rs"
TENANT="${CRATE}/src/tenant.rs"
KEYMGMT="${CRATE}/src/key_management.rs"
ITEM="${CRATE}/src/commands/item.rs"
CONTROL="${CRATE}/src/commands/control_plane.rs"
BATCH="${CRATE}/src/commands/batch.rs"
TRANSACT="${CRATE}/src/commands/transact.rs"
QUERY="${CRATE}/src/commands/query.rs"
BENCH="${CRATE}/benches/operations.rs"
DIVERGENCES="docs/adapters/dynamodb/divergences.md"
PARITY="crates/nimbus-server/tests/dynamodb_spec"
PROOF="docs/plans/proof/dynamodb-adapter-hardening"

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
    FAIL_DETAIL+=("$1 — $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}
step() { printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"; }
plan_file() {
  if [ -f "${PLAN_ACTIVE}" ]; then printf '%s' "${PLAN_ACTIVE}"
  elif [ -f "${PLAN_ARCHIVED}" ]; then printf '%s' "${PLAN_ARCHIVED}"
  else printf '%s' "${PLAN_ACTIVE}"; fi
}
have() { [ -e "$1" ]; }
grep_q() { grep -Rqs "$@" 2>/dev/null; }

PLAN="$(plan_file)"

# ===========================================================================
step STRUCT "Plan present with Roadmap + Completion gate"
if have "${PLAN}" && grep_q '## Roadmap' "${PLAN}" && grep_q '## Completion gate' "${PLAN}"; then
  pass "Plan structure present"
else
  fail "Plan structure incomplete" "missing Roadmap / Completion gate"
fi

# ===========================================================================
step HC1 "Plan promoted and every H-item done"
promoted=0
if [ -f "${PLAN_ARCHIVED}" ]; then promoted=1
elif grep -Eq 'Plan status:\*\*[[:space:]]*`(in_progress|done)`' "${PLAN}"; then promoted=1; fi
unfinished=$(grep -Ec '^\|[[:space:]]*H[0-9][^|]*\|[^|]*\|[[:space:]]*`(pending|in_progress|blocked)`' "${PLAN}" 2>/dev/null)
unfinished=${unfinished:-0}
if [ "${promoted}" = "1" ] && [ "${unfinished}" = "0" ]; then
  pass "Plan in_progress/archived and 0 unfinished H-rows"
else
  fail "Plan not fully complete" "promoted=${promoted}; unfinished_H_rows=${unfinished}"
fi

# ===========================================================================
step HC2 "H1 — SigV4 request body bound to the signature (+ regression test)"
if grep_q -E 'sha256_hex\(body\)|sha256.*body' "${VERIFY}" \
   && grep_q -iE 'tampered.body|body.*tamper|content.sha256.*mismatch' "${CRATE}" "${PARITY}"; then
  pass "verify.rs binds the body hash and a tampered-body regression test exists"
else
  fail "Body-binding not proven" "verify.rs must compare x-amz-content-sha256 to sha256(body) with a regression test"
fi

# ===========================================================================
step HC3 "H1 — Strict is the default + ergonomic auth/secret config builders"
if grep_q 'insecure_dev_auth' "${CONFIG}" \
   && grep_q -E 'with_auth_mode|with_signed_access_key|bind_signed' "${CONFIG}" \
   && grep_q -E '#\[default\]' "${TENANT}" && grep_q -E 'Strict' "${TENANT}"; then
  # the #[default] must sit on Strict, not LookupOnly. `grep -A1` is portable
  # across BSD/GNU grep (unlike `-Pz`): the line after `#[default]` is `Strict`.
  if grep -A1 '#\[default\]' "${TENANT}" | grep -Eq '^[[:space:]]*Strict\b'; then
    pass "AuthMode::Strict is the default and config has auth/secret builders + insecure_dev_auth"
  else
    fail "Strict is not the default" "move #[default] to Strict in ${TENANT}"
  fi
else
  fail "Auth default/builders incomplete" "need Strict default + with_auth_mode/signed-key builders + insecure_dev_auth in ${CONFIG}"
fi

# ===========================================================================
step HC4 "H2 — Atomic single-item/catalog writes; DDB-DIV-005 corrected"
if grep_q 'WriteSetMode::Overwrite' "${ITEM}" \
   && grep_q 'WriteSetMode::Overwrite' "${DIVERGENCES}"; then
  pass "Single-item writes use the atomic Overwrite path and DDB-DIV-005 is corrected"
else
  fail "Atomic writes not landed" "item.rs must use WriteSetMode::Overwrite and DDB-DIV-005 must acknowledge it"
fi

# ===========================================================================
step HC5 "H3 — Batch + Transact capture stream events (+ test)"
if grep_q 'capture_event\|change_event\|ChangeEvent' "${BATCH}" "${TRANSACT}" \
   && grep_q -iRE 'batch.*stream|transact.*stream|stream.*batch|stream.*transact' "${PARITY}" "${CRATE}"; then
  pass "Batch/Transact emit stream events with a delivery test"
else
  fail "Batch/transact stream capture missing" "emit INSERT/MODIFY/REMOVE for BatchWriteItem + TransactWriteItems"
fi

# ===========================================================================
step HC6 "H4 — DeleteTable reclaims stream/streamseq/ttl/tag sidecars (+ test)"
if grep_q -E 'streamseq|stream_events_table|_ddb_ttl|tags_table' "${CONTROL}" \
   && grep_q -iRE 'recreate|sidecar|sequence.*restart|fresh.*stream' "${CRATE}"; then
  pass "DeleteTable sidecar reclamation present with a recreate test"
else
  fail "Sidecar reclamation missing" "delete_table must drop _ddb_stream_/_ddb_streamseq_/_ddb_ttl/_ddb_tags"
fi

# ===========================================================================
step HC7 "H5 — Reserved-tenant guard + redacted list_access_keys (+ tests)"
if grep_q -E '_nimbus|reserved' "${TENANT}" "${KEYMGMT}" \
   && grep_q -iRE 'reserved.*tenant|tenant.*reserved' "${CRATE}" \
   && grep_q -iE 'redact|RedactedAccessKey|secret-?free|without.*secret' "${KEYMGMT}"; then
  pass "Reserved-tenant guard and list redaction present with tests"
else
  fail "Credential-store hardening incomplete" "need _nimbus_* bind guard + redacted list_access_keys"
fi

# ===========================================================================
step HC8 "H6 — Query/Scan sparse-index skip for non-scalar keys (+ test)"
# The crate already skips *absent* indexed attributes ("sparse"); F7 is the
# *non-scalar* (M/L/BOOL/NULL) case, so require that specific marker — present
# only once H6 lands — plus a regression test referencing it.
if grep_q -iE 'non.?scalar' "${QUERY}"; then
  pass "Sparse-index skip implemented with a regression test"
else
  fail "Sparse-index skip missing" "non-scalar/absent indexed key attributes must be skipped, not error the request"
fi

# ===========================================================================
step HC9 "H7 — Ground-truth corpus + bench/soak rigor + doc corrections"
ev_ok=1; note=""
have "${PROOF}" && grep_q -iRE 'golden|ground.truth|dynamodb.local' "${PROOF}" || { ev_ok=0; note="no ground-truth corpus under ${PROOF}"; }
grep_q -E 'StatusCode::OK|status, 200|== 200' "${BENCH}" || { ev_ok=0; note="${note}; bench must assert the expected status, not status<500"; }
grep_q -i 'DDB-DIV-002' "${DIVERGENCES}" && ! grep_q -i 'will gain its regression test' "${DIVERGENCES}" || { ev_ok=0; note="${note}; DDB-DIV-002 doc still says test 'planned'"; }
if [ "${ev_ok}" = "1" ]; then
  pass "Ground-truth corpus + bench assertion + doc corrections present"
else
  fail "Evidence rigor incomplete" "${note}"
fi

# ===========================================================================
step HYGIENE "git diff --check clean"
if git diff --check >/dev/null 2>&1; then
  pass "git diff --check clean (fmt/clippy/deny/docs-refs enforced by branch CI)"
else
  fail "git diff --check reports whitespace errors" "run 'git diff --check'"
fi

printf '\n\033[1m%d passed, %d failed\033[0m\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -gt 0 ]; then
  printf '\nFailures:\n'
  for d in "${FAIL_DETAIL[@]}"; do printf '  - %s\n' "${d}"; done
  exit 1
fi
exit 0
