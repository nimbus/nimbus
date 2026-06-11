#!/usr/bin/env bash
# Completion-gate verifier for
# docs/private/plans/server-seam-extraction-readiness-plan.md.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/private/plans/server-seam-extraction-readiness-plan.md"
PROOF_DIR="docs/private/plans/proof/server-seam-extraction-readiness"
SCRIPT="scripts/verify-server-seam-extraction-readiness.sh"
PREVIOUS_SCRIPT="scripts/verify-server-system-bridge-adapters-extraction.sh"

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

proof_completed() {
  local proof="$1"
  [ -f "${proof}" ] && grep -q '^Status: completed$' "${proof}"
}

rg_no_match() {
  local pattern="$1"
  shift
  if rg -n "${pattern}" "$@" >/tmp/nimbus-sse-rg.out 2>/tmp/nimbus-sse-rg.err; then
    return 1
  fi
  return 0
}

metadata_has_crate() {
  local crate="$1"
  grep -q "\"name\":\"${crate}\"" /tmp/nimbus-sse-metadata.json
}

printf '\033[1mSSE verification gate - server seam extraction readiness\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

cargo metadata --no-deps --format-version 1 >/tmp/nimbus-sse-metadata.json 2>/tmp/nimbus-sse-metadata.err
METADATA_STATUS=$?

step 1 "Control-plane files and SSE0 proof exist"
if [ -f "${PLAN}" ] \
   && [ -d "${PROOF_DIR}" ] \
   && [ -x "${SCRIPT}" ] \
   && proof_completed "${PROOF_DIR}/sse0-baseline-seam-audit.md"; then
  pass "Plan, executable verifier, proof directory, and completed SSE0 proof exist"
else
  fail "SSE0 control-plane files incomplete" "Expected plan, executable verifier, proof directory, and completed sse0-baseline-seam-audit.md"
fi

step 2 "Previous extraction verifier remains authoritative"
if [ -x "${PREVIOUS_SCRIPT}" ] && "${PREVIOUS_SCRIPT}" >/tmp/nimbus-sse-previous-verifier.out 2>&1; then
  pass "Previous system/bridge/auth/license extraction verifier passed"
else
  fail "Previous extraction verifier failed" "Expected ${PREVIOUS_SCRIPT} to pass before continuing server seam readiness"
fi

step 3 "SSE0 proof records the baseline owner and import graph"
if grep -q '## Owner Table' "${PROOF_DIR}/sse0-baseline-seam-audit.md" \
   && grep -q '## Import Graph Summary' "${PROOF_DIR}/sse0-baseline-seam-audit.md" \
   && grep -q '## Denied Import Patterns' "${PROOF_DIR}/sse0-baseline-seam-audit.md" \
   && grep -q 'MongoDB adapter' "${PROOF_DIR}/sse0-baseline-seam-audit.md" \
   && grep -q 'Firebase/provider-family' "${PROOF_DIR}/sse0-baseline-seam-audit.md" \
   && grep -q 'Cloud Functions adapter' "${PROOF_DIR}/sse0-baseline-seam-audit.md" \
   && grep -q 'Convex adapter' "${PROOF_DIR}/sse0-baseline-seam-audit.md" \
   && grep -q 'Artifact verifier effects' "${PROOF_DIR}/sse0-baseline-seam-audit.md" \
   && grep -q 'Provenance/runtime bundle admission' "${PROOF_DIR}/sse0-baseline-seam-audit.md" \
   && grep -q 'Services/service registry/sandbox service traits' "${PROOF_DIR}/sse0-baseline-seam-audit.md" \
   && grep -q 'Operator/local admin' "${PROOF_DIR}/sse0-baseline-seam-audit.md"; then
  pass "SSE0 proof includes owner table, import graph, and denied patterns"
else
  fail "SSE0 proof baseline evidence incomplete" "Expected owner table, import graph summary, denied patterns, and all retained seam owners"
fi

