#!/usr/bin/env bash
# Aggregate completion-gate verifier for the CI PR-Wall Sub-15 plan
# (`docs/plans/ci-pr-wall-sub-15-plan.md`).
#
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# Ships in PW0 so /goal is verifiable from day one; PW1-PW5 progressively
# flip conditions from FAIL to PASS, PW6 archives the plan.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/plans/ci-pr-wall-sub-15-plan.md"
PLAN_ARCHIVED="docs/plans/archive/ci-pr-wall-sub-15-plan.md"
AGENTS_MD="CLAUDE.md"
PROOF_DIR="docs/plans/proof/ci-pr-wall-sub-15"
PROOF_PW0="${PROOF_DIR}/pw0-baseline.md"
PROOF_PW5="${PROOF_DIR}/pw5-green-proof.md"

CI_WF=".github/workflows/ci.yml"
COVERAGE_WF=".github/workflows/coverage.yml"
PR_WALL_DOC="docs/operating/ci-pr-wall.md"

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

printf '\033[1mPW verification gate — ci-pr-wall-sub-15\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan checked in with Status frontmatter.
step 1 "Plan checked in with Status frontmatter"
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  if grep -Eq '^Status:[[:space:]]+(active|complete|done)' "${PLAN_FILE}"; then
    pass "Plan exists at ${PLAN_FILE} with Status frontmatter"
  else
    fail "Plan missing Status frontmatter" "Expected 'Status: active|complete|done' in ${PLAN_FILE}"
  fi
else
  fail "Plan missing" "Neither ${PLAN_ACTIVE} nor ${PLAN_ARCHIVED} exists"
fi

# 2. Verifier script exists and is executable.
step 2 "Verifier script exists and is executable"
SELF="scripts/verify-ci-pr-wall-sub-15.sh"
if [ -x "${SELF}" ]; then
  pass "${SELF} exists and is executable"
else
  fail "Verifier not executable" "Expected ${SELF} with +x"
fi

# 3. Execution Log SHAs for PW0..PW6.
step 3 "Execution Log records 40-char SHAs for PW0..PW6"
if [ -n "${PLAN_FILE}" ]; then
  EXEC_LOG_MISSING=()
  for item in PW0 PW1 PW2 PW3 PW4 PW5 PW6; do
    # Match a markdown table row: | PWN | <40-hex> | ... |
    if ! grep -Eq "^\|[[:space:]]*${item}[[:space:]]*\|[[:space:]]*[0-9a-f]{40}[[:space:]]*\|" "${PLAN_FILE}"; then
      EXEC_LOG_MISSING+=("${item}")
    fi
  done
  if [ "${#EXEC_LOG_MISSING[@]}" -eq 0 ]; then
    pass "All of PW0..PW6 have an Execution Log entry with a 40-char SHA"
  else
    fail "Execution Log entries missing or pending" \
         "Items without 40-char SHA: ${EXEC_LOG_MISSING[*]}"
  fi
else
  fail "Cannot check Execution Log" "Plan file not found"
fi

# 4. Every libsql image reference (in ci.yml and coverage.yml if present)
#    is pinned to a non-:latest tag with a sibling # vX.Y.Z comment.
step 4 "All libsql image refs pinned to vX.Y.Z (ci.yml + coverage.yml)"
COND4_OK=1
COND4_FILES=()
[ -f "${CI_WF}" ] && COND4_FILES+=("${CI_WF}")
[ -f "${COVERAGE_WF}" ] && COND4_FILES+=("${COVERAGE_WF}")
if [ "${#COND4_FILES[@]}" -eq 0 ]; then
  fail "Neither ci.yml nor coverage.yml found" "Expected at least one workflow file"
  COND4_OK=0
else
  for f in "${COND4_FILES[@]}"; do
    while IFS= read -r refline; do
      LN="$(printf '%s\n' "${refline}" | cut -d: -f1)"
      if printf '%s\n' "${refline}" | grep -q 'libsql-server:latest'; then
        fail "libsql still pinned to :latest in ${f}" "Line ${LN}: $(printf '%s\n' "${refline}" | cut -d: -f2-)"
        COND4_OK=0
        continue
      fi
      # Comment may sit at the step boundary (above `- name:`); look ±10 lines.
      LO=$((LN > 10 ? LN - 10 : 1))
      HI=$((LN + 10))
      if ! sed -n "${LO},${HI}p" "${f}" | grep -Eq '#[[:space:]]*v[0-9]+\.[0-9]+\.[0-9]+'; then
        fail "libsql ref in ${f} missing # vX.Y.Z comment" "Add comment near line ${LN}"
        COND4_OK=0
      fi
    done < <(grep -nE 'libsql-server:[^[:space:]]+' "${f}")
  done
fi
if [ "${COND4_OK}" -eq 1 ]; then
  pass "All libsql refs pinned with vX.Y.Z comments"
