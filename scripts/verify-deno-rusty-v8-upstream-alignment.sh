#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Deno/rusty_v8 upstream alignment
# plan (`docs/plans/deno-rusty-v8-upstream-alignment-plan.md`).
#
# Ships as a failing control gate so DUA can be executed autonomously from day
# one. DUA0 should make the baseline/control-plane gates pass; DUA1-DUA8
# progressively flip the remaining conditions. Closeout requires a summary that
# includes `0 failed`.
#
# Run from anywhere; it cd's to the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/plans/deno-rusty-v8-upstream-alignment-plan.md"
PLAN_ARCHIVED="docs/plans/archive/deno-rusty-v8-upstream-alignment-plan.md"
PROOF_DIR="docs/plans/proof/deno-rusty-v8-upstream-alignment"
FORK_LEDGER="docs/architecture/runtime/deno-fork-bump-ledger.md"
OPERATING_DOC="docs/operating/deno-fork-workflow.md"
NDS_PROOF="docs/plans/proof/node-default-runtime-support-hardening/nds3-official-fixture-promotion.md"

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

contains() {
  local file="$1"
  local pattern="$2"
  grep -Eq "${pattern}" "${file}" 2>/dev/null
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

proof_has_contract() {
  local file="$1"
  [ -f "${file}" ] &&
    grep -q '\*\*Row and status\.\*\*' "${file}" &&
    grep -q '\*\*Input baseline\.\*\*' "${file}" &&
    grep -q '\*\*Disposition table\.\*\*' "${file}" &&
    grep -q '\*\*Implementation evidence\.\*\*' "${file}" &&
    grep -q '\*\*Focused verification\.\*\*' "${file}" &&
    grep -q '\*\*Broad verification\.\*\*' "${file}" &&
    grep -q '\*\*Residual risks\.\*\*' "${file}"
}

required_proofs=(
  "${PROOF_DIR}/dua0-baseline.md"
  "${PROOF_DIR}/dua0-control-plane.md"
  "${PROOF_DIR}/dua1-deno-overlap-audit.md"
  "${PROOF_DIR}/dua2-deno-rebase.md"
  "${PROOF_DIR}/dua3-dirty-work-reevaluation.md"
  "${PROOF_DIR}/dua4-rusty-v8-alignment.md"
  "${PROOF_DIR}/dua5-nimbus-repin.md"
  "${PROOF_DIR}/dua6-node-compat-rebaseline.md"
  "${PROOF_DIR}/dua7-docs-and-ledgers.md"
  "${PROOF_DIR}/dua8-closeout.md"
)

printf '\033[1mDUA verification gate - deno-rusty-v8-upstream-alignment\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

PLAN_FILE="$(plan_file)"

step 1 "Plan exists and closeout status"
if [ -n "${PLAN_FILE}" ] &&
   contains "${PLAN_FILE}" 'Status: `(ready|active|archived)`' &&
   ! grep -qE '^\| DUA[0-9]+ .*\| pending \|$|^\| DUA[0-9]+ .*\| in_progress \|$|^\| DUA[0-9]+ .*\| blocked \|$' "${PLAN_FILE}"; then
  pass "Plan exists with valid status and every DUA ledger row is done at closeout"
else
  fail "Plan missing, status invalid, or not closed" "Expected ${PLAN_ACTIVE} or archived copy with all DUA ledger rows done"
fi

step 2 "Required proof files follow the DUA proof contract"
missing_or_incomplete=()
for proof in "${required_proofs[@]}"; do
  if ! proof_has_contract "${proof}"; then
    missing_or_incomplete+=("${proof}")
  fi
done
if [ "${#missing_or_incomplete[@]}" -eq 0 ]; then
  pass "Every required DUA proof file follows the proof contract"
else
  fail "DUA proof contract incomplete" "$(printf '%s; ' "${missing_or_incomplete[@]}")"
fi

step 3 "DUA0 baseline and control-plane details"
if [ -f "${PROOF_DIR}/dua0-baseline.md" ] &&
   [ -f "${PROOF_DIR}/dua0-control-plane.md" ] &&
   has 'v2\.8\.0-nimbus\.15|1f101bf0032a223463507f500ddd236afebd9fcc' "${PROOF_DIR}/dua0-baseline.md" &&
   has 'denoland/deno@v2\.8\.1' "${PROOF_DIR}/dua0-baseline.md" "${PROOF_DIR}/dua0-control-plane.md" &&
   has 'denoland/rusty_v8@v149\.2\.0' "${PROOF_DIR}/dua0-baseline.md" "${PROOF_DIR}/dua0-control-plane.md" &&
   has 'worktree|branch|PR|pull request' "${PROOF_DIR}/dua0-control-plane.md" &&
   has 'NDS3|node-default-runtime-support-hardening' "${PROOF_DIR}/dua0-control-plane.md"; then
  pass "DUA0 records baseline, upstream targets, worktree/PR, and NDS handoff"
else
  fail "DUA0 baseline/control-plane missing" "Expected fork SHAs, upstream targets, worktree/PR, and NDS handoff"
fi

step 4 "Deno patch disposition audit"
if [ -f "${PROOF_DIR}/dua1-deno-overlap-audit.md" ] &&
   has 'upstream-replaced|upstream-adjacent|nimbus-embedding-specific|still-needed-node-gap|drop-no-longer-needed' "${PROOF_DIR}/dua1-deno-overlap-audit.md" &&
   has 'v2\.8\.0-nimbus\.1|v2\.8\.0-nimbus\.15' "${PROOF_DIR}/dua1-deno-overlap-audit.md" &&
   has 'module\.enableCompileCache' "${PROOF_DIR}/dua1-deno-overlap-audit.md"; then
  pass "DUA1 classifies Deno fork patches and compile-cache disposition"
else
  fail "DUA1 Deno overlap audit incomplete" "Expected complete disposition table through current fork tag"
fi

step 5 "Compile cache exception is justified or absent"
if [ -f "${PROOF_DIR}/dua1-deno-overlap-audit.md" ] &&
   { has 'module\.enableCompileCache.*upstream-replaced|module\.enableCompileCache.*drop-no-longer-needed' "${PROOF_DIR}/dua1-deno-overlap-audit.md" ||
     has 'module\.enableCompileCache.*product-specific.*permission.*test' "${PROOF_DIR}/dua1-deno-overlap-audit.md"; }; then
  pass "module.enableCompileCache is dropped or justified with product proof"
else
  fail "module.enableCompileCache disposition missing" "Expected upstream-replaced/drop or product-specific exception"
fi

step 6 "Deno candidate is based on upstream v2.8.1"
if [ -f "${PROOF_DIR}/dua2-deno-rebase.md" ] &&
   has 'v2\.8\.1|denoland/deno@v2\.8\.1' "${PROOF_DIR}/dua2-deno-rebase.md"; then
  pass "DUA2 records Deno v2.8.1 rebase"
else
  fail "Deno v2.8.1 rebase proof missing" "Expected Deno candidate based on upstream v2.8.1"
fi

step 7 "Upstream-replaced patches are absent after rebase"
if [ -f "${PROOF_DIR}/dua2-deno-rebase.md" ] &&
   has 'No upstream-replaced.*remain|upstream-replaced.*absent' "${PROOF_DIR}/dua2-deno-rebase.md"; then
  pass "DUA2 proves upstream-replaced patches are absent"
else
  fail "Upstream-replaced patch cleanup unproven" "Expected proof that dropped patches are absent from the diff"
fi

step 8 "Replay patches have owner, evidence, and triggers"
if [ -f "${PROOF_DIR}/dua2-deno-rebase.md" ] &&
   has 'owner repo|source location|focused verification|removal trigger|upstream trigger' "${PROOF_DIR}/dua2-deno-rebase.md"; then
  pass "Replayed Deno patches carry owner/evidence/removal triggers"
else
  fail "Replayed Deno patch evidence incomplete" "Expected source locations, owner repo, tests, and triggers"
fi

step 9 "Dirty Deno work reevaluated"
if [ -f "${PROOF_DIR}/dua3-dirty-work-reevaluation.md" ] &&
   has 'CommonJS global path|node:v8|crypto random|cipher|internal_binding' "${PROOF_DIR}/dua3-dirty-work-reevaluation.md" &&
   has 'dropped|upstream-replaced|committed|disposition' "${PROOF_DIR}/dua3-dirty-work-reevaluation.md"; then
  pass "DUA3 reevaluates current dirty/fresh Node compatibility work"
else
  fail "DUA3 dirty-work reevaluation missing" "Expected loader, V8, crypto, and internal binding disposition proof"
fi

step 10 "rusty_v8 direct bump safety decision"
if [ -f "${PROOF_DIR}/dua4-rusty-v8-alignment.md" ] &&
   has 'Locker|UnenteredIsolate' "${PROOF_DIR}/dua4-rusty-v8-alignment.md" &&
   has 'v149\.2\.0' "${PROOF_DIR}/dua4-rusty-v8-alignment.md" &&
   has 'direct bump.*reject|rebase|hold' "${PROOF_DIR}/dua4-rusty-v8-alignment.md"; then
  pass "DUA4 records rusty_v8 v149.2.0 decision without dropping locker safety"
else
  fail "rusty_v8 alignment decision missing" "Expected direct-bump rejection/rebase/hold proof"
fi

step 11 "rusty_v8 rebase preserves locker APIs or records hold"
if [ -f "${PROOF_DIR}/dua4-rusty-v8-alignment.md" ] &&
   { has 'v149\.2\.0-nimbus\.1.*Locker.*UnenteredIsolate' "${PROOF_DIR}/dua4-rusty-v8-alignment.md" ||
     has 'hold.*v149\.0\.0-nimbus\.1.*Locker.*UnenteredIsolate' "${PROOF_DIR}/dua4-rusty-v8-alignment.md"; }; then
  pass "DUA4 proves rebase preservation or hold rationale"
else
  fail "rusty_v8 preservation/hold proof missing" "Expected locker API preservation or explicit hold rationale"
fi

step 12 "Hold decision compares consumed upstream behavior"
if [ -f "${PROOF_DIR}/dua4-rusty-v8-alignment.md" ] &&
   has 'annex teardown|consumed runtime benefit|v149\.2\.0' "${PROOF_DIR}/dua4-rusty-v8-alignment.md"; then
  pass "DUA4 compares consumed rusty_v8 upstream behavior"
else
  fail "rusty_v8 consumed-benefit comparison missing" "Expected v149.2.0 benefit/hold analysis"
fi

step 13 "Nimbus pins immutable published fork tags"
if grep -Eq 'tag = "v2\.8\.1-nimbus\.[0-9]+"' Cargo.toml 2>/dev/null &&
   grep -Eq 'git\+https://github.com/nimbus/deno\?tag=v2\.8\.1-nimbus\.[0-9]+#[a-f0-9]{40}' Cargo.lock 2>/dev/null &&
   ! grep -Eq 'path = "|/private/tmp|/Users/jack/src/github.com/nimbus/deno' Cargo.toml Cargo.lock 2>/dev/null; then
  pass "Nimbus Cargo pins use immutable published upstream-aligned Deno tag"
else
  fail "Nimbus repin incomplete" "Expected v2.8.1-nimbus.* git tag/SHA and no local path overrides"
fi

step 14 "Fork provenance and upstream policy pass"
if [ -f "${PROOF_DIR}/dua5-nimbus-repin.md" ] &&
   has 'verify-deno-fork-provenance\.sh.*0 failed|verify-deno-fork-provenance\.sh.*passed' "${PROOF_DIR}/dua5-nimbus-repin.md" &&
   has 'verify-deno-fork-upstream-policy\.sh.*0 failed|verify-deno-fork-upstream-policy\.sh.*passed' "${PROOF_DIR}/dua5-nimbus-repin.md"; then
  pass "DUA5 records green fork provenance and upstream policy verifiers"
else
  fail "Fork provenance/upstream-policy proof missing" "Expected DUA5 proof with green verifier output"
fi

step 15 "Focused Node compatibility tests pass for changed behavior"
if [ -f "${PROOF_DIR}/dua6-node-compat-rebaseline.md" ] &&
   has 'focused.*passed|Focused verification.*passed' "${PROOF_DIR}/dua6-node-compat-rebaseline.md" &&
   has 'CommonJS|crypto|node:v8|loader|async_hooks|networking|fs|stream' "${PROOF_DIR}/dua6-node-compat-rebaseline.md"; then
  pass "DUA6 records focused changed-behavior tests"
else
  fail "Focused compatibility proof missing" "Expected focused tests for every changed behavior"
fi

step 16 "Broad Node compatibility reruns compare before/after"
if [ -f "${PROOF_DIR}/dua6-node-compat-rebaseline.md" ] &&
   has 'before.*after|pre-DUA.*post-DUA|Broad.*rerun' "${PROOF_DIR}/dua6-node-compat-rebaseline.md"; then
  pass "DUA6 records broad before/after compatibility evidence"
else
  fail "Broad compatibility rebaseline missing" "Expected pre-DUA/post-DUA broad counts"
fi

step 17 "Promotion requires broad reruns"
if [ -f "${PROOF_DIR}/dua6-node-compat-rebaseline.md" ] &&
   has 'Newly green.*broad|not promoted from focused tests alone' "${PROOF_DIR}/dua6-node-compat-rebaseline.md"; then
  pass "DUA6 proves newly green fixtures are not focused-only promotions"
else
  fail "Focused-only promotion guard missing" "Expected proof that promotions waited for broad reruns"
fi

step 18 "Remaining failures have owner and follow-up path"
if [ -f "${PROOF_DIR}/dua6-node-compat-rebaseline.md" ] &&
   has 'owner repo|Nimbus runtime|Deno fork|rusty_v8|upstream/platform|non-isolate' "${PROOF_DIR}/dua6-node-compat-rebaseline.md" &&
   has 'follow-up|trigger|blocker' "${PROOF_DIR}/dua6-node-compat-rebaseline.md"; then
  pass "Remaining failures are owned and routed"
else
  fail "Remaining failure ownership missing" "Expected owner repo and follow-up route"
fi

step 19 "Generated compatibility evidence updated when counts move"
if [ -f "${PROOF_DIR}/dua7-docs-and-ledgers.md" ] &&
   has 'dashboard|status-summary|generated Node compatibility' "${PROOF_DIR}/dua7-docs-and-ledgers.md"; then
  pass "DUA7 records generated compatibility evidence handling"
else
  fail "Generated evidence update proof missing" "Expected dashboard/summary update or no-change proof"
fi

step 20 "Fork bump ledger records upstream-aligned tag and dispositions"
if [ -f "${FORK_LEDGER}" ] &&
   has 'v2\.8\.1-nimbus\.[0-9]|v149\.2\.0-nimbus\.[0-9]|v149\.0\.0-nimbus\.1' "${FORK_LEDGER}" &&
   has 'upstream-replaced|upstream-adjacent|nimbus-embedding-specific|still-needed-node-gap|drop-no-longer-needed|Temporary carry|Upstream Deno-family' "${FORK_LEDGER}" &&
   has 'removal trigger|upstream trigger|Release Proof Checklist' "${FORK_LEDGER}"; then
  pass "Fork bump ledger records tags, dispositions, triggers, and verification contract"
else
  fail "Fork bump ledger incomplete" "Expected upstream-aligned tag/SHA, dispositions, triggers, and verification"
fi

step 21 "Docs links and operating workflow are consistent"
if [ -f "${OPERATING_DOC}" ] &&
   [ -n "${PLAN_FILE}" ] &&
   has 'verify-deno-fork-provenance\.sh|verify-deno-fork-upstream-policy\.sh' "${OPERATING_DOC}" &&
   has 'deno-rusty-v8-upstream-alignment-plan\.md' docs/plans/README.md "${PLAN_FILE}"; then
  pass "Operating docs and plan index reference the DUA workflow"
else
  fail "DUA operating docs/index links missing" "Expected docs/plans/README and fork workflow to reference required gates"
fi

step 22 "Closeout validation commands pass"
if [ -f "${PROOF_DIR}/dua8-closeout.md" ] &&
   has 'cargo fmt --all --check.*pass|cargo fmt --all --check.*0 failed' "${PROOF_DIR}/dua8-closeout.md" &&
   has 'docs:validate-refs:strict.*pass|strict docs refs.*pass' "${PROOF_DIR}/dua8-closeout.md" &&
   has 'git diff --check.*pass' "${PROOF_DIR}/dua8-closeout.md"; then
  pass "DUA8 records green local validation commands"
else
  fail "Closeout validation proof missing" "Expected fmt, strict docs refs, and git diff --check proof"
fi

step 23 "Closeout records PR status and NDS handoff"
if [ -f "${PROOF_DIR}/dua8-closeout.md" ] &&
   has '0 failed' "${PROOF_DIR}/dua8-closeout.md" &&
   has 'PR|pull request|checks' "${PROOF_DIR}/dua8-closeout.md" &&
   has 'NDS|node-default-runtime-support-hardening|handoff' "${PROOF_DIR}/dua8-closeout.md"; then
  pass "DUA8 records green verifier, PR status, and handoff back to NDS"
else
  fail "DUA closeout/handoff proof missing" "Expected local verifier, PR checks, and NDS handoff point"
fi

printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