step 4 "Known extracted crates remain present and server-free"
SYSTEM_TREE="$(cargo tree -p nimbus-system --edges normal --depth 1 2>/tmp/nimbus-sse-system-tree.err)"
BRIDGE_TREE="$(cargo tree -p nimbus-bridge --edges normal --depth 1 2>/tmp/nimbus-sse-bridge-tree.err)"
AUTH_TREE="$(cargo tree -p nimbus-auth --edges normal --depth 1 2>/tmp/nimbus-sse-auth-tree.err)"
LICENSE_TREE="$(cargo tree -p nimbus-license --edges normal --depth 1 2>/tmp/nimbus-sse-license-tree.err)"
if [ "${METADATA_STATUS}" -eq 0 ] \
   && metadata_has_crate "nimbus-system" \
   && metadata_has_crate "nimbus-bridge" \
   && metadata_has_crate "nimbus-auth" \
   && metadata_has_crate "nimbus-license" \
   && [ ! -d "crates/nimbus-adapters" ] \
   && ! printf '%s\n%s\n%s\n%s' "${SYSTEM_TREE}" "${BRIDGE_TREE}" "${AUTH_TREE}" "${LICENSE_TREE}" | grep -qE 'nimbus-server|nimbus-adapters' \
   && rg_no_match 'nimbus_server::|use .*nimbus_server|nimbus-server[[:space:]]*=' crates/nimbus-system crates/nimbus-bridge crates/nimbus-auth crates/nimbus-license -g '*.rs' -g 'Cargo.toml'; then
  pass "Previously extracted crates are present and do not import nimbus-server"
else
  fail "Previously extracted crate boundary drift" "Expected nimbus-system, nimbus-bridge, nimbus-auth, and nimbus-license to stay server-free with no aggregate nimbus-adapters crate"
fi

step 5 "Baseline runtime bridge and system ownership guards still hold"
if rg_no_match 'crate::runtime_host|runtime_host::' crates/nimbus-server/src/adapters -g '*.rs' \
   && rg_no_match 'upsert_system_document' crates/nimbus-server/src/adapters -g '*.rs' \
   && rg -n 'record_service_handle_async|record_system_event_async|prepare_system_tenant_async' crates/nimbus-system crates/nimbus-server/src -g '*.rs' >/dev/null; then
  pass "Adapters do not import server runtime_host and direct adapter _nimbus upserts are absent"
else
  fail "Baseline bridge/system ownership guard failed" "Expected adapter runtime calls through nimbus-bridge and _nimbus evidence centralized through nimbus-system/server composition"
fi

step 6 "SSE1A MongoDB readiness proof is complete"
if proof_completed "${PROOF_DIR}/sse1a-mongodb-adapter-readiness.md" \
   && grep -q 'MongoDB is ready for a per-adapter extraction decision' "${PROOF_DIR}/sse1a-mongodb-adapter-readiness.md" \
   && grep -q 'nimbus_tenant::TenantIsolationContext' "${PROOF_DIR}/sse1a-mongodb-adapter-readiness.md" \
   && grep -q 'explicit capability dependency or a narrower MongoDB command trait' "${PROOF_DIR}/sse1a-mongodb-adapter-readiness.md" \
   && grep -q '266 passed, 0 failed' "${PROOF_DIR}/sse1a-mongodb-adapter-readiness.md" \
   && grep -q '23 passed, 0 failed' "${PROOF_DIR}/sse1a-mongodb-adapter-readiness.md" \
   && rg_no_match 'AppState|DeploymentState|RouterBuildConfig|crate::router|crate::local_server|crate::system_tenant|crate::application_auth|crate::runtime_host|crate::tenant|std::process::Command' crates/nimbus-server/src/adapters/mongodb -g '*.rs'; then
  pass "MongoDB has a completed extraction-ready decision with focused test evidence and no server-private imports"
else
  fail "MongoDB readiness evidence incomplete" "Expected completed SSE1A proof, extraction-ready decision, focused test counts, nimbus-tenant use, and no denied server-private imports"
fi

step 7 "SSE1B Firebase/provider-family readiness proof is complete"
if proof_completed "${PROOF_DIR}/sse1b-firebase-provider-readiness.md" \
   && grep -q 'Moved Firestore model helpers' "${PROOF_DIR}/sse1b-firebase-provider-readiness.md" \
   && grep -q 'operations.rs` to accept `&Arc<nimbus_engine::Service>`' "${PROOF_DIR}/sse1b-firebase-provider-readiness.md" \
   && grep -q 'Firebase/provider-family is ready for partial extraction' "${PROOF_DIR}/sse1b-firebase-provider-readiness.md" \
   && grep -q '142 passed, 0 failed' "${PROOF_DIR}/sse1b-firebase-provider-readiness.md" \
   && [ -f "crates/nimbus-server/src/adapters/firebase/firestore_model.rs" ] \
   && rg_no_match 'provider_family|mod provider_family' crates/nimbus-server/src -g '*.rs' \
   && rg_no_match 'AppState|crate::tenant|crate::provider_family|crate::application_auth|crate::system_tenant|crate::local_server|crate::runtime_host|RouterBuildConfig|std::process::Command' crates/nimbus-server/src/adapters/firebase/operations.rs \
   && rg_no_match 'AppState' crates/nimbus-server/src/adapters/firebase/grpc/write_stream.rs crates/nimbus-server/src/adapters/firebase/grpc/listen_stream.rs; then
  pass "Firebase/provider-family has cleaned model/operation seams, focused test evidence, and recorded transport/auth blockers"
