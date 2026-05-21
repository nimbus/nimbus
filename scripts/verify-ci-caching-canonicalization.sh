#!/usr/bin/env bash
# Aggregate completion-gate verifier for the CI caching canonicalization
# plan (`docs/plans/ci-caching-canonicalization-plan.md`).
#
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# This script is the single-shell-exit-code stop condition for the
# `/goal` control plane that drives the plan to done. It ships in CC0 so
# /goal is verifiable from day one; CC1-CC7 progressively flip
# conditions from FAIL to PASS, and CC8 archives the plan.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/plans/ci-caching-canonicalization-plan.md"
PLAN_ARCHIVED="docs/plans/archive/ci-caching-canonicalization-plan.md"
OPERATING_DOC="docs/operating/ci-caching.md"
AGENTS_MD="CLAUDE.md"  # symlinks to AGENTS.md
PROOF_DIR="docs/plans/proof/ci-caching-canonicalization"
PROOF_BASELINE_CACHES="${PROOF_DIR}/baseline-cache-state.json"
PROOF_BASELINE_TIMINGS="${PROOF_DIR}/baseline-coverage-timings.md"
PROOF_CC1="${PROOF_DIR}/cc1-coverage-only-stats.md"
PROOF_CC6="${PROOF_DIR}/cc6-link-parallelism.md"
PROOF_CC7="${PROOF_DIR}/cc7-no-doctests.md"

# Rust workflow files. Every Rust job in these must wire sccache once CC2 lands.
CI_YML=".github/workflows/ci.yml"
DESKTOP_UI_YML=".github/workflows/desktop-ui.yml"
NODE_COMPAT_YML=".github/workflows/node-compat-nightly.yml"

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

printf '\033[1mCC verification gate — ci-caching-canonicalization\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan checked in (active or archived).
step 1 "Plan checked in"
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  pass "Plan exists at ${PLAN_FILE}"
else
  fail "Plan missing" "Neither ${PLAN_ACTIVE} nor ${PLAN_ARCHIVED} exists"
fi

# 2. sccache wired into every Rust workflow that defines a Rust job.
step 2 "sccache wired into every Rust workflow"
SCCACHE_MISSING=()
for wf in "${CI_YML}" "${DESKTOP_UI_YML}" "${NODE_COMPAT_YML}"; do
  if [ ! -f "${wf}" ]; then
    SCCACHE_MISSING+=("${wf} (file missing)")
    continue
  fi
  if ! grep -qE 'mozilla-actions/sccache-action' "${wf}"; then
    SCCACHE_MISSING+=("${wf} (no sccache-action wired)")
  fi
