#!/usr/bin/env bash
# Aggregate completion-gate verifier for the CI modernization plan
# (`docs/private/plans/ci-modernization-plan.md`).
#
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# Ships in CM0 so /goal is verifiable from day one; CM1-CM7 progressively
# flip conditions from FAIL to PASS, CM8 archives the plan.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/ci-modernization-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/ci-modernization-plan.md"
AGENTS_MD="CLAUDE.md"  # symlinks to AGENTS.md
PROOF_DIR="docs/private/plans/proof/ci-modernization"
PROOF_CM7="${PROOF_DIR}/cm7-dependabot-audit.md"

COMPOSITE_ACTION=".github/actions/setup-rust-cached/action.yml"
CODEQL_WF=".github/workflows/codeql.yml"

WORKFLOWS_GLOB=".github/workflows"
ACTIONS_GLOB=".github/actions"

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

# All workflow + composite-action yaml files.
all_yaml_files() {
  local files=()
  while IFS= read -r f; do files+=("${f}"); done < <(find "${WORKFLOWS_GLOB}" -maxdepth 1 -name '*.yml' -type f 2>/dev/null | sort)
  if [ -d "${ACTIONS_GLOB}" ]; then
    while IFS= read -r f; do files+=("${f}"); done < <(find "${ACTIONS_GLOB}" -name 'action.yml' -type f 2>/dev/null | sort)
  fi
  printf '%s\n' "${files[@]}"
}

# -------- conditions -------------------------------------------------------

printf '\033[1mCM verification gate — ci-modernization\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan checked in (active or archived).
step 1 "Plan checked in"
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  pass "Plan exists at ${PLAN_FILE}"
else
  fail "Plan missing" "Neither ${PLAN_ACTIVE} nor ${PLAN_ARCHIVED} exists"
fi

# 2. Composite action exists.
step 2 "Composite action setup-rust-cached exists"
if [ -f "${COMPOSITE_ACTION}" ]; then
  pass "${COMPOSITE_ACTION} exists"
else
  fail "${COMPOSITE_ACTION} missing"
fi

# 3. Zero inline mozilla-actions/sccache-action references in workflows
#    (all sccache wiring flows through the composite).
step 3 "sccache-action wired only via composite"
INLINE_SCCACHE=0
INLINE_FILES=()
while IFS= read -r wf; do
  [ -z "${wf}" ] && continue
  hits=$(grep -cE 'uses:\s*mozilla-actions/sccache-action' "${wf}" 2>/dev/null || true)
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