fi

# 5. Coverage extracted from ci.yml into its own workflow with main + schedule triggers.
step 5 "Coverage extracted: coverage.yml exists, ci.yml has no Coverage jobs"
COND5_OK=1
if [ ! -f "${COVERAGE_WF}" ]; then
  fail "coverage.yml missing" "Expected ${COVERAGE_WF}"
  COND5_OK=0
else
  if ! grep -Eq '^[[:space:]]*schedule:' "${COVERAGE_WF}"; then
    fail "coverage.yml has no schedule trigger" "Expected 'schedule:' in on: block"
    COND5_OK=0
  fi
  if ! grep -Eq 'branches:[[:space:]]*\[[^]]*main' "${COVERAGE_WF}"; then
    fail "coverage.yml has no push.branches: [main] trigger" "Expected push trigger limited to main"
    COND5_OK=0
  fi
fi
if [ -f "${CI_WF}" ]; then
  if grep -Eq 'name:[[:space:]]+Coverage (shard|reducer)' "${CI_WF}"; then
    fail "ci.yml still contains Coverage jobs" "Expected coverage jobs removed from ${CI_WF}"
    COND5_OK=0
  fi
fi
if [ "${COND5_OK}" -eq 1 ]; then
  pass "coverage.yml ships with schedule + push.main; ci.yml has no Coverage jobs"
fi

# 6. ci.yml top-level concurrency cap protects main from cancellation.
# The existing block at PW0 land time cancels main too, which corrodes
# cache-save side effects. PW3 flips cancel-in-progress to a branch-conditional
# expression that never cancels refs/heads/main.
step 6 "ci.yml concurrency cap protects main (cancel-in-progress branch-conditional)"
if [ -f "${CI_WF}" ]; then
  CONCURRENCY_BLOCK="$(awk '
    /^concurrency:/ { inblock=1; print; next }
    inblock && /^[a-zA-Z]/ { inblock=0 }
    inblock { print }
  ' "${CI_WF}")"
  COND6_OK=1
  if [ -z "${CONCURRENCY_BLOCK}" ]; then
    fail "No top-level concurrency: block in ci.yml" "Expected a 'concurrency:' key at column 0"
    COND6_OK=0
  else
    if ! printf '%s\n' "${CONCURRENCY_BLOCK}" | grep -q 'group:'; then
      fail "concurrency block missing group:" "Expected a 'group:' key"
      COND6_OK=0
    fi
    if ! printf '%s\n' "${CONCURRENCY_BLOCK}" | grep -q 'github.ref'; then
      fail "concurrency.group does not reference github.ref" "Per-branch cancellation requires github.ref in group"
      COND6_OK=0
    fi
    # Require the cancel-in-progress to be either an explicit false on main
    # OR a branch-conditional expression that excludes refs/heads/main.
    CIP="$(printf '%s\n' "${CONCURRENCY_BLOCK}" | grep 'cancel-in-progress:' || true)"
    if [ -z "${CIP}" ]; then
      fail "concurrency block missing cancel-in-progress:" "Expected 'cancel-in-progress:' key"
      COND6_OK=0
    elif printf '%s\n' "${CIP}" | grep -Eq "cancel-in-progress:[[:space:]]*true[[:space:]]*$"; then
      fail "cancel-in-progress: true cancels main runs" \
           "Flip to: cancel-in-progress: \${{ github.ref != 'refs/heads/main' }}"
      COND6_OK=0
    elif ! printf '%s\n' "${CIP}" | grep -q 'refs/heads/main'; then
      # Either explicit false, or some other expression — require the main exclusion.
      fail "cancel-in-progress does not exclude refs/heads/main" \
           "Expected: cancel-in-progress: \${{ github.ref != 'refs/heads/main' }}"
      COND6_OK=0
    fi
  fi
  if [ "${COND6_OK}" -eq 1 ]; then
    pass "concurrency.cancel-in-progress excludes refs/heads/main"
  fi
else
  fail "ci.yml missing" "${CI_WF} not found"
fi

# 7. warm-sccache either removed (PW4b) or retained with a documented retention pointer (PW4c).
step 7 "warm-sccache decision documented (PW4b retired OR PW4c retained with rationale)"
if [ -f "${CI_WF}" ]; then
  if ! grep -Eq '^[[:space:]]+warm-sccache:[[:space:]]*$' "${CI_WF}"; then
    pass "warm-sccache job removed from ci.yml (PW4b retire path)"
  else
    # Look for a comment line referencing pw4c retention proof bundle near the job.
    if grep -Eq '#.*pw4c-warm-sccache-retained\.md' "${CI_WF}"; then
      pass "warm-sccache retained with PW4c rationale comment"
    else
      fail "warm-sccache retained without PW4c retention pointer" \
           "Add a '# see docs/plans/proof/ci-pr-wall-sub-15/pw4c-warm-sccache-retained.md' comment near the warm-sccache: job"
    fi
  fi
