#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Tenant and Node Crate Extraction
# Readiness plan (`docs/plans/tenant-and-node-crate-extraction-readiness-plan.md`).
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/plans/tenant-and-node-crate-extraction-readiness-plan.md"
PROOF_DIR="docs/plans/proof/tenant-node-extraction-readiness"
TENANT_CRATE="crates/nimbus-tenant"
NODE_CRATE="crates/nimbus-node"
SERVER_CRATE="crates/nimbus-server"

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

line_count() {
  wc -l | tr -d ' '
}

run_cmd() {
  local title="$1"
  shift
  local tmp
  tmp="$(mktemp)"
  if "$@" >"${tmp}" 2>&1; then
    pass "${title}"
  else
    fail "${title}" "$(tail -n 30 "${tmp}" | tr '\n' '; ' | head -c 800)"
  fi
  rm -f "${tmp}"
}

printf '\033[1mTNE verification gate - tenant-node-extraction-readiness\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

step 1 "Plan and proof files exist"
PROOF_DETAIL=()
[ -f "${PLAN}" ] || PROOF_DETAIL+=("${PLAN} missing")
for proof in \
  tne1-artifact-verifier-effects.md \
  tne2-nimbus-tenant-extraction.md \
  tne3-node-reconciler.md \
  tne4-nimbus-node-extraction.md \
  tne5-closeout.md; do
  [ -f "${PROOF_DIR}/${proof}" ] || PROOF_DETAIL+=("${PROOF_DIR}/${proof} missing")
done
if [ "${#PROOF_DETAIL[@]}" -eq 0 ]; then
  pass "Plan and TNE1-TNE5 proof files exist"
else
  fail "Plan/proof files missing" "$(printf '%s; ' "${PROOF_DETAIL[@]}")"
fi

step 2 "Workspace members and server shim are wired"
WIRE_DETAIL=()
grep -q '"crates/nimbus-tenant"' Cargo.toml || WIRE_DETAIL+=("nimbus-tenant missing from workspace")
grep -q '"crates/nimbus-node"' Cargo.toml || WIRE_DETAIL+=("nimbus-node missing from workspace")
[ -f "${TENANT_CRATE}/Cargo.toml" ] || WIRE_DETAIL+=("${TENANT_CRATE}/Cargo.toml missing")
[ -f "${NODE_CRATE}/Cargo.toml" ] || WIRE_DETAIL+=("${NODE_CRATE}/Cargo.toml missing")
grep -q 'nimbus-node = { path = "../nimbus-node" }' "${SERVER_CRATE}/Cargo.toml" \
  || WIRE_DETAIL+=("nimbus-server does not depend on nimbus-node")
if [ "$(tr -d '[:space:]' < "${SERVER_CRATE}/src/local_enforcement.rs")" != "pubusenimbus_node::*;" ]; then
  WIRE_DETAIL+=("nimbus-server local_enforcement shim is not the intentional re-export")
fi
if [ "${#WIRE_DETAIL[@]}" -eq 0 ]; then
  pass "Workspace and server shim point at extracted crates"
else
  fail "Workspace/server wiring incomplete" "$(printf '%s; ' "${WIRE_DETAIL[@]}")"
fi

step 3 "nimbus-tenant has no host/server side-effect imports"
TENANT_FORBIDDEN="$(grep -R -nE 'nimbus_server|nimbus_storage|nimbus_engine|system_tenant|HostLifecycle|RuntimeExecutor|std::process|Command::new|Stdio|std::fs::|compute_sha256_for_path|axum::|tokio::|tonic::|tower::' "${TENANT_CRATE}/src" "${TENANT_CRATE}/Cargo.toml" 2>/dev/null || true)"
if [ -z "${TENANT_FORBIDDEN}" ]; then
  pass "nimbus-tenant production files have no forbidden host/server imports"
else
  fail "nimbus-tenant forbidden imports found" "$(printf '%s' "${TENANT_FORBIDDEN}" | head -c 800)"
fi

step 4 "nimbus-node normal dependencies are narrow"
NODE_TREE="$(cargo tree -p nimbus-node -e normal --depth 1 2>&1)"
NODE_BAD_DEPS="$(printf '%s\n' "${NODE_TREE}" | grep -E 'nimbus-server|nimbus-storage|nimbus-engine|nimbus-machine|axum|tonic|tower|mongodb|firebase|convex|tokio|reqwest' || true)"
if [ -z "${NODE_BAD_DEPS}" ]; then
  pass "nimbus-node normal dependency tree excludes server/adapters/storage/persistence"