# 4. Every third-party (non actions/*) `uses:` is SHA-pinned (40-char hex)
#    with a `# vX.Y.Z` version-name comment within 2 lines of the uses: line.
# Allowed first-party org allowlist (tag pins OK):
#   actions/*           (github-owned)
# Allowed local refs (start with ./):
#   uses: ./.github/...
# Everything else must be `uses: org/repo@<40-hex>` with a comment.
step 4 "Third-party actions SHA-pinned with version comment"
BAD_PINS=()
while IFS= read -r wf; do
  [ -z "${wf}" ] && continue
  # Grep numbered lines for `uses: ` references.
  while IFS=: read -r lineno line; do
    # Strip leading whitespace from line content.
    raw="$(printf '%s' "${line}" | sed -E 's/^[[:space:]]+//')"
    # Skip if not a `uses:` directive.
    case "${raw}" in
      uses:*) : ;;
      *) continue ;;
    esac
    # Extract everything after `uses:` (trimmed).
    ref="$(printf '%s' "${raw}" | sed -E 's/^uses:[[:space:]]*//; s/[[:space:]]+#.*$//; s/[[:space:]]+$//')"
    # Local composite action (relative path) — skip.
    case "${ref}" in
      ./*) continue ;;
    esac
    # First-party `actions/*` — tag pin OK at this condition (covered by 6).
    case "${ref}" in
      actions/*) continue ;;
    esac
    # `docker://` or other non-git references — skip (not present in our repo).
    case "${ref}" in
      docker://*) continue ;;
    esac
    # The reference must be `org/repo@<40-hex>`.
    if ! printf '%s' "${ref}" | grep -qE '^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*@[a-f0-9]{40}$'; then
      BAD_PINS+=("${wf}:${lineno} not SHA-pinned (${ref})")
      continue
    fi
    # The line itself, or the next two lines, must include a `# vX.Y.Z` or `# stable` style comment.
    snippet="$(sed -n "${lineno},$((lineno + 2))p" "${wf}")"
    if ! printf '%s' "${snippet}" | grep -qE '#[[:space:]]*(v[0-9]+(\.[0-9]+){0,2}|stable|nightly|beta|main|master)([[:space:]]|$)'; then
      BAD_PINS+=("${wf}:${lineno} missing version comment (${ref})")
    fi
  done < <(grep -nE '^[[:space:]]*uses:[[:space:]]' "${wf}" 2>/dev/null || true)
done < <(find "${WORKFLOWS_GLOB}" -maxdepth 1 -name '*.yml' -type f | sort)
# Also inspect composite-action files for the same discipline.
if [ -d "${ACTIONS_GLOB}" ]; then
  while IFS= read -r af; do
    [ -z "${af}" ] && continue
    while IFS=: read -r lineno line; do
      raw="$(printf '%s' "${line}" | sed -E 's/^[[:space:]]+//')"
      case "${raw}" in
        uses:*) : ;;
        *) continue ;;
      esac
      ref="$(printf '%s' "${raw}" | sed -E 's/^uses:[[:space:]]*//; s/[[:space:]]+#.*$//; s/[[:space:]]+$//')"
      case "${ref}" in
        ./*) continue ;;
        actions/*) continue ;;
        docker://*) continue ;;
      esac
      if ! printf '%s' "${ref}" | grep -qE '^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*@[a-f0-9]{40}$'; then
        BAD_PINS+=("${af}:${lineno} not SHA-pinned (${ref})")
        continue
      fi
      snippet="$(sed -n "${lineno},$((lineno + 2))p" "${af}")"
      if ! printf '%s' "${snippet}" | grep -qE '#[[:space:]]*(v[0-9]+(\.[0-9]+){0,2}|stable|nightly|beta|main|master)([[:space:]]|$)'; then
        BAD_PINS+=("${af}:${lineno} missing version comment (${ref})")
      fi
    done < <(grep -nE '^[[:space:]]*uses:[[:space:]]' "${af}" 2>/dev/null || true)
  done < <(find "${ACTIONS_GLOB}" -name 'action.yml' -type f | sort)
fi
if [ ${#BAD_PINS[@]} -eq 0 ]; then
  pass "Every third-party uses: is SHA-pinned with a version comment"
else
  fail "Third-party uses: not properly pinned" "$(printf '%s; ' "${BAD_PINS[@]}" | head -c 400)"
fi

# 5. Zero `runs-on: ubuntu-latest` references; ARM runners (`ubuntu-24.04-arm`)
#    are explicitly allowed.
step 5 "Runners pinned (no ubuntu-latest)"
UBUNTU_LATEST_HITS=0
UBUNTU_LATEST_FILES=()
while IFS= read -r wf; do
  [ -z "${wf}" ] && continue
  hits=$(grep -cE '^[[:space:]]*runs-on:[[:space:]]*ubuntu-latest([[:space:]]|$)' "${wf}" 2>/dev/null || true)
  hits=${hits// /}
  if [ "${hits}" -gt 0 ] 2>/dev/null; then
    UBUNTU_LATEST_HITS=$((UBUNTU_LATEST_HITS + hits))
    UBUNTU_LATEST_FILES+=("${wf}(${hits})")
  fi
done < <(find "${WORKFLOWS_GLOB}" -maxdepth 1 -name '*.yml' -type f | sort)
if [ "${UBUNTU_LATEST_HITS}" = "0" ]; then
  pass "Zero ubuntu-latest references"
else
  fail "ubuntu-latest still used" "$(printf '%s; ' "${UBUNTU_LATEST_FILES[@]}")"
fi

# 6. No `actions/*` pin uses patch-version granularity (no `@vN.M.P`).
step 6 "actions/* pins are major-only"
OVERPINNED=()
while IFS= read -r wf; do
  [ -z "${wf}" ] && continue
  while IFS=: read -r lineno line; do
    raw="$(printf '%s' "${line}" | sed -E 's/^[[:space:]]+//')"
    case "${raw}" in
      uses:*actions/*) : ;;
      *) continue ;;
    esac
    if printf '%s' "${raw}" | grep -qE 'uses:[[:space:]]*actions/[^@]+@v[0-9]+\.[0-9]+\.[0-9]+'; then
      OVERPINNED+=("${wf}:${lineno} $(printf '%s' "${raw}" | tr -d '\n' | head -c 80)")
    fi
  done < <(grep -nE '^[[:space:]]*uses:[[:space:]]*actions/' "${wf}" 2>/dev/null || true)
done < <(find "${WORKFLOWS_GLOB}" -maxdepth 1 -name '*.yml' -type f | sort)
if [ ${#OVERPINNED[@]} -eq 0 ]; then
  pass "All actions/* pins are major-version (@vN)"
else
  fail "actions/* pins use patch-version granularity" "$(printf '%s; ' "${OVERPINNED[@]}" | head -c 300)"
fi

# 7. At least 4 jobs reference $GITHUB_STEP_SUMMARY.
step 7 "Job summaries emitted from at least 4 jobs"
SUMMARY_HITS=0
while IFS= read -r wf; do
  [ -z "${wf}" ] && continue
  hits=$(grep -cE 'GITHUB_STEP_SUMMARY' "${wf}" 2>/dev/null || true)
  hits=${hits// /}
  SUMMARY_HITS=$((SUMMARY_HITS + hits))
done < <(find "${WORKFLOWS_GLOB}" -maxdepth 1 -name '*.yml' -type f | sort)
if [ "${SUMMARY_HITS}" -ge 4 ] 2>/dev/null; then
  pass "GITHUB_STEP_SUMMARY referenced ${SUMMARY_HITS} time(s) across workflows"
else
  fail "Insufficient GITHUB_STEP_SUMMARY usage" "found ${SUMMARY_HITS}, need >= 4"
fi

# 8. CodeQL workflow exists and references github/codeql-action.
step 8 "CodeQL workflow present"
if [ -f "${CODEQL_WF}" ]; then
  if grep -qE 'github/codeql-action' "${CODEQL_WF}"; then
    pass "${CODEQL_WF} exists and references github/codeql-action"
  else
    fail "${CODEQL_WF} exists but does not reference github/codeql-action"
  fi
else
  fail "${CODEQL_WF} missing"
fi

# 9. CM7 dependabot audit doc exists.
step 9 "Dependabot audit doc present"
if [ -f "${PROOF_CM7}" ]; then
  pass "${PROOF_CM7} exists"
else
  fail "${PROOF_CM7} missing"
fi

# 10. Routing entry exists in CLAUDE.md / AGENTS.md.
step 10 "Routing entry exists in CLAUDE.md"
if [ -f "${AGENTS_MD}" ] || [ -L "${AGENTS_MD}" ]; then
  if grep -q 'ci-modernization-plan' "${AGENTS_MD}"; then
    pass "${AGENTS_MD} references ci-modernization-plan"
  else
    fail "${AGENTS_MD} does not reference ci-modernization-plan"
  fi
else
  fail "${AGENTS_MD} missing"
fi

# 11. Every ledger row marked done.
step 11 "Every ledger row is marked done"
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
