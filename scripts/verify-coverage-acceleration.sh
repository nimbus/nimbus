#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Coverage Acceleration plan
# (`docs/plans/coverage-acceleration-plan.md`).
#
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# Ships in CA0 so /goal is verifiable from day one; CA1-CA4 progressively
# flip conditions from FAIL to PASS, CA5 archives the plan.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/plans/coverage-acceleration-plan.md"
PLAN_ARCHIVED="docs/plans/archive/coverage-acceleration-plan.md"
AGENTS_MD="CLAUDE.md"  # symlinks to AGENTS.md
PROOF_DIR="docs/plans/proof/coverage-acceleration"
PROOF_CA0="${PROOF_DIR}/ca0-baseline.md"

COMPOSITE_ACTION=".github/actions/setup-rust-cached/action.yml"
CI_WF=".github/workflows/ci.yml"
RELEASE_WF=".github/workflows/release.yml"

WORKFLOWS_GLOB=".github/workflows"

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

printf '\033[1mCA verification gate — coverage-acceleration\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan checked in (active or archived).
step 1 "Plan checked in"
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  pass "Plan exists at ${PLAN_FILE}"
else
  fail "Plan missing" "Neither ${PLAN_ACTIVE} nor ${PLAN_ARCHIVED} exists"
fi

# 2. Routing entry exists in CLAUDE.md / AGENTS.md.
step 2 "Routing entry exists in CLAUDE.md"
if [ -f "${AGENTS_MD}" ] || [ -L "${AGENTS_MD}" ]; then
  if grep -q 'coverage-acceleration-plan' "${AGENTS_MD}"; then
    pass "${AGENTS_MD} references coverage-acceleration-plan"
  else
    fail "${AGENTS_MD} does not reference coverage-acceleration-plan"
  fi
else
  fail "${AGENTS_MD} missing"
fi

# 3. CA0 baseline proof exists.
step 3 "CA0 baseline proof present"
if [ -f "${PROOF_CA0}" ]; then
  pass "${PROOF_CA0} exists"
else
  fail "${PROOF_CA0} missing"
fi

# 4. Composite action installs mold and exports the linker env var (CA1).
#    Accept either CARGO_TARGET_*_LINKER=mold or
#    CARGO_TARGET_*_RUSTFLAGS=-C link-arg=-fuse-ld=mold (the canonical
#    Rust mold invocation pattern — see composite-action comment for why
#    LINKER=mold trips `mold: fatal: unknown -m argument: 64`).
step 4 "Composite action installs mold + exports linker env"
if [ -f "${COMPOSITE_ACTION}" ]; then
  has_mold=0
  has_linker_env=0
  if grep -qE '(apt-get|apt)[[:space:]]+install[^|&]*[[:space:]]mold([[:space:]]|$)' "${COMPOSITE_ACTION}"; then
    has_mold=1
  fi
  if grep -qE 'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER[[:space:]]*[:=][[:space:]]*"?mold"?' "${COMPOSITE_ACTION}"; then
    has_linker_env=1
  elif grep -qE 'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS[[:space:]]*[:=][^"]*-fuse-ld=mold' "${COMPOSITE_ACTION}"; then
    has_linker_env=1
  fi
  if [ "${has_mold}" = "1" ] && [ "${has_linker_env}" = "1" ]; then
    pass "Composite installs mold and exports linker env var"
  else
    fail "Composite missing mold setup" "install=${has_mold} linker_env=${has_linker_env}"
  fi
else
  fail "${COMPOSITE_ACTION} missing"
fi

# 5. Coverage step is not -j 1 anymore (CA2 acceptance signal).
#    Accept any of: no -j flag, -j 2/4/8, or an inline comment block tagging
#    `CA2-disposition:` to document an intentional regression-keep.
step 5 "Coverage step parallelism unlocked (post-CA2)"
if [ -f "${CI_WF}" ]; then
  # Find the cargo llvm-cov invocation line.
  cov_line="$(grep -nE '^[[:space:]]*run:[[:space:]]*cargo llvm-cov' "${CI_WF}" | head -1 || true)"
  if [ -z "${cov_line}" ]; then
    fail "Coverage run-line not found in ${CI_WF}"
  else
    lineno="${cov_line%%:*}"
    content="${cov_line#*:}"
    if printf '%s' "${content}" | grep -qE 'cargo llvm-cov[[:space:]]+-j[[:space:]]+1([[:space:]]|$)'; then
      # Allow explicit CA2 disposition comment within the 12 lines above.
      window_start=$((lineno - 12))
      if [ "${window_start}" -lt 1 ]; then window_start=1; fi
      if sed -n "${window_start},${lineno}p" "${CI_WF}" | grep -qE 'CA2-disposition:'; then
        pass "Coverage still -j 1 but documents CA2-disposition tag (intentional)"
      else
        fail "Coverage step still pins -j 1 with no CA2-disposition tag" "${CI_WF}:${lineno}"
      fi
    else
      pass "Coverage step no longer pinned to -j 1 (${CI_WF}:${lineno})"
    fi
  fi
