#!/usr/bin/env bash
# Aggregate completion-gate verifier for the DynamoDB Adapter plan
# (`docs/plans/dynamodb-adapter-plan.md`).
#
# Exits 0 iff every condition in the plan's "## Completion Gate" is satisfied.
# Shipped in D0.0a so /goal is verifiable from day one: it FAILS on every
# unimplemented gate today and roadmap items D0..D9 progressively flip
# conditions from FAIL to PASS. D9.7 closes the plan with `N passed, 0 failed`.
#
# Philosophy (matches scripts/verify-node-dbus-binding.sh): this verifier proves
# the durable *artifacts and evidence* exist and are structurally complete. The
# heavy "it compiles / tests pass" proof is enforced by branch CI (the green
# `dynamodb-adapter` branch run is part of the /goal stop condition); this script
# does not run a full workspace build on every invocation.
#
# Run from the repo root (or anywhere — it cd's to the repo root).

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 2

# -------- expected artifact paths -----------------------------------------

PLAN_ACTIVE="docs/plans/dynamodb-adapter-plan.md"
PLAN_ARCHIVED="docs/plans/archive/dynamodb-adapter-plan.md"
START_PROMPT="docs/prompts/dynamodb-adapter-start.md"

CRATE_DIR="crates/nimbus-dynamodb"
CRATE_CARGO="${CRATE_DIR}/Cargo.toml"
ROOT_CARGO="Cargo.toml"
SERVER_CARGO="crates/nimbus-server/Cargo.toml"
SIGV4_DIR="${CRATE_DIR}/src/auth/sigv4"
NOTICE_FILE="NOTICE"

ADAPTER_DOCS="docs/adapters/dynamodb"
COVERAGE_DOC="${ADAPTER_DOCS}/feature-coverage.md"
SDK_DOC="${ADAPTER_DOCS}/sdk-compatibility.md"
DIVERGENCES_DOC="${ADAPTER_DOCS}/divergences.md"
ENTERPRISE_DOC="${ADAPTER_DOCS}/enterprise-readiness.md"

SDK_PKG_DIR="packages/dynamodb"
PARITY_DIR="crates/nimbus-server/tests/dynamodb_spec"
PROOF_DIR="docs/plans/proof/dynamodb-adapter"

PASS=0
FAIL=0
FAIL_DETAIL=()

# -------- helpers ----------------------------------------------------------

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

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
}

# plan_file: the active plan while pending/in_progress, the archived copy once
# D9.7 moves it.
plan_file() {
  if [ -f "${PLAN_ACTIVE}" ]; then
    printf '%s' "${PLAN_ACTIVE}"
  elif [ -f "${PLAN_ARCHIVED}" ]; then
    printf '%s' "${PLAN_ARCHIVED}"
  else
    printf '%s' "${PLAN_ACTIVE}"
  fi
}

# have <path>  -> file or dir exists
have() { [ -e "$1" ]; }

# grep_q [grep-flags] <pattern> <path...> -> match found (quiet, recursive).
# All args forward straight to grep so callers may pass flags like -i / -E.
grep_q() {
  grep -Rqs "$@" 2>/dev/null
}

PLAN="$(plan_file)"

# ===========================================================================
step C1 "Plan promoted and every roadmap item done at closeout"
# ---------------------------------------------------------------------------
promoted=0
if [ -f "${PLAN_ARCHIVED}" ]; then
  promoted=1
elif grep -Eq 'Plan status:\*\*[[:space:]]*`(in_progress|done)`' "${PLAN}"; then
  promoted=1
fi
# Count unfinished roadmap rows (rows like "| D0.0a ... | `pending` | ...").
unfinished=$(grep -Ec '^\|[[:space:]]*D[0-9][^|]*\|[[:space:]]*`(pending|in_progress|blocked)`' "${PLAN}" 2>/dev/null || printf '0')
if [ "${promoted}" = "1" ] && [ "${unfinished}" = "0" ]; then
  pass "Plan is in_progress/archived and 0 unfinished roadmap rows"
else
  fail "Plan not fully complete" "promoted=${promoted}; unfinished_roadmap_rows=${unfinished}"
fi

# ===========================================================================
step C2 "nimbus-dynamodb crate exists (compile-green proven by branch CI)"
# ---------------------------------------------------------------------------
if have "${CRATE_CARGO}" && grep_q 'name *= *"nimbus-dynamodb"' "${CRATE_CARGO}"; then
  pass "${CRATE_DIR} is a workspace member named nimbus-dynamodb"
