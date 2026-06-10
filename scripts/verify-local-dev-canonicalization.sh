#!/usr/bin/env bash
# Aggregate completion-gate verifier for the local-dev canonicalization plan.
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# This script is the single-shell-exit-code stop condition for the
# `/goal` control plane that drives `docs/private/plans/local-dev-canonicalization-plan.md`
# (or `docs/private/plans/archive/local-dev-canonicalization-plan.md` after LD7).
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/local-dev-canonicalization-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/local-dev-canonicalization-plan.md"
OPERATING_DOC="docs/private/staging/operating/local-dev.md"
CI_YML=".github/workflows/ci.yml"
BUILD_RS="crates/nimbus-server/build.rs"
MAKEFILE="Makefile"
AGENTS_MD="CLAUDE.md"  # symlinks to AGENTS.md
PROOF_LOG="docs/private/plans/proof/local-dev-canonicalization/clean-tree-make-ci-required.log"

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

# Resolve the plan file (active or archived).
plan_file() {
  if [ -f "${PLAN_ACTIVE}" ]; then
    printf '%s\n' "${PLAN_ACTIVE}"
  elif [ -f "${PLAN_ARCHIVED}" ]; then
    printf '%s\n' "${PLAN_ARCHIVED}"
  else
    printf ''
  fi
}

# -------- conditions -------------------------------------------------------

printf '\033[1mLD verification gate — local-dev-canonicalization\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan checked in (active or archived).
step 1 "Plan checked in"
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  pass "Plan exists at ${PLAN_FILE}"
else
  fail "Plan missing" "Neither ${PLAN_ACTIVE} nor ${PLAN_ARCHIVED} exists"
fi

# 2. CI workflows have no inlined npm orchestration of nimbus-ui.
step 2 "CI workflows have no inlined npm orchestration of nimbus-ui"
if [ -f "${CI_YML}" ]; then
  MATCHES="$(grep -nE 'npm run (codegen|build) -w packages/nimbus-ui' "${CI_YML}" || true)"
  if [ -z "${MATCHES}" ]; then
    pass "${CI_YML} has zero \`npm run (codegen|build) -w packages/nimbus-ui\` matches"
  else
    fail "${CI_YML} still contains inlined npm orchestration" "${MATCHES}"
  fi
else
  fail "${CI_YML} missing"
fi

# 3. No fabricated HTML stub in build.rs.
# A stub manifests as an inline HTML doctype written by build.rs; the
# narrow regex avoids false positives like tonic-build's
# `.generate_default_stubs(true)` (gRPC server default-method-body
# generation, unrelated to the UI dist stub we want to forbid).
step 3 "build.rs has no fabricated HTML stub"
if [ -f "${BUILD_RS}" ]; then
  MATCHES="$(grep -niE '<!doctype html>' "${BUILD_RS}" || true)"
  if [ -z "${MATCHES}" ]; then
    pass "${BUILD_RS} has zero fabricated-HTML matches"
  else
    fail "${BUILD_RS} still emits a stub HTML doctype" "${MATCHES}"
  fi
else
  fail "${BUILD_RS} missing"
fi