else
  fail "Firebase/provider-family readiness evidence incomplete" "Expected completed SSE1B proof, provider_family removal, operations without AppState/server shims, streaming core without AppState, and focused test counts"
fi

step 8 "SSE1C Cloud Functions readiness proof is complete"
if proof_completed "${PROOF_DIR}/sse1c-cloud-functions-readiness.md" \
   && grep -q 'CloudFunctionsRuntimeContext' "${PROOF_DIR}/sse1c-cloud-functions-readiness.md" \
   && grep -q 'blocked by runtime invocation/provenance and' "${PROOF_DIR}/sse1c-cloud-functions-readiness.md" \
   && grep -q '39 passed, 0 failed' "${PROOF_DIR}/sse1c-cloud-functions-readiness.md" \
   && rg_no_match 'AppState|crate::tenant|crate::runtime_host|crate::provider_family|crate::system_tenant' crates/nimbus-server/src/adapters/cloud_functions/http/invocation.rs crates/nimbus-server/src/adapters/cloud_functions/execution.rs \
   && rg_no_match 'crate::runtime_host|crate::provider_family|crate::system_tenant' crates/nimbus-server/src/adapters/cloud_functions -g '*.rs'; then
  pass "Cloud Functions has narrowed runtime invocation, bridge/tenant imports, focused tests, and recorded runtime/provenance blockers"
else
  fail "Cloud Functions readiness evidence incomplete" "Expected completed SSE1C proof, CloudFunctionsRuntimeContext cleanup, focused test counts, no runtime_host/provider_family/system_tenant imports, and recorded blockers"
fi

step 9 "SSE1D Convex readiness proof is complete"
if proof_completed "${PROOF_DIR}/sse1d-convex-adapter-readiness.md" \
   && grep -q 'Convex remains the largest adapter and is not ready for whole-adapter' "${PROOF_DIR}/sse1d-convex-adapter-readiness.md" \
   && grep -q 'no `crate::tenant` import remains' "${PROOF_DIR}/sse1d-convex-adapter-readiness.md" \
   && grep -q 'no `crate::system_tenant` import remains' "${PROOF_DIR}/sse1d-convex-adapter-readiness.md" \
   && grep -q 'subscription transform planning' "${PROOF_DIR}/sse1d-convex-adapter-readiness.md" \
   && grep -q '132 passed, 0 failed, 5 ignored' "${PROOF_DIR}/sse1d-convex-adapter-readiness.md" \
   && grep -q '18 passed, 0 failed' "${PROOF_DIR}/sse1d-convex-adapter-readiness.md" \
   && grep -q 'Aggregate `nimbus-adapters` remains rejected' "${PROOF_DIR}/sse1d-convex-adapter-readiness.md" \
   && rg -n 'nimbus_tenant::TenantIsolationContext|nimbus_system::record_subscription_state_async|nimbus_auth::normalize_principal_context|nimbus_bridge::admission::RuntimeExecutionAdmission' crates/nimbus-server/src/adapters/convex -g '*.rs' >/dev/null \
   && rg -n 'resolve_convex_document_id\(&table, id\)\?\.into_document_id\(\)' crates/nimbus-server/src/adapters/convex/subscriptions/transforms/planner.rs >/dev/null \
   && rg_no_match 'crate::tenant|crate::system_tenant|crate::runtime_host|runtime_host::|upsert_system_document' crates/nimbus-server/src/adapters/convex -g '*.rs'; then
  pass "Convex uses canonical tenant/system/bridge/auth primitives, preserves focused behavior tests, and records honest extraction blockers"
else
  fail "Convex readiness evidence incomplete" "Expected completed SSE1D proof, canonical crate imports, table-scoped subscription planner fix, focused test counts, no tenant/system/runtime-host server shims, and aggregate-adapter rejection"
fi

