#!/usr/bin/env bash
# Verifies the Deno-family fork upstream-first operating policy is documented.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

OPERATING_DOC="docs/private/operating/deno-fork-workflow.md"
LEDGER_DOC="docs/private/architecture/runtime/deno-fork-bump-ledger.md"

EXPECTED_DENO_TAG="v2.8.2-nimbus.1"
EXPECTED_DENO_SHA="d7befe8b02b4334100e879ed5b8d46b1a9406313"
EXPECTED_V8_TAG="v149.2.0-nimbus.1"
EXPECTED_V8_SHA="ce6663111a3ff8fde06bc04ba19bbbced60dbc8d"

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

require_contains() {
  local file="$1"
  local pattern="$2"
  local label="$3"

  if contains "${file}" "${pattern}"; then
    pass "${label}"
  else
    fail "${label}" "Expected ${file} to match: ${pattern}"
  fi
}

printf '\033[1mDeno fork upstream policy verifier\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

step 1 "Operating workflow exists and names canonical fork sources"
if [ -f "${OPERATING_DOC}" ] && [ -f "${LEDGER_DOC}" ]; then
  pass "Operating workflow and bump ledger files exist"
else
  fail "Missing operating workflow or ledger" "Expected ${OPERATING_DOC} and ${LEDGER_DOC}"
fi
require_contains "${OPERATING_DOC}" '/Users/jack/src/github.com/nimbus/deno' "Workflow names canonical local nimbus/deno checkout"
require_contains "${OPERATING_DOC}" '/Users/jack/src/github.com/nimbus/rusty_v8' "Workflow names canonical local nimbus/rusty_v8 checkout"
require_contains "${OPERATING_DOC}" 'Do not use `/private/tmp` checkouts' "Workflow forbids temporary checkouts as progress state"

step 2 "Workflow requires publish, tag, repin, and verification proof"
require_contains "${OPERATING_DOC}" 'Unpin Nimbus' "Workflow includes the unpin-to-local-fork step"
require_contains "${OPERATING_DOC}" 'Commit, tag, and push' "Workflow includes commit/tag/push before repin"
require_contains "${OPERATING_DOC}" 'Repin Nimbus' "Workflow includes repinning Cargo.toml and Cargo.lock"
require_contains "${OPERATING_DOC}" 'scripts/verify-deno-fork-provenance.sh' "Workflow requires Deno fork provenance verifier"
require_contains "${OPERATING_DOC}" 'scripts/verify-deno-fork-upstream-policy.sh' "Workflow requires this upstream-policy verifier"

step 3 "Patch disposition taxonomy is explicit"
require_contains "${OPERATING_DOC}" 'Upstream Deno-family' "Workflow defines upstream Deno-family disposition"
require_contains "${OPERATING_DOC}" 'Nimbus-only host integration' "Workflow defines Nimbus-only host-integration disposition"
require_contains "${OPERATING_DOC}" 'Temporary carry' "Workflow defines temporary-carry disposition"
require_contains "${OPERATING_DOC}" 'Removal or upstream trigger' "Workflow requires removal or upstream trigger"

step 4 "Current fork pins and carried patch dispositions are ledgered"
require_contains "${LEDGER_DOC}" "${EXPECTED_DENO_TAG}" "Ledger records expected nimbus/deno tag"
require_contains "${LEDGER_DOC}" "${EXPECTED_DENO_SHA}" "Ledger records expected nimbus/deno commit SHA"
require_contains "${LEDGER_DOC}" "${EXPECTED_V8_TAG}" "Ledger records expected nimbus/rusty_v8 tag"
require_contains "${LEDGER_DOC}" "${EXPECTED_V8_SHA}" "Ledger records expected nimbus/rusty_v8 commit SHA"
require_contains "${LEDGER_DOC}" '693efb7d872f2a7097e8b85e8f39eb0fb0c80d68' "Ledger records first current nimbus/deno carried patch"
require_contains "${LEDGER_DOC}" 'd7befe8b02b4334100e879ed5b8d46b1a9406313' "Ledger records latest current nimbus/deno carried patch"
require_contains "${LEDGER_DOC}" 'bf8dbbb4211152ce71d8f6777bb5d4980f178f41' "Ledger records first current nimbus/rusty_v8 carried patch"
require_contains "${LEDGER_DOC}" 'ce6663111a3ff8fde06bc04ba19bbbced60dbc8d' "Ledger records latest current nimbus/rusty_v8 carried patch"
require_contains "${LEDGER_DOC}" 'Upstream Deno-family' "Ledger uses upstream Deno-family disposition"
require_contains "${LEDGER_DOC}" 'Nimbus-only host integration' "Ledger uses Nimbus-only host-integration disposition"
require_contains "${LEDGER_DOC}" 'Temporary carry' "Ledger uses temporary-carry disposition"

step 5 "Release proof checklist is present"
require_contains "${LEDGER_DOC}" 'Release Proof Checklist' "Ledger includes release proof checklist"
require_contains "${LEDGER_DOC}" 'repinned to published tags' "Ledger requires published-tag repin proof"
require_contains "${LEDGER_DOC}" 'generated Node evidence' "Ledger ties fork bumps to generated Node evidence when claims move"

printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
