#!/usr/bin/env bash
# Completion-gate verifier for
# docs/plans/nimbus-system-bridge-adapters-extraction-plan.md.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/plans/nimbus-system-bridge-adapters-extraction-plan.md"
PROOF_DIR="docs/plans/proof/nimbus-system-bridge-adapters-extraction"
FCE_PROOF_DIR="docs/plans/proof/server-crate-extraction-completion"
SCRIPT="scripts/verify-server-system-bridge-adapters-extraction.sh"

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

rg_no_match() {
  local pattern="$1"
  shift
  if rg -n "${pattern}" "$@" >/tmp/nimbus-sba-rg.out 2>/tmp/nimbus-sba-rg.err; then
    return 1
  fi
  return 0
}

metadata_has_crate() {
  local crate="$1"
  grep -q "\"name\":\"${crate}\"" /tmp/nimbus-sba-metadata.json
}

proof_completed() {
  local proof="$1"
  [ -f "${proof}" ] && grep -q '^Status: completed$' "${proof}"
}

post_sba_adapter_facade_completed() {
  [ -d "crates/nimbus-adapters" ] \
    && metadata_has_crate "nimbus-adapters" \
    && proof_completed "${FCE_PROOF_DIR}/fce9-adapters-facade.md" \
    && proof_completed "${FCE_PROOF_DIR}/fce10-closeout.md" \
    && grep -q 'feature-gated re-export-only facade' "${FCE_PROOF_DIR}/fce9-adapters-facade.md" \
    && grep -q 'Final verifier result: 18 passed; 0 failed' "${FCE_PROOF_DIR}/fce10-closeout.md"
}

printf '\033[1mSBA verification gate - system, bridge, adapters extraction\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

cargo metadata --no-deps --format-version 1 >/tmp/nimbus-sba-metadata.json 2>/tmp/nimbus-sba-metadata.err
METADATA_STATUS=$?

step 1 "Control-plane files and phase proofs exist"
if [ -f "${PLAN}" ] \
   && [ -d "${PROOF_DIR}" ] \
   && [ -x "${SCRIPT}" ] \
   && proof_completed "${PROOF_DIR}/sba0-current-dependency-audit.md" \
   && proof_completed "${PROOF_DIR}/sba1-system-readiness.md" \
   && proof_completed "${PROOF_DIR}/sba2-system-extraction.md" \
   && proof_completed "${PROOF_DIR}/sba3-bridge-readiness.md" \
   && proof_completed "${PROOF_DIR}/sba4-bridge-extraction.md" \
   && proof_completed "${PROOF_DIR}/sba45-auth-extraction.md" \
   && proof_completed "${PROOF_DIR}/sba5-adapters-readiness.md" \
   && proof_completed "${PROOF_DIR}/sba6-adapters-extraction.md" \
   && proof_completed "${PROOF_DIR}/sba7-follow-on-decisions.md"; then
  pass "Plan, verifier, and completed SBA0-SBA7 proofs exist"
else
  fail "Control-plane files incomplete" "Expected plan, executable verifier, proof directory, and completed SBA0-SBA7 proof files"
fi

step 2 "Workspace crate membership matches extraction decisions"
if [ "${METADATA_STATUS}" -eq 0 ] \
   && metadata_has_crate "nimbus-system" \
   && metadata_has_crate "nimbus-bridge" \
   && metadata_has_crate "nimbus-auth" \
   && metadata_has_crate "nimbus-license" \
   && { { [ ! -d "crates/nimbus-adapters" ] \
          && grep -q 'Do not create `nimbus-adapters`' "${PROOF_DIR}/sba6-adapters-extraction.md"; } \
        || post_sba_adapter_facade_completed; }; then
  pass "Extracted crates match SBA decisions or the later FCE adapter-facade proof"
else
  fail "Workspace crate membership mismatch" "Expected nimbus-system, nimbus-bridge, nimbus-auth, nimbus-license; nimbus-adapters is allowed only when FCE9/FCE10 prove it is a facade-only follow-on"
fi