step 10 "SSE2 artifact effects readiness proof is complete"
if proof_completed "${PROOF_DIR}/sse2-artifact-effects-readiness.md" \
   && grep -q 'ProcessArtifactVerifierCommandRunner' "${PROOF_DIR}/sse2-artifact-effects-readiness.md" \
   && grep -q '37 passed, 0 failed' "${PROOF_DIR}/sse2-artifact-effects-readiness.md" \
   && grep -q '7 passed, 0 failed' "${PROOF_DIR}/sse2-artifact-effects-readiness.md" \
   && grep -q '`nimbus-artifacts` remains blocked' "${PROOF_DIR}/sse2-artifact-effects-readiness.md" \
   && [ -f "crates/nimbus-server/src/artifact_verifier_effects/process.rs" ] \
   && rg -n 'Command::new' crates/nimbus-server/src/artifact_verifier_effects/process.rs >/dev/null \
   && rg_no_match 'crate::tenant' crates/nimbus-server/src/artifact_verifier_effects.rs crates/nimbus-server/src/artifact_verifier_effects -g '*.rs' \
   && rg_no_match 'std::process|Command::new|Stdio|ProcessArtifactVerifierCommandRunner' crates/nimbus-tenant/src -g '*.rs' \
   && rg_no_match 'std::process|Command::new|Stdio' crates/nimbus-server/src/artifact_verifier_effects.rs crates/nimbus-server/src/artifact_verifier_effects/cosign.rs crates/nimbus-server/src/artifact_verifier_effects/sbom.rs crates/nimbus-server/src/artifact_verifier_effects/slsa.rs; then
  pass "Artifact effects are split from pure tenant contracts, process execution is isolated, and focused tests are recorded"
else
  fail "Artifact effects readiness evidence incomplete" "Expected completed SSE2 proof, process runner isolated in process.rs, no server tenant shim imports, no process execution in nimbus-tenant or artifact-effect root/backends, and focused test counts"
fi

step 11 "SSE3 provenance readiness proof is complete"
if proof_completed "${PROOF_DIR}/sse3-provenance-readiness.md" \
   && grep -q 'RuntimeBundleProvenanceConfig' "${PROOF_DIR}/sse3-provenance-readiness.md" \
   && grep -q '4 passed, 0 failed' "${PROOF_DIR}/sse3-provenance-readiness.md" \
   && grep -q '2 passed, 0 failed' "${PROOF_DIR}/sse3-provenance-readiness.md" \
   && grep -q '15 passed, 0 failed' "${PROOF_DIR}/sse3-provenance-readiness.md" \
   && grep -q '11 passed, 0 failed, 1 ignored' "${PROOF_DIR}/sse3-provenance-readiness.md" \
   && grep -q '1 passed, 0 failed' "${PROOF_DIR}/sse3-provenance-readiness.md" \
   && grep -q '`nimbus-provenance` remains blocked' "${PROOF_DIR}/sse3-provenance-readiness.md" \
   && [ -f "crates/nimbus-server/src/execution/invocations/provenance.rs" ] \
   && rg -n 'struct RuntimeBundleProvenanceConfig' crates/nimbus-server/src/execution/invocations/provenance.rs >/dev/null \
   && rg -n 'use nimbus_tenant::\{ArtifactVerificationPolicy, ArtifactVerifierBackend\}' crates/nimbus-server/src/adapters/cloud_functions/registry.rs crates/nimbus-server/src/adapters/convex/registry/loading.rs >/dev/null \
   && rg_no_match 'crate::ArtifactVerificationPolicy|crate::ArtifactVerifierBackend|crate::tenant::Artifact|crate::tenant::SLSA|crate::tenant::TenantImage|crate::tenant::ArtifactVerifier' crates/nimbus-server/src/execution crates/nimbus-server/src/adapters/cloud_functions crates/nimbus-server/src/adapters/convex -g '*.rs'; then
  pass "Provenance ownership is split by real owner, runtime admission is isolated, and focused integrity tests are recorded"
else
  fail "Provenance readiness evidence incomplete" "Expected completed SSE3 proof, runtime provenance module, direct nimbus-tenant artifact contracts, denied facade imports, focused test counts, and blocked nimbus-provenance decision"
fi