else
  fail "nimbus-dynamodb crate absent" "expected ${CRATE_CARGO} with package name nimbus-dynamodb"
fi

# ===========================================================================
step C3 "Crate boundary: server depends on nimbus-dynamodb, not extenddb-core/axum in the adapter"
# ---------------------------------------------------------------------------
boundary_ok=1
note=""
if ! grep_q 'nimbus-dynamodb' "${SERVER_CARGO}"; then
  boundary_ok=0; note="nimbus-server does not depend on nimbus-dynamodb"
fi
if have "${CRATE_CARGO}" && grep -Eq '^[[:space:]]*axum[[:space:]]*=' "${CRATE_CARGO}"; then
  boundary_ok=0; note="${note}; nimbus-dynamodb must not depend on axum"
fi
# No stale "*-adapter"-suffixed crate names introduced.
if grep_q 'nimbus-dynamodb-adapter' "${ROOT_CARGO}"; then
  boundary_ok=0; note="${note}; stale *-adapter crate name present"
fi
if [ "${boundary_ok}" = "1" ] && have "${CRATE_CARGO}"; then
  pass "Crate boundary holds (server->nimbus-dynamodb; no axum in adapter; no stale names)"
else
  fail "Crate boundary not yet established" "${note:-crate absent}"
fi

# ===========================================================================
step C4 "Upstream attribution: extenddb-core pin + vendored SigV4 headers + NOTICE"
# ---------------------------------------------------------------------------
attr_ok=1; note=""
grep_q 'extenddb-core' "${ROOT_CARGO}" || { attr_ok=0; note="extenddb-core not pinned in root Cargo.toml"; }
grep_q 'ExtendDB' "${NOTICE_FILE}" || { attr_ok=0; note="${note}; ${NOTICE_FILE} missing ExtendDB entry"; }
if have "${SIGV4_DIR}"; then
  for f in mod canonical parse signing_key verify; do
    # Vendored files carry the SPDX identifier `Apache-2.0` (not the full
    # "Apache License" text); accept either form.
    if ! grep_q -E 'Apache-2\.0|Apache License' "${SIGV4_DIR}/${f}.rs"; then
      attr_ok=0; note="${note}; ${f}.rs missing Apache-2.0 header"
    fi
  done
else
  attr_ok=0; note="${note}; vendored SigV4 dir absent"
fi
if [ "${attr_ok}" = "1" ]; then
  pass "extenddb-core pinned, 5 SigV4 files carry Apache-2.0 headers, NOTICE covers ExtendDB"
else
  fail "Attribution incomplete" "${note}"
fi

# ===========================================================================
step C5 "Feature-coverage table for T0-T7 with per-op status"
# ---------------------------------------------------------------------------
if have "${COVERAGE_DOC}" && grep_q -i 'implemented\|classified-divergence\|unsupported-deferred' "${COVERAGE_DOC}"; then
  pass "${COVERAGE_DOC} exists with operation statuses"
else
  fail "Feature-coverage table absent" "expected ${COVERAGE_DOC}"
fi

# ===========================================================================
step C6 "Implemented operations carry request-shape/success/error/limit/malformed tests"
# ---------------------------------------------------------------------------
# Proxy: coverage doc present AND every 'implemented' row references a test lane.
if ! have "${COVERAGE_DOC}"; then
  fail "Operation test lanes not yet proven" "coverage doc absent"
elif ! grep -Eiq 'implemented' "${COVERAGE_DOC}"; then
  fail "No implemented operations yet" "coverage doc has no implemented rows"
elif grep_q 'NO-TEST' "${COVERAGE_DOC}"; then
  fail "An implemented operation lacks a test lane" "remove all NO-TEST markers"
else
  pass "Coverage doc present; implemented rows carry test lanes (no NO-TEST markers)"
fi

# ===========================================================================
step C7 "Pagination proofs (token roundtrip, exhausted, invalid-token, SDK paginator)"
# ---------------------------------------------------------------------------
if grep_q -i 'LastEvaluatedKey\|ExclusiveStartKey' "${PARITY_DIR}" 2>/dev/null; then
  pass "Pagination scenarios present in parity runner"
else
  fail "Pagination proofs absent" "expected pagination scenarios under ${PARITY_DIR}"
fi