step 3 "nimbus-system boundary has no server or adapter imports"
SYSTEM_TREE="$(cargo tree -p nimbus-system --edges normal --depth 1 2>/tmp/nimbus-sba-system-tree.err)"
if [ $? -eq 0 ] \
   && ! printf '%s' "${SYSTEM_TREE}" | grep -qE 'nimbus-server|nimbus-adapters' \
   && rg_no_match 'use .*nimbus_server|nimbus_server::|nimbus-server[[:space:]]*=|crate::(adapters|router|state|http)::|use crate::(adapters|router|state|http)|ConvexRegistryDeploySummary' crates/nimbus-system -g '*.rs' -g 'Cargo.toml' \
   && grep -q 'pub(crate) use nimbus_system::\*;' crates/nimbus-server/src/system_tenant.rs; then
  pass "nimbus-system is server-free and server system_tenant is a shim"
else
  fail "nimbus-system boundary violation" "Expected no nimbus-server/adapter dependencies or imports and a server shim re-exporting nimbus_system"
fi

step 4 "nimbus-bridge boundary has no server, system, or provider imports"
BRIDGE_TREE="$(cargo tree -p nimbus-bridge --edges normal 2>/tmp/nimbus-sba-bridge-tree.err)"
if [ $? -eq 0 ] \
   && ! printf '%s' "${BRIDGE_TREE}" | grep -qE 'nimbus-server|nimbus-system|nimbus-adapters' \
   && rg_no_match 'nimbus_server|nimbus-server|crate::(adapters|router|state|http|system_tenant|local_server|application_auth)|convex|firebase|firestore|cloud_functions|mongodb' crates/nimbus-bridge -g '*.rs' -g 'Cargo.toml' \
   && rg -n 'TenantIsolationDecision|TenantStorageAccessDecision|TenantServiceAccessDecision|TenantRuntimePolicyAdmission' crates/nimbus-bridge -g '*.rs' >/dev/null; then
  pass "nimbus-bridge depends only on admitted tenant/runtime primitives"
else
  fail "nimbus-bridge boundary violation" "Expected no server/system/provider imports and visible admitted decision/projection usage"
fi

step 5 "nimbus-auth boundary has no server, transport, or operator imports"
AUTH_TREE="$(cargo tree -p nimbus-auth --edges normal 2>/tmp/nimbus-sba-auth-tree.err)"
if [ $? -eq 0 ] \
   && ! printf '%s' "${AUTH_TREE}" | grep -qE 'nimbus-server|nimbus-adapters' \
   && rg_no_match 'nimbus_server|nimbus-server|AppState|DeploymentState|axum|tonic|router|LocalAdmin|local_admin|crate::adapters|ConvexRegistry|FirebaseConfig|CloudFunctionsRegistry' crates/nimbus-auth -g '*.rs' -g 'Cargo.toml'; then
  pass "nimbus-auth owns neutral auth contracts only"
else
  fail "nimbus-auth boundary violation" "Expected no server, transport, local-admin, or adapter-registry imports"
fi

step 6 "nimbus-license boundary has no server, runtime, storage, or adapter imports"
LICENSE_TREE="$(cargo tree -p nimbus-license --edges normal --depth 1 2>/tmp/nimbus-sba-license-tree.err)"
if [ $? -eq 0 ] \
   && printf '%s' "${LICENSE_TREE}" | grep -q 'serde' \
   && printf '%s' "${LICENSE_TREE}" | grep -q 'serde_json' \
   && printf '%s' "${LICENSE_TREE}" | grep -q 'thiserror' \
   && ! printf '%s' "${LICENSE_TREE}" | grep -qE 'nimbus-server|nimbus-engine|nimbus-storage|nimbus-runtime|nimbus-adapters' \
   && rg_no_match 'nimbus_server|nimbus-server|nimbus_engine|nimbus_storage|nimbus_runtime|nimbus_adapters|crate::(state|router|http|adapters|runtime_host|system_tenant|storage|execution)' crates/nimbus-license -g '*.rs' -g 'Cargo.toml' \
   && [ -f crates/nimbus-server/src/license.rs ] \
   && [ ! -d crates/nimbus-server/src/license ]; then
  pass "nimbus-license owns license logic with only a server re-export shim"
else
  fail "nimbus-license boundary violation" "Expected only serde/serde_json/thiserror normal deps, no server-owned source directory, and no server/runtime/storage imports"
fi