step 12 "SSE4 services readiness proof is complete"
if proof_completed "${PROOF_DIR}/sse4-services-readiness.md" \
   && grep -q 'ServiceEvidenceWriter' "${PROOF_DIR}/sse4-services-readiness.md" \
   && grep -q '14 passed, 0 failed, 0 ignored' "${PROOF_DIR}/sse4-services-readiness.md" \
   && grep -q '5 passed, 0 failed, 0 ignored' "${PROOF_DIR}/sse4-services-readiness.md" \
   && grep -q '7 passed, 0 failed, 1 ignored' "${PROOF_DIR}/sse4-services-readiness.md" \
   && grep -q '`nimbus-services` remains blocked' "${PROOF_DIR}/sse4-services-readiness.md" \
   && rg -n 'trait ServiceEvidenceWriter' crates/nimbus-server/src/service_manager/system_state.rs >/dev/null \
   && rg -n 'nimbus_system::record_service_handle_async' crates/nimbus-server/src/service_manager/system_state.rs >/dev/null \
   && rg -n 'nimbus_node::LocalEnforcementBinding|nimbus_node::\{LocalEnforcementBinding, TenantEgressReloadRequest\}' crates/nimbus-server/src/service_manager/activation.rs crates/nimbus-server/src/service_manager/launch.rs >/dev/null \
   && rg -n 'nimbus_tenant::TenantServiceAccessDecision' crates/nimbus-server/src/service_registry.rs >/dev/null \
   && rg_no_match 'crate::system_tenant|crate::local_enforcement|crate::tenant::|use crate::tenant|AppState' crates/nimbus-server/src/service_manager.rs crates/nimbus-server/src/service_manager crates/nimbus-server/src/service_registry.rs -g '*.rs'; then
  pass "Services invert system evidence writes, use canonical tenant/node imports, preserve focused tests, and record blocked nimbus-services decision"
else
  fail "Services readiness evidence incomplete" "Expected completed SSE4 proof, ServiceEvidenceWriter, nimbus-system writer adapter, nimbus-node/nimbus-tenant imports, no server tenant/system/local-enforcement shims, focused service test counts, and blocked nimbus-services decision"
fi

step 13 "SSE5 operator readiness proof is complete"
if proof_completed "${PROOF_DIR}/sse5-operator-readiness.md" \
   && grep -q 'access_policy.rs' "${PROOF_DIR}/sse5-operator-readiness.md" \
   && grep -q '3 passed, 0 failed, 0 ignored' "${PROOF_DIR}/sse5-operator-readiness.md" \
   && grep -q '13 passed, 0 failed, 0 ignored' "${PROOF_DIR}/sse5-operator-readiness.md" \
   && grep -q '4 passed, 0 failed, 0 ignored' "${PROOF_DIR}/sse5-operator-readiness.md" \
   && grep -q '10 passed, 0 failed, 0 ignored' "${PROOF_DIR}/sse5-operator-readiness.md" \
   && grep -q '`nimbus-operator` remains blocked' "${PROOF_DIR}/sse5-operator-readiness.md" \
   && [ -f "crates/nimbus-server/src/local_server/access_policy.rs" ] \
   && rg -n 'authorize_deploy_admin_bearer' crates/nimbus-server/src/http/deploy.rs crates/nimbus-server/src/local_server/access_policy.rs >/dev/null \
   && rg -n 'extract_required_bearer_token' crates/nimbus-server/src/http/local_admin.rs crates/nimbus-server/src/local_server/access_policy.rs >/dev/null \
   && rg -n 'nimbus_system::record_system_event_async' crates/nimbus-server/src/http/local_admin.rs >/dev/null \
   && rg -n 'nimbus_system::record_deployment_state_async|nimbus_system::record_listener_state_async|nimbus_system::install_table_projection_observer' crates/nimbus-server/src/http/deploy.rs crates/nimbus-server/src/router.rs >/dev/null \
   && rg_no_match 'AppState|axum::middleware|axum::Router|RouterBuildConfig|ApplicationAuthVerifier|crate::system_tenant|crate::adapters|TenantIsolation|nimbus_auth' crates/nimbus-server/src/local_server/access_policy.rs \
   && rg_no_match 'crate::system_tenant' crates/nimbus-server/src/http/local_admin.rs crates/nimbus-server/src/http/deploy.rs crates/nimbus-server/src/router.rs crates/nimbus-server/src/local_server -g '*.rs'; then
  pass "Operator access policy is transport-free, system evidence routes through nimbus-system, focused tests pass, and nimbus-operator remains honestly blocked"
else
  fail "Operator readiness evidence incomplete" "Expected completed SSE5 proof, transport-free access_policy.rs, shared deploy/local-admin bearer policy, nimbus-system evidence writes, no server system_tenant shim in operator files, focused test counts, and blocked nimbus-operator decision"
fi