done
if [ ${#SCCACHE_MISSING[@]} -eq 0 ]; then
  pass "sccache-action present in ci.yml, desktop-ui.yml, node-compat-nightly.yml"
else
  fail "sccache-action not wired into all Rust workflows" "$(printf '%s; ' "${SCCACHE_MISSING[@]}")"
fi

# 3. Swatinem cache keys rotated from v1 to v2 (forces dep-tree refresh
#    alongside sccache rollout).
step 3 "Swatinem shared-keys rotated to -v2"
if [ -f "${CI_YML}" ]; then
  V1_HITS=$(grep -cE 'shared-key:.*-v1' "${CI_YML}" || true)
  V2_HITS=$(grep -cE 'shared-key:.*-v2' "${CI_YML}" || true)
  V1_HITS=${V1_HITS// /}
  V2_HITS=${V2_HITS// /}
  if [ "${V1_HITS}" = "0" ] && [ "${V2_HITS}" -gt "0" ] 2>/dev/null; then
    pass "Zero -v1 shared-keys; ${V2_HITS} -v2 shared-key(s) present in ${CI_YML}"
  elif [ "${V1_HITS}" = "0" ] && [ "${V2_HITS}" = "0" ]; then
    fail "No Swatinem shared-keys found at all" "Expected at least one -v2 shared-key after CC2"
  else
    fail "${CI_YML} still has -v1 shared-keys" "v1_count=${V1_HITS} v2_count=${V2_HITS}"
  fi
else
  fail "${CI_YML} missing"
fi

# 4. Rerun-safe save policy on every Swatinem invocation.
step 4 "Swatinem invocations set save-always: true"
if [ -f "${CI_YML}" ]; then
  RUST_CACHE_HITS=$(grep -cE 'Swatinem/rust-cache' "${CI_YML}" || true)
  SAVE_ALWAYS_HITS=$(grep -cE 'save-always:\s*true' "${CI_YML}" || true)
  RUST_CACHE_HITS=${RUST_CACHE_HITS// /}
  SAVE_ALWAYS_HITS=${SAVE_ALWAYS_HITS// /}
  if [ "${RUST_CACHE_HITS}" = "0" ]; then
    fail "No Swatinem/rust-cache invocations found in ${CI_YML}" "Expected at least one after CC3"
  elif [ "${SAVE_ALWAYS_HITS}" -ge "${RUST_CACHE_HITS}" ] 2>/dev/null; then
    pass "save-always: true matches Swatinem invocation count (${SAVE_ALWAYS_HITS}/${RUST_CACHE_HITS})"
  else
    fail "Not every Swatinem invocation has save-always: true" "swatinem=${RUST_CACHE_HITS} save_always=${SAVE_ALWAYS_HITS}"
  fi
else
  fail "${CI_YML} missing"
fi

# 5. ui-artifacts leader job exists and is consumed by downstream Rust jobs.
step 5 "ui-artifacts leader job exists and downstream jobs consume it"
if [ -f "${CI_YML}" ]; then
  if grep -qE '^  ui-artifacts:' "${CI_YML}"; then
    NEEDS_HITS=$(grep -cE 'needs:.*ui-artifacts' "${CI_YML}" || true)
    NEEDS_HITS=${NEEDS_HITS// /}
    if [ "${NEEDS_HITS}" -ge "1" ] 2>/dev/null; then
      pass "ui-artifacts job defined and referenced by ${NEEDS_HITS} needs: clause(s)"
    else
      fail "ui-artifacts job defined but no downstream needs: reference"
    fi
  else
    fail "ui-artifacts leader job not defined in ${CI_YML}"
  fi
else
  fail "${CI_YML} missing"
fi

# 6. warm-sccache leader job exists and is consumed by downstream Rust jobs.
step 6 "warm-sccache leader job exists and downstream jobs consume it"
if [ -f "${CI_YML}" ]; then
  if grep -qE '^  warm-sccache:' "${CI_YML}"; then
    NEEDS_HITS=$(grep -cE 'needs:.*warm-sccache' "${CI_YML}" || true)
    NEEDS_HITS=${NEEDS_HITS// /}
    if [ "${NEEDS_HITS}" -ge "1" ] 2>/dev/null; then
      pass "warm-sccache job defined and referenced by ${NEEDS_HITS} needs: clause(s)"
    else
      fail "warm-sccache job defined but no downstream needs: reference"
    fi
  else
    fail "warm-sccache leader job not defined in ${CI_YML}"
  fi
else
  fail "${CI_YML} missing"
fi

# 7. Caching contract documented at docs/operating/ci-caching.md.
step 7 "Caching contract doc exists"
if [ -f "${OPERATING_DOC}" ]; then
  pass "${OPERATING_DOC} exists"
else
  fail "${OPERATING_DOC} missing"
fi

# 8. Routing entry exists in CLAUDE.md / AGENTS.md.
step 8 "Routing entry exists in CLAUDE.md"
if [ -f "${AGENTS_MD}" ] || [ -L "${AGENTS_MD}" ]; then
  if grep -q 'ci-caching-canonicalization-plan' "${AGENTS_MD}"; then
    pass "${AGENTS_MD} references ci-caching-canonicalization-plan"
  else
    fail "${AGENTS_MD} does not reference ci-caching-canonicalization-plan"
  fi
else
  fail "${AGENTS_MD} missing"
fi

# 9. All proof captures present (baseline + per-phase).
step 9 "Proof captures present"
PROOF_MISSING=()
for path in "${PROOF_BASELINE_CACHES}" "${PROOF_BASELINE_TIMINGS}" "${PROOF_CC1}" "${PROOF_CC6}" "${PROOF_CC7}"; do
  if [ ! -f "${path}" ]; then
    PROOF_MISSING+=("${path}")
  fi
done
if [ ${#PROOF_MISSING[@]} -eq 0 ]; then
  pass "All five proof artifacts present under ${PROOF_DIR}/"
else
  fail "Missing proof artifacts" "$(printf '%s; ' "${PROOF_MISSING[@]}")"
fi

# 10. Every ledger row marked done.
step 10 "Every ledger row is marked done"
if [ -n "${PLAN_FILE}" ]; then
  NOT_STARTED=$(grep -cE '\| not started \|$' "${PLAN_FILE}" || true)
  IN_PROGRESS=$(grep -cE '\| in_progress \|$' "${PLAN_FILE}" || true)
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

# 11. Branch state — all local commits pushed to main.
step 11 "Branch state — local commits pushed"
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

# 12. CI green on main (latest run).
step 12 "Latest CI run on main is green"
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