step 7 "Server adapters cannot import server-private runtime host internals"
if rg_no_match 'crate::runtime_host|runtime_host::' crates/nimbus-server/src/adapters -g '*.rs' \
   && rg_no_match 'mod runtime_host|crate::runtime_host' crates/nimbus-server/src -g '*.rs'; then
  pass "Adapters route through nimbus-bridge APIs instead of server runtime_host"
else
  fail "Runtime host internals are still imported by server adapter code" "Expected no crate::runtime_host or runtime_host:: references"
fi

step 8 "Server consumers use neutral auth from nimbus-auth"
if rg_no_match 'crate::application_auth::(ApplicationAuthVerifier|normalize_principal_context|ResolvedApplicationAuth)|use crate::application_auth::\{[^\n]*(ApplicationAuthVerifier|normalize_principal_context|ResolvedApplicationAuth)' crates/nimbus-server/src -g '*.rs' \
   && grep -q 'pub use nimbus_license::\*;' crates/nimbus-server/src/license.rs; then
  pass "Neutral auth and license consumers route through extracted crates"
else
  fail "Extracted neutral contract still imported through server-private modules" "Expected auth contracts from nimbus-auth and license logic from nimbus-license"
fi

step 9 "Follow-on decisions are recorded and non-decorative"
if grep -q '## `nimbus-artifacts` Decision' "${PROOF_DIR}/sba7-follow-on-decisions.md" \
   && grep -q '## `nimbus-provenance` Decision' "${PROOF_DIR}/sba7-follow-on-decisions.md" \
   && grep -q '## `nimbus-operator` Decision' "${PROOF_DIR}/sba7-follow-on-decisions.md" \
   && grep -q '## `nimbus-services` Decision' "${PROOF_DIR}/sba7-follow-on-decisions.md" \
   && grep -q '## `nimbus-license` Decision' "${PROOF_DIR}/sba7-follow-on-decisions.md" \
   && grep -q 'Extract `nimbus-license`' "${PROOF_DIR}/sba7-follow-on-decisions.md" \
   && grep -q 'Keep for now; do not create `nimbus-artifacts`' "${PROOF_DIR}/sba7-follow-on-decisions.md" \
   && grep -q 'Keep for now; do not create `nimbus-provenance`' "${PROOF_DIR}/sba7-follow-on-decisions.md" \
   && grep -q 'Keep for now; do not create `nimbus-operator`' "${PROOF_DIR}/sba7-follow-on-decisions.md" \
   && grep -q 'Keep for now; do not create `nimbus-services`' "${PROOF_DIR}/sba7-follow-on-decisions.md"; then
  pass "All ordered SBA7 follow-on decisions are explicit"
else
  fail "SBA7 follow-on decisions incomplete" "Expected extract/keep sections for artifacts, provenance, operator, services, and license"
fi

step 10 "_nimbus writes and service evidence remain system-owned"
if rg -n 'record_service_handle_async|record_system_event_async|prepare_system_tenant_async' crates/nimbus-system crates/nimbus-server/src -g '*.rs' >/dev/null \
   && rg_no_match 'upsert_system_document' crates/nimbus-server/src/adapters -g '*.rs'; then
  pass "System evidence writes remain centralized through nimbus-system/server composition"
else
  fail "System evidence write boundary unclear" "Expected system writers in nimbus-system/server composition and no direct adapter upsert/state writers"
fi

step 11 "Focused behavior tests are recorded in phase proofs"
if grep -q 'cargo test -p nimbus-system' "${PROOF_DIR}/sba2-system-extraction.md" \
   && grep -q 'cargo test -p nimbus-bridge' "${PROOF_DIR}/sba4-bridge-extraction.md" \
   && grep -q 'cargo test -p nimbus-auth' "${PROOF_DIR}/sba45-auth-extraction.md" \
   && grep -q 'cargo test -p nimbus-license' "${PROOF_DIR}/sba7-follow-on-decisions.md" \
   && grep -q 'cargo test -p nimbus-server license' "${PROOF_DIR}/sba7-follow-on-decisions.md"; then
  pass "Focused tests are named with pass counts in proof artifacts"
else
  fail "Focused test evidence missing" "Expected system, bridge, auth, license, and server license focused tests recorded in proofs"
fi

step 12 "Workspace check passes"
if cargo check --workspace; then
  pass "cargo check --workspace passed"
else
  fail "cargo check --workspace failed"
fi

printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"

if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
