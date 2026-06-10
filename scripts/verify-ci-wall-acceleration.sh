#!/usr/bin/env bash
# Aggregate completion-gate verifier for the CI Wall Acceleration plan
# (`docs/private/plans/ci-wall-acceleration-plan.md`).
#
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# Ships in CW0 so /goal is verifiable from day one; CW1-CW4 progressively
# flip conditions from FAIL to PASS, CW5 archives the plan.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/ci-wall-acceleration-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/ci-wall-acceleration-plan.md"
AGENTS_MD="CLAUDE.md"
PROOF_DIR="docs/private/plans/proof/ci-wall-acceleration"
PROOF_CW0="${PROOF_DIR}/cw0-baseline.md"

HARNESS_SCRIPT="scripts/verification-harness.sh"
CI_WF=".github/workflows/ci.yml"
MODERN_DOC="docs/private/staging/operating/ci-modernization.md"

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

printf '\033[1mCW verification gate — ci-wall-acceleration\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan checked in.
step 1 "Plan checked in"
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  pass "Plan exists at ${PLAN_FILE}"
else
  fail "Plan missing" "Neither ${PLAN_ACTIVE} nor ${PLAN_ARCHIVED} exists"
fi

# 2. Routing entry exists in CLAUDE.md.
step 2 "Routing entry exists in CLAUDE.md"
if [ -f "${AGENTS_MD}" ] || [ -L "${AGENTS_MD}" ]; then
  if grep -q 'ci-wall-acceleration-plan' "${AGENTS_MD}"; then
    pass "${AGENTS_MD} references ci-wall-acceleration-plan"
  else
    fail "${AGENTS_MD} does not reference ci-wall-acceleration-plan"
  fi
else
  fail "${AGENTS_MD} missing"
fi

# 3. CW0 baseline proof exists.
step 3 "CW0 baseline proof present"
if [ -f "${PROOF_CW0}" ]; then
  pass "${PROOF_CW0} exists"
else
  fail "${PROOF_CW0} missing"
fi

# 4. CW1: harness script accepts shard arg AND test honors NIMBUS_HARNESS_SHARD.
step 4 "Harness script accepts shard arg + corpus test honors NIMBUS_HARNESS_SHARD"
if [ -f "${HARNESS_SCRIPT}" ]; then
  has_shard_arg=0
  has_env_propagation=0
  if grep -qE 'NIMBUS_HARNESS_SHARD' "${HARNESS_SCRIPT}"; then
    has_env_propagation=1
  fi
  # Portable check (BSD grep on macOS lacks `\<`/`\>` anchors): the script
  # must reference a third positional arg ($3 or "$3" or ${3:-}) inside the
  # required|nightly branch or call validate_shard_spec.
  if grep -qE 'validate_shard_spec|"\$3"|\$\{3' "${HARNESS_SCRIPT}"; then
    has_shard_arg=1
  fi
  has_corpus_filter=0
  for src in \
    crates/nimbus-storage/src/simulation/verification.rs \
    crates/nimbus-server/src/tests/verification_harness.rs \
    crates/nimbus-runtime/src/runtime/tests/verification_harness.rs; do
    if [ -f "${src}" ] && grep -qE 'NIMBUS_HARNESS_SHARD|VERIFICATION_SHARD_ENV' "${src}"; then
      has_corpus_filter=1
    fi
  done
  if [ "${has_shard_arg}" = "1" ] && [ "${has_env_propagation}" = "1" ] && [ "${has_corpus_filter}" = "1" ]; then
    pass "Harness script accepts shard arg, propagates NIMBUS_HARNESS_SHARD, corpus filter present"
  else
    fail "Harness sharding wiring incomplete" "shard_arg=${has_shard_arg} env_propagation=${has_env_propagation} corpus_filter=${has_corpus_filter}"
  fi
else
  fail "${HARNESS_SCRIPT} missing"
fi

# Helper: extract a single top-level job block from ci.yml. Uses a flag so the
# end-pattern doesn't match the start line (BSD awk range patterns include the
# start in the end test, which collapses the range to one line when both
# patterns shape the same).
job_block() {
  local job="$1"
  local file="$2"
  awk -v job="^  ${job}:$" '
    $0 ~ job {flag=1; print; next}
    flag && /^  [a-z][a-z-]*:[[:space:]]*$/ {flag=0}
    flag {print}
  ' "${file}"
}

