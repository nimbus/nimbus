#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Node Default Runtime Support
# Hardening plan (`docs/private/plans/node-default-runtime-support-hardening-plan.md`).
#
# Ships in NDS0 as a failing control gate. Conditions already satisfied by the
# scaffold pass; every condition tied to later NDS rows fails until that row
# lands. Closeout requires all conditions to pass with a summary containing
# `0 failed`.
#
# Run from anywhere; it cd's to the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/node-default-runtime-support-hardening-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/node-default-runtime-support-hardening-plan.md"
PROOF_DIR="docs/private/plans/proof/node-default-runtime-support-hardening"
BASELINE_PROOF="${PROOF_DIR}/nds0-baseline.md"
CONTROL_PROOF="${PROOF_DIR}/nds0-control-plane.md"
POSTURE_JSON="docs/private/staging/architecture/runtime/node-default-support-posture.json"
POSTURE_MD="docs/private/staging/architecture/runtime/node-default-support-posture.md"
CANARY_REGISTRY="tests/runtime/node/canary-registry.json"
STATUS_SUMMARY="docs/private/staging/architecture/runtime/node-compat-evidence/latest/status-summary.md"

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
    FAIL_DETAIL+=("$1 - $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
}

has() {
  grep -RqE "$1" "${@:2}" 2>/dev/null
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

proof_has_template() {
  local file="$1"
  [ -f "${file}" ] &&
    grep -q '^## Row And Status' "${file}" &&
    grep -q '^## Broad Pre-Run' "${file}" &&
    grep -q '^## Failure Grouping' "${file}" &&
    grep -q '^## Focused Work' "${file}" &&
    grep -q '^## Broad Final Rerun' "${file}" &&
    grep -q '^## Evidence Links' "${file}" &&
    grep -q '^## Residual Risks' "${file}"
}

required_proofs=(
  "${PROOF_DIR}/nds0-baseline.md"
  "${PROOF_DIR}/nds0-control-plane.md"
  "${PROOF_DIR}/nds1-posture-model-and-feasibility.md"
  "${PROOF_DIR}/nds2-foundation-slices.md"
  "${PROOF_DIR}/nds3-official-fixture-promotion.md"
  "${PROOF_DIR}/nds4-node26-current-evidence.md"
  "${PROOF_DIR}/nds5-package-canaries.md"
  "${PROOF_DIR}/nds6-convex-app-suites.md"
  "${PROOF_DIR}/nds7-permissions-and-shim-audit.md"
  "${PROOF_DIR}/nds8-generated-docs.md"
  "${PROOF_DIR}/nds9-ci-and-nightly-gates.md"
  "${PROOF_DIR}/nds10-closeout.md"
)

printf '\033[1mNDS verification gate - node-default-runtime-support-hardening\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

PLAN_FILE="$(plan_file)"

step 1 "Plan closeout status"
if [ -n "${PLAN_FILE}" ] && \
   ! grep -qE '^\| NDS[0-9]+ .*\| (pending|in_progress|blocked) \|$' "${PLAN_FILE}"; then
  pass "Plan exists and every ledger row is done"
else
  fail "Plan not closed" "Expected plan with all NDS ledger rows done; plan=${PLAN_FILE:-missing}"
fi

step 2 "Baseline proof records low Node24/Node26 posture"
if [ -f "${BASELINE_PROOF}" ] &&
   grep -q 'Node24 `1002 / 5198`' "${BASELINE_PROOF}" &&
   grep -q 'Node22 `1000 / 4748`' "${BASELINE_PROOF}" &&
   grep -q 'Node26 `0 / 5578`' "${BASELINE_PROOF}" &&
   grep -q 'package/framework canary claims.*37' "${BASELINE_PROOF}" &&
   grep -q 'registry split `32` Application / `5` Tooling' "${BASELINE_PROOF}"; then
  pass "NDS0 baseline proof captures fixture and package baselines"
else
  fail "NDS0 baseline proof incomplete" "Expected ${BASELINE_PROOF} with lane and package baselines"
fi

step 3 "Control-plane proof and Active Execution Pointer"
if [ -f "${CONTROL_PROOF}" ] &&
   grep -q 'codex/node-default-runtime-support-hardening' "${CONTROL_PROOF}" &&
   grep -q '/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening' "${CONTROL_PROOF}" &&
   grep -q 'Deno fork publish/repin protocol' "${CONTROL_PROOF}" &&
   grep -q 'Resume protocol' "${CONTROL_PROOF}" &&
   [ -n "${PLAN_FILE}" ] &&
   grep -q 'Active worktree | `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`' "${PLAN_FILE}" &&
   grep -q 'Active branch | `codex/node-default-runtime-support-hardening`' "${PLAN_FILE}" &&
   grep -q 'Current row | `NDS0`' "${PLAN_FILE}"; then
  pass "Control-plane proof and Active Execution Pointer are populated"
else
  fail "Control-plane proof or Active Execution Pointer incomplete" "Draft PR/main-visible pointer may still be pending"
fi

step 4 "Default-support posture artifacts"
if [ -f "${POSTURE_JSON}" ] && [ -f "${POSTURE_MD}" ]; then
  pass "Default-support posture artifacts exist"
else
  fail "Default-support posture artifacts missing" "Expected ${POSTURE_JSON} and ${POSTURE_MD}"
fi

step 5 "NDS1 feasibility proof"
if [ -f "${PROOF_DIR}/nds1-posture-model-and-feasibility.md" ] &&
   grep -q '2000' "${PROOF_DIR}/nds1-posture-model-and-feasibility.md" &&
   grep -q 'reachable' "${PROOF_DIR}/nds1-posture-model-and-feasibility.md"; then
  pass "NDS1 feasibility proof records 2000-pass ceiling"
else
  fail "NDS1 feasibility proof missing" "Expected reachable ceiling or blocked path"
fi

step 6 "Denominator schema is strict"
if [ -f "${POSTURE_JSON}" ] &&
   grep -q 'v8_isolate_required' "${POSTURE_JSON}" &&
   grep -q 'diagnostic_only_non_isolate' "${POSTURE_JSON}" &&
   grep -q 'test_harness_only' "${POSTURE_JSON}" &&
   grep -q 'upstream_or_platform_boundary' "${POSTURE_JSON}"; then
  pass "Posture schema names required denominator vocabulary"
else
  fail "Posture denominator schema incomplete" "Expected schema-controlled denominator vocabulary"
fi

step 7 "Docs and posture agree on required support"
if [ -f "${POSTURE_JSON}" ] && [ -f "${POSTURE_MD}" ] &&
   has 'v8_isolate_required' "${POSTURE_JSON}" "${POSTURE_MD}"; then
  pass "Docs and posture contain V8-isolate-required support class"
else
  fail "Docs/posture cross-check missing" "Expected generated docs to expose required denominator"
fi

step 8 "Node24 unpromoted surface eliminated"
if [ -f "${STATUS_SUMMARY}" ] && ! grep -q '`node24`.*Requires Unpromoted Node Surface' "${STATUS_SUMMARY}"; then
  pass "Node24 has no unpromoted surface entries"
else
  fail "Node24 still has Requires Unpromoted Node Surface" "NDS1/NDS3 must eliminate or reclassify them"
fi

step 9 "Node22/Node24 V8-isolate-required green"
if [ -f "${POSTURE_JSON}" ] &&
   grep -q '"node22".*"v8_isolate_required".*"pass_rate".*100' "${POSTURE_JSON}" &&
   grep -q '"node24".*"v8_isolate_required".*"pass_rate".*100' "${POSTURE_JSON}"; then
  pass "Node22 and Node24 V8-isolate-required fixtures are 100%"
else
  fail "V8-isolate-required fixtures not proven green" "Expected generated posture metrics"
fi

step 10 "Node22 full-corpus parity"
if [ -f "${PROOF_DIR}/nds3-official-fixture-promotion.md" ] &&
   grep -q 'within 5 percentage points' "${PROOF_DIR}/nds3-official-fixture-promotion.md"; then
  pass "Node22 parity proof exists"
else
  fail "Node22 parity proof missing" "Expected within-5pp proof or upstream delta"
fi

step 11 "Node26 real Current evidence"
if [ -f "${PROOF_DIR}/nds4-node26-current-evidence.md" ] &&
   grep -q 'Node26 official fixture pass count.*1000' "${PROOF_DIR}/nds4-node26-current-evidence.md"; then
  pass "Node26 Current evidence reaches fixture threshold"
else
  fail "Node26 Current evidence incomplete" "Expected >=1000 passes and shared required surface proof"
fi

step 12 "Canonical foundation slices green"
if [ -f "${PROOF_DIR}/nds2-foundation-slices.md" ] &&
   grep -q 'assert-and-buffer-foundation' "${PROOF_DIR}/nds2-foundation-slices.md" &&
   grep -q 'process-foundation' "${PROOF_DIR}/nds2-foundation-slices.md" &&
   grep -q 'os-tty-readline-foundation' "${PROOF_DIR}/nds2-foundation-slices.md" &&
   grep -q 'dns-net-foundation' "${PROOF_DIR}/nds2-foundation-slices.md" &&
   grep -q 'module-and-async-foundation' "${PROOF_DIR}/nds2-foundation-slices.md" &&
   grep -q 'final broad rerun.*green' "${PROOF_DIR}/nds2-foundation-slices.md"; then
  pass "All five foundation slices are green on Node22/Node24"
else
  fail "Foundation slice proof incomplete" "Expected all five canonical slices and final green rerun"
fi

step 13 "NDS2 inherited NCG fixture fidelity"
if [ -f "${PROOF_DIR}/nds2-foundation-slices.md" ] &&
   grep -q 'test-process-features.js' "${PROOF_DIR}/nds2-foundation-slices.md" &&
   grep -q 'test-module-builtin.js' "${PROOF_DIR}/nds2-foundation-slices.md" &&
   grep -q 'test-module-version.js' "${PROOF_DIR}/nds2-foundation-slices.md" &&
   has 'bootstrap-shim|runtime-op|fork-bump|explicit-divergence' "${PROOF_DIR}/nds2-foundation-slices.md"; then
  pass "NDS2 proof names process and loader-context fixture details"
else
  fail "NDS2 fixture fidelity missing" "Expected process fixture, 10 module fixtures, 4 failures, and classification taxonomy"
fi

step 14 "Node24 full-corpus threshold"
if [ -f "${POSTURE_JSON}" ] && grep -q '"node24".*"full_corpus".*"passed".*2000' "${POSTURE_JSON}"; then
  pass "Node24 full-corpus official pass count is at least 2000"
else
  fail "Node24 full-corpus threshold unmet" "Expected generated metric >= 2000"
fi

step 15 "Package registry category schema"
if [ -f "${CANARY_REGISTRY}" ] &&
   grep -q '"compat_category"' "${CANARY_REGISTRY}" &&
   grep -q '"compat_family"' "${CANARY_REGISTRY}" &&
   grep -q '"canary_surfaces"' "${CANARY_REGISTRY}"; then
  pass "Package registry has category schema fields"
else
  fail "Package registry category schema missing" "Expected compat_family, compat_category, canary_surfaces"
fi

step 16 "Positive Application package claim breadth"
if [ -f "${PROOF_DIR}/nds5-package-canaries.md" ] &&
   grep -q '50 positive Application claims' "${PROOF_DIR}/nds5-package-canaries.md" &&
   grep -q '12 distinct `compat_category`' "${PROOF_DIR}/nds5-package-canaries.md"; then
  pass "Application package breadth proof exists"
else
  fail "Application package breadth incomplete" "Expected >=50 claims across >=12 categories"
fi

step 17 "Required canary gaps are zero"
if [ -f "docs/private/staging/architecture/runtime/node-compat-evidence/latest/dashboard-summary.md" ] &&
   grep -q 'required canary gaps: `0`' docs/private/staging/runtimes/nodejs/compatibility.md 2>/dev/null; then
  pass "Required Application canary gaps are zero"
else
  fail "Required canary gap proof missing" "Expected generated docs/dashboard to show 0"
fi

step 18 "Convex real app suites"
if [ -f "${PROOF_DIR}/nds6-convex-app-suites.md" ] &&
   grep -q '5 app suites' "${PROOF_DIR}/nds6-convex-app-suites.md"; then
  pass "Convex app suite proof exists"
else
  fail "Convex app suite proof missing" "Expected >=5 real app suites on Node22/Node24"
fi

step 19 "Non-isolate diagnostics excluded from support"
if [ -f "${PROOF_DIR}/nds7-permissions-and-shim-audit.md" ] &&
   grep -q 'diagnostic' "${PROOF_DIR}/nds7-permissions-and-shim-audit.md" &&
   grep -q 'excluded from positive support' "${PROOF_DIR}/nds7-permissions-and-shim-audit.md"; then
  pass "Non-isolate diagnostics are excluded from support counts"
else
  fail "Non-isolate diagnostic proof missing" "Expected diagnostic evidence exclusion proof"
fi

step 20 "Generated public docs match evidence"
if [ -f "${PROOF_DIR}/nds8-generated-docs.md" ] &&
   grep -q 'generated docs match' "${PROOF_DIR}/nds8-generated-docs.md"; then
  pass "Generated public docs match evidence"
else
  fail "Generated docs proof missing" "Expected NDS8 generated docs proof"
fi

step 21 "Package reference is per-version"
if [ -f docs/private/staging/runtimes/nodejs/reference/packages.md ] &&
   grep -q 'Node22' docs/private/staging/runtimes/nodejs/reference/packages.md &&
   grep -q 'Node24' docs/private/staging/runtimes/nodejs/reference/packages.md &&
   grep -q 'Node26' docs/private/staging/runtimes/nodejs/reference/packages.md; then
  pass "Package reference contains per-version support"
else
  fail "Package reference lacks per-version support" "Expected Node22/Node24/Node26 package evidence"
fi

step 22 "API reference is per-version and boundary-aware"
if [ -f docs/private/staging/runtimes/nodejs/reference/node-apis.md ] &&
   grep -q 'Node22' docs/private/staging/runtimes/nodejs/reference/node-apis.md &&
   grep -q 'Service/microVM required' docs/private/staging/runtimes/nodejs/reference/node-apis.md; then
  pass "API reference contains per-version support and non-isolate boundaries"
else
  fail "API reference incomplete" "Expected per-version support and non-isolate boundaries"
fi

step 23 "Shim/emulation inventory"
if [ -f docs/private/staging/architecture/runtime/node-isolate-shim-inventory.json ] ||
   [ -f docs/private/staging/architecture/runtime/node-isolate-shim-inventory.md ]; then
  pass "Shim/emulation inventory exists"
else
  fail "Shim/emulation inventory missing" "Expected inventory covering nimbus/nimbus and nimbus/deno"
fi

step 24 "User-facing docs disclose capability classes"
if has 'native|shimmed|emulated|test-harness-only|diagnostic|unsupported' docs/private/staging/runtimes/nodejs docs/private/staging/architecture/runtime 2>/dev/null; then
  pass "User-facing docs disclose capability classes"
else
  fail "Capability class docs missing" "Expected native/shimmed/emulated/diagnostic/unsupported disclosure"
fi

step 25 "Release-train and latest-suite drift"
if [ -f docs/private/staging/architecture/runtime/node-lts-compat/node-release-train.json ] &&
   grep -q '"drift_detected": false' docs/private/staging/architecture/runtime/node-lts-compat/node-release-train.json; then
  pass "Release-train drift check is clean"
else
  fail "Release-train drift proof missing or dirty" "Expected drift_detected=false"
fi

step 26 "PR CI includes default-support gate"
if grep -Rq 'verify-node-default-runtime-support-hardening' .github/workflows 2>/dev/null; then
  pass "PR CI includes NDS verifier"
else
  fail "PR CI missing NDS verifier" "Expected workflow wiring"
fi

step 27 "Nightly includes broad fixture/package/Node26 lanes"
if [ -f .github/workflows/node-compat-nightly.yml ] &&
   grep -q 'node26' .github/workflows/node-compat-nightly.yml &&
   grep -q 'fixture' .github/workflows/node-compat-nightly.yml; then
  pass "Nightly includes broad Node compatibility lanes"
else
  fail "Nightly broad lanes missing" "Expected fixture, package, and Node26 Current lanes"
fi

step 28 "Local validation commands recorded"
if [ -f "${PROOF_DIR}/nds10-closeout.md" ] &&
   grep -q 'cargo fmt --all --check' "${PROOF_DIR}/nds10-closeout.md" &&
   grep -q 'npm run docs:validate-refs:strict' "${PROOF_DIR}/nds10-closeout.md" &&
   grep -q 'git diff --check' "${PROOF_DIR}/nds10-closeout.md"; then
  pass "Closeout records local validation commands"
else
  fail "Closeout validation proof missing" "Expected fmt, docs refs, and git diff checks"
fi

step 29 "Required row proof files follow template"
missing_templates=()
for proof in "${required_proofs[@]}"; do
  if ! proof_has_template "${proof}"; then
    missing_templates+=("${proof}")
  fi
done
if [ "${#missing_templates[@]}" -eq 0 ]; then
  pass "Every required row proof follows template"
else
  fail "Proof template coverage incomplete" "$(printf '%s; ' "${missing_templates[@]}")"
fi

step 30 "Every row proof records wide-then-focused loop"
loop_missing=()
for proof in "${required_proofs[@]}"; do
  if [ -f "${proof}" ] &&
     grep -q 'Broad Pre-Run' "${proof}" &&
     grep -q 'Failure Grouping' "${proof}" &&
     grep -q 'Focused Work' "${proof}" &&
     grep -q 'Broad Final Rerun' "${proof}"; then
    :
  else
    loop_missing+=("${proof}")
  fi
done
if [ "${#loop_missing[@]}" -eq 0 ]; then
  pass "Every row proof records wide-then-focused loop"
else
  fail "Wide-then-focused proof coverage incomplete" "$(printf '%s; ' "${loop_missing[@]}")"
fi

step 31 "Diagnostic canaries and fake-success stubs rejected"
if [ -f "${PROOF_DIR}/nds7-permissions-and-shim-audit.md" ] &&
   grep -q 'fake-success' "${PROOF_DIR}/nds7-permissions-and-shim-audit.md" &&
   grep -q 'diagnostic canaries counted as positive support.*rejected' "${PROOF_DIR}/nds7-permissions-and-shim-audit.md"; then
  pass "Diagnostic/fake-success rejection proof exists"
else
  fail "Diagnostic/fake-success rejection proof missing" "Expected registry/test-backed rejection proof"
fi

step 32 "Stale hand-written support numbers rejected"
if [ -f "${PROOF_DIR}/nds8-generated-docs.md" ] &&
   grep -q 'stale hand-written support numbers' "${PROOF_DIR}/nds8-generated-docs.md"; then
  pass "Generated docs reject stale support numbers"
else
  fail "Stale support-number guard missing" "Expected NDS8 stale-number proof"
fi

step 33 "Closeout proof records green local and remote checks"
if [ -f "${PROOF_DIR}/nds10-closeout.md" ] &&
   grep -q 'green local verifier' "${PROOF_DIR}/nds10-closeout.md" &&
   grep -q 'green draft PR checks' "${PROOF_DIR}/nds10-closeout.md"; then
  pass "Closeout proof records local and remote green checks"
else
  fail "Closeout proof incomplete" "Expected local verifier, PR checks, and approval path"
fi

step 34 "Blocked rows cannot be archived as complete"
if [ -n "${PLAN_FILE}" ] && grep -qE '^\| NDS[0-9]+ .*\| blocked \|$' "${PLAN_FILE}"; then
  if grep -Rq 'owner repository' "${PROOF_DIR}" 2>/dev/null &&
     grep -Rq 'follow-up' "${PROOF_DIR}" 2>/dev/null &&
     grep -Rq 'unsatisfied verifier' "${PROOF_DIR}" 2>/dev/null; then
    fail "Plan is blocked, not complete" "Blocked state is documented; archive/closeout must remain rejected"
  else
    fail "Blocked row lacks required blocker record" "Expected exact blockers, owner repo, follow-up plan, pointer update, unsatisfied gates"
  fi
else
  pass "No blocked ledger rows present"
fi

printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi

exit 0