# 4. Makefile encodes the dependency graph.
step 4 "Makefile encodes the UI dependency graph"
if [ -f "${MAKEFILE}" ]; then
  if grep -qE '^UI_DIST_INDEX' "${MAKEFILE}"; then
    pass "Makefile defines UI_DIST_INDEX"
  else
    fail "Makefile does not define UI_DIST_INDEX"
  fi

  # Targets that MUST depend on $(UI_DIST_INDEX). test-rust-runtime is
  # deliberately excluded (nimbus-runtime has zero workspace deps).
  REQUIRED_TARGETS=(check test clippy ci-required verify-desktop-ui test-rust-workspace test-rust-docs)
  MISSING=()
  for target in "${REQUIRED_TARGETS[@]}"; do
    LINE="$(grep -E "^${target}:" "${MAKEFILE}" || true)"
    if [ -z "${LINE}" ]; then
      MISSING+=("${target} (target not defined)")
    elif ! printf '%s\n' "${LINE}" | grep -q '\$(UI_DIST_INDEX)'; then
      MISSING+=("${target} (defined but missing \$(UI_DIST_INDEX) prereq)")
    fi
  done
  if [ ${#MISSING[@]} -eq 0 ]; then
    pass "All required targets depend on \$(UI_DIST_INDEX): ${REQUIRED_TARGETS[*]}"
  else
    fail "One or more required targets are missing the \$(UI_DIST_INDEX) prereq" "$(printf '%s; ' "${MISSING[@]}")"
  fi
else
  fail "${MAKEFILE} missing"
fi

# 5. Build contract documented.
step 5 "Build contract doc exists"
if [ -f "${OPERATING_DOC}" ]; then
  pass "${OPERATING_DOC} exists"
else
  fail "${OPERATING_DOC} missing"
fi

# 6. Routing entry exists in CLAUDE.md.
step 6 "Routing entry exists in CLAUDE.md"
if [ -f "${AGENTS_MD}" ] || [ -L "${AGENTS_MD}" ]; then
  if grep -q 'local-dev-canonicalization-plan' "${AGENTS_MD}"; then
    pass "${AGENTS_MD} references local-dev-canonicalization-plan"
  else
    fail "${AGENTS_MD} does not reference local-dev-canonicalization-plan"
  fi
else
  fail "${AGENTS_MD} missing"
fi

# 7. Fresh-clone proof captured.
step 7 "Fresh-clone proof log captured"
if [ -f "${PROOF_LOG}" ]; then
  pass "${PROOF_LOG} exists"
else
  fail "${PROOF_LOG} missing"
fi

# 8. Ledger rows all done.
# The pattern is anchored to end-of-line because ledger rows have the
# structure `| LDn | ... | <status> |`. Anchoring this way avoids
# matching prose elsewhere in the plan (e.g. the condition-8 description
# below quotes `| not started |` literally in backticks).
step 8 "Every ledger row is marked done"
if [ -n "${PLAN_FILE}" ]; then
  NOT_STARTED=$(grep -cE '\| not started \|$' "${PLAN_FILE}" || true)
  IN_PROGRESS=$(grep -cE '\| in_progress \|$' "${PLAN_FILE}" || true)
  # Trim whitespace that grep -c sometimes emits on macOS.
  NOT_STARTED=${NOT_STARTED// /}
  IN_PROGRESS=${IN_PROGRESS// /}
  if [ "${NOT_STARTED}" = "0" ] && [ "${IN_PROGRESS}" = "0" ]; then
    pass "All ledger rows in ${PLAN_FILE} are done"
  else
    fail "Ledger has unfinished rows" "not_started=${NOT_STARTED} in_progress=${IN_PROGRESS}"
  fi
else
  fail "Skipped: plan file not located"
fi

# 9. Branch state — all local commits pushed to main.
step 9 "Branch state — local commits pushed"
if git rev-parse --git-dir > /dev/null 2>&1; then
  if git rev-parse origin/main > /dev/null 2>&1; then
    UNPUSHED="$(git log --oneline origin/main..HEAD 2>/dev/null || true)"
    if [ -z "${UNPUSHED}" ]; then
      pass "No commits ahead of origin/main"
    else
      fail "Local commits not yet pushed to origin/main" "$(printf '%s\n' "${UNPUSHED}" | head -5)"
    fi
  else
    fail "origin/main not available (need a remote-tracking branch)"
  fi
else
  fail "Not inside a git repository"
fi

# 10. CI green on main (latest run).
step 10 "Latest CI run on main is green"
if command -v gh > /dev/null 2>&1; then
  GH_JSON="$(gh run list --branch main --workflow=CI --limit 1 --json conclusion,status,headSha,databaseId 2>/dev/null || true)"
  if [ -z "${GH_JSON}" ] || [ "${GH_JSON}" = "[]" ]; then
    fail "No CI runs found on main" "(gh authenticated? workflow name 'CI' present?)"
  else
    CONCLUSION="$(printf '%s' "${GH_JSON}" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d[0].get("conclusion") or "")')"
    STATUS="$(printf '%s' "${GH_JSON}" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d[0].get("status") or "")')"
    SHA="$(printf '%s' "${GH_JSON}" | python3 -c 'import sys,json; d=json.load(sys.stdin); print((d[0].get("headSha") or "")[:8])')"
    if [ "${CONCLUSION}" = "success" ]; then
      pass "Latest CI run on main is green (sha=${SHA})"
    elif [ "${STATUS}" = "in_progress" ] || [ "${STATUS}" = "queued" ] || [ "${STATUS}" = "pending" ]; then
      fail "Latest CI run on main is still ${STATUS} (sha=${SHA})" "Wait for it to finish before completion-gating"
    else
      fail "Latest CI run on main is not green" "status=${STATUS} conclusion=${CONCLUSION} sha=${SHA}"
    fi
  fi
else
  fail "gh CLI not installed — cannot verify CI conclusion"
fi

# -------- summary ----------------------------------------------------------

printf '\n\033[1mSummary:\033[0m %d passed, %d failed\n' "${PASS}" "${FAIL}"

if [ "${FAIL}" -gt 0 ]; then
  printf '\n\033[1mFailing conditions:\033[0m\n'
  for line in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${line}"
  done
  exit 1
fi

printf '\n\033[32mAll completion-gate conditions satisfied.\033[0m\n'
exit 0
