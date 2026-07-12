#!/usr/bin/env bash
# Verifies the tracked, upstream-first Deno/rusty_v8 fork policy.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit

# shellcheck source=scripts/deno-fork-pins.sh
source "${REPO_ROOT}/scripts/deno-fork-pins.sh"
deno_fork_load_consumed_pins

POLICY_DOC="scripts/deno-fork-policy.md"

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
  grep -Eq -- "${pattern}" "${file}" 2>/dev/null
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

step 1 "Tracked policy exists and names canonical fork sources"
if [ -f "${POLICY_DOC}" ]; then
  pass "Tracked fork policy exists"
else
  fail "Missing tracked fork policy" "Expected ${POLICY_DOC}"
fi
require_contains "${POLICY_DOC}" '/Users/jack/src/github.com/nimbus/deno' "Policy names canonical local nimbus/deno checkout"
require_contains "${POLICY_DOC}" '/Users/jack/src/github.com/nimbus/rusty_v8' "Policy names canonical local nimbus/rusty_v8 checkout"
# shellcheck disable=SC2016 # Backticks are literal policy text.
require_contains "${POLICY_DOC}" 'Do not use `/private/tmp` checkouts' "Policy forbids temporary checkouts as progress state"
require_contains "${POLICY_DOC}" 'preserve unrelated dirty' "Policy preserves unrelated dirty worktrees"

step 2 "Policy requires candidate proof, publication, immutable repin, and verification"
require_contains "${POLICY_DOC}" 'Temporarily unpin Nimbus' "Policy includes candidate consumer proof"
require_contains "${POLICY_DOC}" 'Commit, tag, and push' "Policy includes explicit commit/tag/push"
require_contains "${POLICY_DOC}" 'Repin Nimbus to published tags' "Policy includes immutable published-tag repin"
require_contains "${POLICY_DOC}" 'scripts/verify-deno-fork-provenance.sh' "Policy requires Deno fork provenance verifier"
require_contains "${POLICY_DOC}" 'scripts/verify-deno-fork-upstream-policy.sh' "Policy requires this upstream-policy verifier"
require_contains "${POLICY_DOC}" '--no-follow-tags' "Policy prevents accidental upstream tag publication"

step 3 "Patch disposition taxonomy is explicit"
require_contains "${POLICY_DOC}" 'Upstream Deno-family' "Policy defines upstream Deno-family disposition"
require_contains "${POLICY_DOC}" 'Nimbus-only host integration' "Policy defines Nimbus-only host-integration disposition"
require_contains "${POLICY_DOC}" 'Temporary carry' "Policy defines temporary-carry disposition"
require_contains "${POLICY_DOC}" 'Removal or upstream trigger' "Policy requires removal or upstream trigger"
require_contains "${POLICY_DOC}" 'Prefer wrappers around upstream logic' "Policy minimizes copied fork logic"

step 4 "Consumed pins are derived and recorded separately from forward releases"
require_contains "${POLICY_DOC}" "consumed.*nimbus/deno.*${DENO_FORK_PATCH_TAG}.*${DENO_FORK_SHA}" "Policy records derived consumed nimbus/deno tag and SHA"
require_contains "${POLICY_DOC}" "consumed.*nimbus/rusty_v8.*${RUSTY_V8_PATCH_TAG}.*${RUSTY_V8_SHA}" "Policy records derived consumed nimbus/rusty_v8 tag and SHA"
require_contains "${POLICY_DOC}" 'published, not consumed.*nimbus/rusty_v8.*-nimbus\.' "Policy distinguishes a published forward rusty_v8 release"
require_contains "${POLICY_DOC}" 'silently change Nimbus.s V8' "Policy prohibits implicit V8-line coupling"

step 5 "Release proof checklist covers the durable safety gates"
require_contains "${POLICY_DOC}" 'Release Proof Checklist' "Policy includes release proof checklist"
require_contains "${POLICY_DOC}" 'peeled annotated tag' "Policy requires tag-to-candidate identity proof"
require_contains "${POLICY_DOC}" 'branch and tag CI are green' "Policy requires exact-commit branch/tag CI"
require_contains "${POLICY_DOC}" 'new default branch' "Policy requires remote default-branch proof"
require_contains "${POLICY_DOC}" 'assets and SHA-256 sidecars' "Policy requires rusty_v8 asset-manifest proof"
# shellcheck disable=SC2016 # Backticks are literal policy text.
require_contains "${POLICY_DOC}" 'resolved `v8` crate version matches' "Policy requires Deno/rusty_v8 coupling proof"
require_contains "${POLICY_DOC}" 'Generated Node evidence' "Policy ties fork bumps to generated Node evidence when claims move"

printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