step 14 "SSE6 extraction decisions proof is complete"
if proof_completed "${PROOF_DIR}/sse6-extraction-decisions.md" \
   && grep -q 'Aggregate `nimbus-adapters`' "${PROOF_DIR}/sse6-extraction-decisions.md" \
   && grep -q 'MongoDB adapter | ready for targeted per-adapter extraction' "${PROOF_DIR}/sse6-extraction-decisions.md" \
   && grep -q 'Firebase/provider-family | partial-ready; full extraction blocked' "${PROOF_DIR}/sse6-extraction-decisions.md" \
   && grep -q 'Cloud Functions | partial-ready; full extraction blocked' "${PROOF_DIR}/sse6-extraction-decisions.md" \
   && grep -q 'Convex | selected subtrees ready; whole adapter blocked' "${PROOF_DIR}/sse6-extraction-decisions.md" \
   && grep -q '`nimbus-artifacts` | blocked' "${PROOF_DIR}/sse6-extraction-decisions.md" \
   && grep -q '`nimbus-provenance` | blocked' "${PROOF_DIR}/sse6-extraction-decisions.md" \
   && grep -q '`nimbus-services` | blocked' "${PROOF_DIR}/sse6-extraction-decisions.md" \
   && grep -q '`nimbus-operator` | blocked' "${PROOF_DIR}/sse6-extraction-decisions.md" \
   && grep -q 'Decision: no new crate extraction in this phase' "${PROOF_DIR}/sse6-extraction-decisions.md" \
   && [ ! -d "crates/nimbus-adapters" ] \
   && [ ! -d "crates/nimbus-artifacts" ] \
   && [ ! -d "crates/nimbus-provenance" ] \
   && [ ! -d "crates/nimbus-services" ] \
   && [ ! -d "crates/nimbus-operator" ]; then
  pass "Extraction decisions reject aggregate/decorative crates, preserve MongoDB targeted readiness, and record blockers for remaining candidates"
else
  fail "Extraction decision evidence incomplete" "Expected completed SSE6 proof, explicit decisions for all adapter/artifact/provenance/services/operator candidates, no premature decorative crates, and no aggregate nimbus-adapters"
fi

step 15 "Phase ledger has SSE0-SSE7 completed"
if grep -q '| SSE0 Baseline seam audit and verifier skeleton | completed |' "${PLAN}" \
   && grep -q '| SSE1A MongoDB adapter readiness | completed |' "${PLAN}" \
   && grep -q '| SSE1B Firebase/provider-family adapter readiness | completed |' "${PLAN}" \
   && grep -q '| SSE1C Cloud Functions adapter readiness | completed |' "${PLAN}" \
   && grep -q '| SSE1D Convex adapter readiness | completed |' "${PLAN}" \
   && grep -q '| SSE2 Artifact effects readiness | completed |' "${PLAN}" \
   && grep -q '| SSE3 Provenance readiness | completed |' "${PLAN}" \
   && grep -q '| SSE4 Services readiness | completed |' "${PLAN}" \
   && grep -q '| SSE5 Operator readiness | completed |' "${PLAN}" \
   && grep -q '| SSE6 Extraction decisions | completed |' "${PLAN}" \
   && grep -q '| SSE7 Final verifier closeout | completed |' "${PLAN}" \
   && grep -q '^Status: completed$' "${PLAN}"; then
  pass "Plan ledger marks every phase and the plan complete"
else
  fail "Plan ledger state is not complete" "Expected plan status completed and SSE0-SSE7 completed"
fi

step 16 "SSE7 final verifier closeout proof is complete"
if proof_completed "${PROOF_DIR}/sse7-verifier-closeout.md" \
   && grep -q '15 passed, 0 failed' "${PROOF_DIR}/sse7-verifier-closeout.md" \
   && grep -q 'cargo fmt --all --check' "${PROOF_DIR}/sse7-verifier-closeout.md" \
   && grep -q 'Result: passed with no formatting diff' "${PROOF_DIR}/sse7-verifier-closeout.md" \
   && grep -q 'cargo check --workspace' "${PROOF_DIR}/sse7-verifier-closeout.md" \
   && grep -q 'Finished `dev` profile' "${PROOF_DIR}/sse7-verifier-closeout.md" \
   && grep -q 'Final verifier after adding the SSE7 gate: 16 passed, 0 failed' "${PROOF_DIR}/sse7-verifier-closeout.md"; then
  pass "Final closeout proof records verifier, formatting, workspace check, and completed extraction decisions"
else
  fail "Final closeout evidence incomplete" "Expected completed SSE7 proof with verifier result, cargo fmt --all --check, cargo check --workspace, and final extraction decision"
fi

printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"

if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