# ===========================================================================
step C8 "Batch/transaction partial-failure + cancellation-reason + atomicity proofs"
# ---------------------------------------------------------------------------
if grep_q -i 'UnprocessedItems\|UnprocessedKeys\|CancellationReasons' "${PARITY_DIR}" 2>/dev/null; then
  pass "Batch/transaction envelope scenarios present"
else
  fail "Batch/transaction proofs absent" "expected partial-failure/cancellation scenarios"
fi

# ===========================================================================
step C9 "Official SDK matrix (AWS CLI, JS v3, Rust, Python) with versions + counts"
# ---------------------------------------------------------------------------
sdk_ok=1; note=""
if have "${SDK_DOC}"; then
  for c in 'AWS CLI' 'client-dynamodb|JS|JavaScript' 'aws-sdk-dynamodb|Rust' 'boto3|Python'; do
    grep -Eiq -- "${c}" "${SDK_DOC}" || { sdk_ok=0; note="${note}; missing client ${c}"; }
  done
else
  sdk_ok=0; note="${SDK_DOC} absent"
fi
if [ "${sdk_ok}" = "1" ]; then
  pass "SDK compatibility matrix records all four official clients"
else
  fail "SDK matrix incomplete" "${note}"
fi

# ===========================================================================
step C10 "Parity classification report committed; 0 unclassified; divergences documented"
# ---------------------------------------------------------------------------
if have "${PROOF_DIR}" && grep_q -i 'classification\|pass\|divergence' "${PROOF_DIR}" 2>/dev/null && have "${DIVERGENCES_DOC}"; then
  if grep_q -i 'unclassified' "${PROOF_DIR}" 2>/dev/null; then
    fail "Parity report has unclassified diffs" "resolve every unclassified entry"
  else
    pass "Parity classification report committed; divergences documented"
  fi
else
  fail "Parity report / divergences doc absent" "expected ${PROOF_DIR} + ${DIVERGENCES_DOC}"
fi

# ===========================================================================
step C11 "SigV4 strict mode verifies signed requests and rejects malformed/expired"
# ---------------------------------------------------------------------------
if grep_q -i 'InvalidSignatureException\|SigV4Strict\|signature' "${CRATE_DIR}" 2>/dev/null \
   && grep_q -i 'sigv4\|signature' "${CRATE_DIR}/tests" 2>/dev/null; then
  pass "SigV4 strict verification + rejection tests present"
else
  fail "SigV4 strict-mode proof absent" "expected SigV4 verify + reject tests in ${CRATE_DIR}"
fi

# ===========================================================================
step C12 "Tenant isolation proof across >= 2 access keys"
# ---------------------------------------------------------------------------
if grep_q -i 'tenant' "${PARITY_DIR}" 2>/dev/null && grep_q -i 'isolation\|cross-tenant\|two.tenant' "${CRATE_DIR}" "${PARITY_DIR}" 2>/dev/null; then
  pass "Tenant-isolation scenarios present"
else
  fail "Tenant-isolation proof absent" "expected two-tenant cross-access tests"
fi

# ===========================================================================
step C13 "Failure-injection / cancellation: fail closed, 0 panics, 0 unclassified 5xx"
# ---------------------------------------------------------------------------
if grep_q -i 'malformed\|cancel\|failure.injection\|oversize' "${CRATE_DIR}/tests" "${PARITY_DIR}" 2>/dev/null; then
  pass "Failure-injection/cancellation tests present"
else
  fail "Failure-injection proof absent" "expected malformed/cancellation/oversize tests"
fi

# ===========================================================================
step C14 "Mixed-workload soak report (counts, task/memory deltas, 0 panics/leaks)"
# ---------------------------------------------------------------------------
if grep_q -i 'soak' "${PROOF_DIR}" "${ENTERPRISE_DOC}" 2>/dev/null; then
  pass "Soak report present"
else
  fail "Soak report absent" "expected a committed soak report under ${PROOF_DIR}"
fi

# ===========================================================================
step C15 "Benchmark baseline for every T0-T7 op family with p50/p95/p99 + thresholds"
# ---------------------------------------------------------------------------
if grep_q -i 'p50\|p95\|p99\|benchmark\|baseline' "${PROOF_DIR}" 2>/dev/null && have "${CRATE_DIR}/benches"; then
  pass "Benchmark baseline committed"
else
  fail "Benchmark baseline absent" "expected ${CRATE_DIR}/benches + committed baseline"
fi