else
  fail "ci.yml missing" "${CI_WF} not found"
fi

# 8. pw5-green-proof.md exists with 3 runs and a measured wall threshold.
step 8 "PW5 green proof: 3 runs at wall ≤ 15m (or ≤ 18m if PW4c)"
if [ ! -f "${PROOF_PW5}" ]; then
  fail "${PROOF_PW5} missing" "Expected PW5 proof bundle"
else
  # Count "Run: <10-12 digit id>" lines.
  RUN_LINE_COUNT="$(grep -cE '^Run:[[:space:]]+[0-9]{10,12}' "${PROOF_PW5}" || true)"
  if [ "${RUN_LINE_COUNT}" -lt 3 ]; then
    fail "PW5 proof lists fewer than 3 runs" "Found ${RUN_LINE_COUNT} 'Run: <id>' lines; need 3"
  else
    # Determine PW4 path from ci.yml warm-sccache presence.
    if [ -f "${CI_WF}" ] && grep -Eq '^[[:space:]]+warm-sccache:[[:space:]]*$' "${CI_WF}"; then
      WALL_LIMIT_MIN=18
    else
      WALL_LIMIT_MIN=15
    fi
    # Each run line should be followed (within 5 lines) by a Wall: <Nm Ms> entry under threshold.
    OVER=0
    while IFS= read -r runline; do
      LN=$(printf '%s\n' "${runline}" | cut -d: -f1)
      WALL_LINE="$(sed -n "${LN},$((LN + 5))p" "${PROOF_PW5}" | grep -Eo 'Wall:[[:space:]]+[0-9]+m[[:space:]]*[0-9]+s' | head -n1)"
      if [ -z "${WALL_LINE}" ]; then
        OVER=$((OVER + 1))
        continue
      fi
      WMIN="$(printf '%s\n' "${WALL_LINE}" | grep -oE '[0-9]+m' | head -n1 | tr -d 'm')"
      WSEC="$(printf '%s\n' "${WALL_LINE}" | grep -oE '[0-9]+s' | head -n1 | tr -d 's')"
      TOTAL_SEC=$(( WMIN * 60 + ${WSEC:-0} ))
      LIMIT_SEC=$(( WALL_LIMIT_MIN * 60 ))
      if [ "${TOTAL_SEC}" -gt "${LIMIT_SEC}" ]; then
        OVER=$((OVER + 1))
      fi
    done < <(grep -nE '^Run:[[:space:]]+[0-9]{10,12}' "${PROOF_PW5}")
    if [ "${OVER}" -eq 0 ]; then
      pass "PW5 lists ≥3 runs all under ${WALL_LIMIT_MIN}m wall"
    else
      fail "PW5 has ${OVER} run(s) over the ${WALL_LIMIT_MIN}m wall limit" \
           "Check Wall: NNm NNs entries in ${PROOF_PW5}"
    fi
  fi
fi

# 9. Canonical contract doc exists with required sections.
step 9 "docs/operating/ci-pr-wall.md exists with Target / Pole attacks / Retain-retire-warm-sccache sections"
if [ ! -f "${PR_WALL_DOC}" ]; then
  fail "${PR_WALL_DOC} missing" "Expected canonical contract page"
else
  COND9_OK=1
  for sec in '## Target' '## Pole attacks' '## Retain'; do
    if ! grep -Fq "${sec}" "${PR_WALL_DOC}"; then
      fail "${PR_WALL_DOC} missing section '${sec}'" "Add the section header"
      COND9_OK=0
    fi
  done
  if [ "${COND9_OK}" -eq 1 ]; then
    pass "${PR_WALL_DOC} carries the three required sections"
  fi
fi

# 10. CLAUDE.md routing references ci-pr-wall-sub-15-plan in the CI modernization block.
step 10 "CLAUDE.md routing references ci-pr-wall-sub-15-plan"
if [ ! -f "${AGENTS_MD}" ]; then
  fail "${AGENTS_MD} missing" "Expected at repo root"
else
  if grep -Fq 'ci-pr-wall-sub-15-plan' "${AGENTS_MD}"; then
    pass "${AGENTS_MD} references ci-pr-wall-sub-15-plan"
  else
    fail "${AGENTS_MD} does not reference ci-pr-wall-sub-15-plan" \
         "Add a routing entry under the CI modernization / PR wall block"
  fi
fi

# -------- summary ----------------------------------------------------------

printf '\n\033[1mResult:\033[0m %d pass, %d fail\n' "${PASS}" "${FAIL}"

if [ "${FAIL}" -gt 0 ]; then
  printf '\n\033[1mOutstanding:\033[0m\n'
  for d in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${d}"
  done
  exit 1
fi
exit 0