else
  fail "nimbus-node dependency tree includes forbidden deps" "$(printf '%s' "${NODE_BAD_DEPS}" | head -c 800)"
fi

step 5 "nimbus-node source has no server/adapters/persistence imports"
NODE_FORBIDDEN="$(grep -R -nE 'nimbus_server|nimbus_storage|nimbus_engine|nimbus_machine|system_tenant|crate::system|crate::adapters|axum::|tonic::|tower::|mongodb::|firebase::|std::process|Command::new|\bRuntimeExecutor\b|\bHostBridge\b|\bTableName\b|insert_document|upsert_system_document' "${NODE_CRATE}/src" "${NODE_CRATE}/Cargo.toml" 2>/dev/null || true)"
if [ -z "${NODE_FORBIDDEN}" ]; then
  pass "nimbus-node source excludes server/adapters/persistence imports"
else
  fail "nimbus-node forbidden imports found" "$(printf '%s' "${NODE_FORBIDDEN}" | head -c 800)"
fi

step 6 "Host lifecycle reconciler and writer inversion exist"
RECONCILER_DETAIL=()
for call in validate inspect start stop; do
  grep -q "self.backend.${call}" "${NODE_CRATE}/src/reconciler.rs" \
    || RECONCILER_DETAIL+=("NodeWorkloadReconciler missing ${call} call")
done
grep -q 'trait StatusEvidenceWriter' "${NODE_CRATE}/src/reconciler.rs" \
  || RECONCILER_DETAIL+=("StatusEvidenceWriter trait missing from nimbus-node")
grep -q 'impl StatusEvidenceWriter for SystemTenantStatusEvidenceWriter' "${SERVER_CRATE}/src/system_tenant/records.rs" \
  || RECONCILER_DETAIL+=("server-owned SystemTenantStatusEvidenceWriter impl missing")
grep -q 'ensure_system_or_operator_authority("_nimbus workload status projection")' "${SERVER_CRATE}/src/system_tenant/records.rs" \
  || RECONCILER_DETAIL+=("_nimbus workload status writer lacks system/operator authority check")
grep -q 'projection.ensure_status_matches(status)' "${SERVER_CRATE}/src/system_tenant/records.rs" \
  || RECONCILER_DETAIL+=("_nimbus workload status writer lacks projection/status match check")
if [ "${#RECONCILER_DETAIL[@]}" -eq 0 ]; then
  pass "Reconciler calls lifecycle backend and persistence stays inverted"
else
  fail "Reconciler/writer inversion incomplete" "$(printf '%s; ' "${RECONCILER_DETAIL[@]}")"
fi

step 7 "Focused security tests pass"
run_cmd "cargo test -p nimbus-tenant" cargo test -p nimbus-tenant
run_cmd "cargo test -p nimbus-node" cargo test -p nimbus-node
run_cmd "cargo test -p nimbus-server artifact_verifier_effects" cargo test -p nimbus-server artifact_verifier_effects
run_cmd "cargo test -p nimbus-server system_tenant" cargo test -p nimbus-server system_tenant -- --test-threads=1
run_cmd "cargo test -p nimbus-server tenant_isolation" cargo test -p nimbus-server tenant_isolation -- --test-threads=1

step 8 "Workspace and docs checks pass"
run_cmd "cargo check --workspace" cargo check --workspace
run_cmd "cargo clippy -p nimbus-tenant --all-targets --no-deps" cargo clippy -p nimbus-tenant --all-targets --no-deps
run_cmd "cargo clippy -p nimbus-node --all-targets --no-deps" cargo clippy -p nimbus-node --all-targets --no-deps
run_cmd "cargo clippy -p nimbus-server --all-targets --no-deps" cargo clippy -p nimbus-server --all-targets --no-deps
run_cmd "cargo fmt --all --check" cargo fmt --all --check
run_cmd "git diff --check" git diff --check
run_cmd "npm run docs:validate-refs:strict" npm run docs:validate-refs:strict

printf '\n\033[1mSummary\033[0m\n'
printf 'PASS: %s\n' "${PASS}"
printf 'FAIL: %s\n' "${FAIL}"

if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf ' - %s\n' "${detail}"
  done
  exit 1
fi

printf 'All tenant/node extraction readiness checks passed.\n'