# ===========================================================================
step C16 "@nimbus/dynamodb package builds (ESM/CJS/types) and selftest passes"
# ---------------------------------------------------------------------------
if have "${SDK_PKG_DIR}/package.json" && grep_q '"@nimbus/dynamodb"' "${SDK_PKG_DIR}/package.json"; then
  pass "${SDK_PKG_DIR} package present"
else
  fail "@nimbus/dynamodb package absent" "expected ${SDK_PKG_DIR}/package.json"
fi

# ===========================================================================
step C17 "Five DynamoDB verification-harness cases in PR + nightly lanes"
# ---------------------------------------------------------------------------
harness_hits=0
# Search only real registration sites (harness code + CI workflows), NOT the
# plan docs — a case being *named* in the plan is not registration.
for c in dynamodb-wire-handshake-and-control-plane dynamodb-item-crud-roundtrip \
         dynamodb-query-scan-with-pagination dynamodb-transact-write-commit-abort \
         dynamodb-streams-event-delivery; do
  grep_q "${c}" crates .github/workflows 2>/dev/null && harness_hits=$((harness_hits + 1))
done
if [ "${harness_hits}" -ge 5 ]; then
  pass "All five DynamoDB harness cases registered in harness code + CI"
else
  fail "Harness cases not all registered" "found ${harness_hits}/5 in crates/.github (plan mentions don't count)"
fi

# ===========================================================================
step C18 "External-suite registry + canary-app matrix with pins/lanes/counts"
# ---------------------------------------------------------------------------
if grep_q -i 'external-suite\|canary' "${PROOF_DIR}" "${ADAPTER_DOCS}" 2>/dev/null; then
  pass "External-suite registry / canary matrix recorded"
else
  fail "External-suite/canary registry absent" "expected registry under ${ADAPTER_DOCS} or ${PROOF_DIR}"
fi

# ===========================================================================
step C19 "Enterprise-readiness closeout document"
# ---------------------------------------------------------------------------
if have "${ENTERPRISE_DOC}"; then
  pass "${ENTERPRISE_DOC} exists"
else
  fail "Enterprise-readiness doc absent" "expected ${ENTERPRISE_DOC}"
fi

# ===========================================================================
step C20 "Repo hygiene: no whitespace errors in the working tree diff"
# ---------------------------------------------------------------------------
# fmt/clippy/strict-docs-refs are the heavy gates; they run in branch CI (part of
# the /goal stop condition). The verifier checks the cheap, always-runnable one.
if git diff --check >/dev/null 2>&1; then
  pass "git diff --check clean (fmt/clippy/docs-refs enforced by branch CI)"
else
  fail "git diff --check reports whitespace errors" "run 'git diff --check'"
fi

# ===========================================================================
step C21 "No soft evidence: every implemented coverage row has a test; 0 unclassified parity diffs"
# ---------------------------------------------------------------------------
soft_ok=1; note=""
if ! have "${COVERAGE_DOC}"; then
  soft_ok=0; note="coverage doc absent"
else
  grep_q 'NO-TEST' "${COVERAGE_DOC}" && { soft_ok=0; note="implemented row(s) without a test lane"; }
fi
if have "${PROOF_DIR}" && grep_q -i 'unclassified' "${PROOF_DIR}" 2>/dev/null; then
  soft_ok=0; note="${note}; unclassified parity diffs remain"
fi
if [ "${soft_ok}" = "1" ] && have "${COVERAGE_DOC}"; then
  pass "No soft evidence: all implemented rows tested, 0 unclassified diffs"
else
  fail "Soft-evidence guard not yet satisfiable" "${note}"
fi

# ===========================================================================
step STRUCT "Plan-structure preflight (control-plane scaffold present)"
# ---------------------------------------------------------------------------
if have "${PLAN}" && grep_q '## Goal Control Plane' "${PLAN}" && grep_q '## Completion Gate' "${PLAN}"; then
  pass "Plan present with Goal Control Plane + Completion Gate sections"
else
  fail "Plan structure incomplete" "missing Goal Control Plane / Completion Gate"
fi
if have "${START_PROMPT}"; then
  pass "Startup prompt ${START_PROMPT} present"
else
  fail "Startup prompt absent" "expected ${START_PROMPT}"
fi

# -------- summary ----------------------------------------------------------

printf '\n\033[1m%d passed, %d failed\033[0m\n' "${PASS}" "${FAIL}"

if [ "${FAIL}" -gt 0 ]; then
  printf '\nFailures:\n'
  for d in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${d}"
  done
  exit 1
fi

exit 0