# 5. CW1: harness job matrix in ci.yml has per-surface shard expansion.
step 5 "harness job matrix includes per-surface shard expansion"
if [ -f "${CI_WF}" ]; then
  HARNESS_BLOCK="$(job_block harness "${CI_WF}")"
  has_shard_axis=0
  if printf '%s\n' "${HARNESS_BLOCK}" | grep -qE '^[[:space:]]+shard:'; then
    has_shard_axis=1
  fi
  has_shard_include=0
  surface_entries=$(printf '%s\n' "${HARNESS_BLOCK}" | grep -cE '^[[:space:]]+-[[:space:]]+surface:' || true)
  surface_entries=${surface_entries// /}
  if [ "${surface_entries}" -ge 6 ] 2>/dev/null; then
    has_shard_include=1
  fi
  if [ "${has_shard_axis}" = "1" ] || [ "${has_shard_include}" = "1" ]; then
    pass "harness matrix includes per-surface shard expansion (${surface_entries} surface entries)"
  else
    fail "harness matrix not yet expanded with shard entries" "shard_axis=${has_shard_axis} expanded_surface_count=${surface_entries}"
  fi
else
  fail "${CI_WF} missing"
fi

# 6. CW2: rust-workspace-tests uses nextest --partition + matrix.
#    The partition can be either inline in ci.yml (`--partition hash:N/M`) or
#    forwarded through Makefile via the NIMBUS_NEXTEST_PARTITION env var that
#    ci.yml sets per shard. Both are valid wirings.
step 6 "rust-workspace-tests uses nextest --partition with matrix"
if [ -f "${CI_WF}" ]; then
  WORKSPACE_BLOCK="$(job_block rust-workspace-tests "${CI_WF}")"
  has_partition=0
  has_workspace_shard_matrix=0
  if printf '%s\n' "${WORKSPACE_BLOCK}" | grep -qE 'nextest run.*--partition|NIMBUS_NEXTEST_PARTITION'; then
    has_partition=1
  fi
  if [ "${has_partition}" = "1" ]; then
    # Also ensure the Makefile actually forwards the env var to nextest.
    if ! grep -qE 'NIMBUS_NEXTEST_PARTITION.*--partition|--partition.*NIMBUS_NEXTEST_PARTITION|partition hash:\$\(NIMBUS_NEXTEST_PARTITION' Makefile 2>/dev/null; then
      # Allow inline --partition in ci.yml without Makefile coupling.
      if ! printf '%s\n' "${WORKSPACE_BLOCK}" | grep -qE 'nextest run.*--partition'; then
        has_partition=0
      fi
    fi
  fi
  if printf '%s\n' "${WORKSPACE_BLOCK}" | grep -qE '^[[:space:]]+shard:|^[[:space:]]+partition:|^[[:space:]]+-[[:space:]]+partition:|^[[:space:]]+-[[:space:]]+shard:'; then
    has_workspace_shard_matrix=1
  fi
  if [ "${has_partition}" = "1" ] && [ "${has_workspace_shard_matrix}" = "1" ]; then
    pass "rust-workspace-tests uses nextest --partition and matrix shard axis"
  else
    fail "rust-workspace-tests not yet sharded with nextest --partition" "partition=${has_partition} matrix=${has_workspace_shard_matrix}"
  fi
else
  fail "${CI_WF} missing"
fi

# 7. CW3: external-provider-tests job has provider matrix axis with ≥ 3 entries.
step 7 "external-provider-tests job has provider matrix with ≥ 3 entries"
if [ -f "${CI_WF}" ]; then
  # Try both common job names.
  PROVIDER_BLOCK="$(job_block external-provider-tests "${CI_WF}")"
  if [ -z "${PROVIDER_BLOCK}" ]; then
    PROVIDER_BLOCK="$(job_block external-provider "${CI_WF}")"
  fi
  PROVIDER_AXIS_COUNT=$(printf '%s\n' "${PROVIDER_BLOCK}" | grep -cE '^[[:space:]]+-[[:space:]]+provider:[[:space:]]+[a-z]+' || true)
  PROVIDER_AXIS_COUNT=${PROVIDER_AXIS_COUNT// /}
  if [ "${PROVIDER_AXIS_COUNT}" -ge 3 ] 2>/dev/null; then
    pass "external-provider job has provider matrix axis with ${PROVIDER_AXIS_COUNT} entries"
  else
    fail "external-provider job not yet split by provider" "provider_entries=${PROVIDER_AXIS_COUNT}"
  fi
else
  fail "${CI_WF} missing"
fi

# 8. CW4: warm-sccache lane documented + one of the two lanes landed.
step 8 "Warm-sccache lane documented in ci-modernization.md and landed"
if [ -f "${MODERN_DOC}" ]; then
  has_doc=0
  if grep -qiE 'pr critical-path acceleration|warm-sccache.*tests|warm-sccache.*target' "${MODERN_DOC}"; then
    has_doc=1
  fi
  has_landing=0
  WARM_BLOCK="$(job_block warm-sccache "${CI_WF}")"
  # Lane (a): --tests dropped from warm-sccache's cargo invocation.
  if printf '%s\n' "${WARM_BLOCK}" | grep -qE 'cargo check[[:space:]]+--workspace[[:space:]]*$|cargo check[[:space:]]+--workspace[[:space:]]+[^-]'; then
    if ! printf '%s\n' "${WARM_BLOCK}" | grep -qE 'cargo check[[:space:]]+--workspace[[:space:]]+--tests'; then
      has_landing=1
    fi
  fi
  # Lane (b): warm-sccache job has a `target/` cache restore step beyond Swatinem's default.
  if printf '%s\n' "${WARM_BLOCK}" | grep -qiE 'actions/cache.*target/|target-cache|warm-target-cache'; then
    has_landing=1
  fi
  if [ "${has_doc}" = "1" ] && [ "${has_landing}" = "1" ]; then
    pass "Warm-sccache lane documented and landed"
  else
    fail "Warm-sccache lane not yet documented + landed" "doc=${has_doc} landed=${has_landing}"
  fi
else
  fail "${MODERN_DOC} missing"
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