else
  fail "${CI_WF} missing"
fi

# 6. Coverage job is sharded (matrix N>=2 plus reducer that calls
#    `cargo llvm-cov report`) (CA3).
step 6 "Coverage job sharded with reducer (CA3)"
if [ -f "${CI_WF}" ]; then
  has_matrix_shard=0
  has_reducer=0
  if grep -qE '^[[:space:]]+(shard|group):[[:space:]]*\[' "${CI_WF}"; then
    has_matrix_shard=1
  fi
  if grep -qE 'cargo llvm-cov report' "${CI_WF}"; then
    has_reducer=1
  fi
  if [ "${has_matrix_shard}" = "1" ] && [ "${has_reducer}" = "1" ]; then
    pass "Coverage job declares shard matrix and reducer calls cargo llvm-cov report"
  else
    fail "Coverage job is not sharded with a reducer" "shard_matrix=${has_matrix_shard} reducer=${has_reducer}"
  fi
else
  fail "${CI_WF} missing"
fi

# 7. Zero inline `Swatinem/rust-cache` references in release.yml (CA4 —
#    release uses the composite exclusively).
step 7 "release.yml uses composite (no inline Swatinem/rust-cache)"
if [ -f "${RELEASE_WF}" ]; then
  inline=$(grep -cE 'uses:[[:space:]]*Swatinem/rust-cache' "${RELEASE_WF}" 2>/dev/null || true)
  inline=${inline// /}
  if [ "${inline}" = "0" ]; then
    pass "Zero inline Swatinem/rust-cache references in ${RELEASE_WF}"
  else
    fail "release.yml still has ${inline} inline Swatinem/rust-cache reference(s)"
  fi
else
  fail "${RELEASE_WF} missing"
fi

# 8. Zero inline `mozilla-actions/sccache-action` references workflow-wide
#    (CM-era invariant; CA4 regression gate).
step 8 "Zero inline sccache-action references workflow-wide"
INLINE_SCCACHE=0
INLINE_FILES=()
while IFS= read -r wf; do
  [ -z "${wf}" ] && continue
  hits=$(grep -cE 'uses:[[:space:]]*mozilla-actions/sccache-action' "${wf}" 2>/dev/null || true)
  hits=${hits// /}
  if [ "${hits}" -gt 0 ] 2>/dev/null; then
    INLINE_SCCACHE=$((INLINE_SCCACHE + hits))
    INLINE_FILES+=("${wf}(${hits})")
  fi
done < <(find "${WORKFLOWS_GLOB}" -maxdepth 1 -name '*.yml' -type f | sort)
if [ "${INLINE_SCCACHE}" = "0" ]; then
  pass "Zero inline sccache-action references in workflow files"
else
  fail "Inline sccache-action references remain" "$(printf '%s; ' "${INLINE_FILES[@]}")"
fi

# 9. Every ledger row marked done.
step 9 "Every ledger row is marked done"
if [ -n "${PLAN_FILE}" ]; then
  PENDING=$(grep -cE '\| pending \|$' "${PLAN_FILE}" || true)
  IN_PROGRESS=$(grep -cE '\| in_progress \|$' "${PLAN_FILE}" || true)
  PENDING=${PENDING// /}
  IN_PROGRESS=${IN_PROGRESS// /}
  if [ "${PENDING}" = "0" ] && [ "${IN_PROGRESS}" = "0" ]; then
    pass "All ledger rows in ${PLAN_FILE} are done"
  else
    fail "Ledger has unfinished rows" "pending=${PENDING} in_progress=${IN_PROGRESS}"
  fi
else
  fail "Skipped: plan file not located"
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
