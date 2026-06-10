#!/usr/bin/env bash
# Completion-gate verifier for
# docs/private/plans/server-crate-extraction-completion-plan.md.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN="docs/private/plans/server-crate-extraction-completion-plan.md"
PROOF_DIR="docs/private/plans/proof/server-crate-extraction-completion"
SCRIPT="scripts/verify-server-crate-extraction-completion.sh"
PREDECESSOR_SCRIPT="scripts/verify-server-seam-extraction-readiness.sh"

PASS=0
FAIL=0
FAIL_DETAIL=()

TARGET_CRATES="
nimbus-artifacts
nimbus-provenance
nimbus-services
nimbus-operator
nimbus-mongodb
nimbus-firebase
nimbus-cloud-functions
nimbus-convex
"

SERVER_ONLY_SHELLS="
route mounting
listener lifecycle
AppState construction
global composition
shutdown signaling
process-backed verifier execution
server-owned adapter transport shells
"

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

proof_status() {
  local proof="$1"
  if [ -f "${proof}" ]; then
    awk -F': ' '/^Status: / { print $2; exit }' "${proof}"
  fi
}

proof_completed() {
  local proof="$1"
  [ "$(proof_status "${proof}")" = "completed" ]
}

all_phases_completed() {
  local phase
  for phase in FCE0 FCE1 FCE2 FCE3 FCE4 FCE5 FCE6 FCE7 FCE8 FCE9 FCE10; do
    [ "$(phase_status "${phase}")" = "completed" ] || return 1
  done
}

metadata_has_crate() {
  local crate="$1"
  grep -q "\"name\":\"${crate}\"" /tmp/nimbus-fce-metadata.json
}

crate_has_no_server_dependency() {
  local crate="$1"
  local output="/tmp/nimbus-fce-${crate}-tree.out"
  local error="/tmp/nimbus-fce-${crate}-tree.err"

  cargo tree -p "${crate}" --edges normal >"${output}" 2>"${error}" || return 1
  ! grep -q 'nimbus-server' "${output}"
}

denied_imports_absent() {
  local crate_path="$1"
  shift

  [ -d "${crate_path}" ] || return 1
  if rg -n "$*" "${crate_path}" -g '*.rs' -g 'Cargo.toml' >/tmp/nimbus-fce-denied-imports.out 2>/tmp/nimbus-fce-denied-imports.err; then
    return 1
  fi
  return 0
}

phase_status() {
  local phase="$1"
  awk -F'|' -v phase="${phase}" '
    $2 ~ phase {
      gsub(/^[ \t]+|[ \t]+$/, "", $3);
      print $3;
      exit;
    }
  ' "${PLAN}"
}

in_progress_count() {
  awk -F'|' '
    $2 ~ /FCE[0-9]/ && $3 ~ /in_progress/ { count++ }
    END { print count + 0 }
  ' "${PLAN}"
}

current_phase() {
  awk -F'|' '
    $2 ~ /FCE[0-9]/ && $3 ~ /in_progress/ {
      gsub(/^[ \t]+|[ \t]+$/, "", $2);
      print $2;
      exit;
    }
  ' "${PLAN}"
}

proof_for_phase() {
  case "$1" in
    FCE0*) printf '%s/fce0-baseline.md\n' "${PROOF_DIR}" ;;
    FCE1*) printf '%s/fce1-artifacts.md\n' "${PROOF_DIR}" ;;
    FCE2*) printf '%s/fce2-provenance.md\n' "${PROOF_DIR}" ;;
    FCE3*) printf '%s/fce3-services.md\n' "${PROOF_DIR}" ;;
    FCE4*) printf '%s/fce4-operator.md\n' "${PROOF_DIR}" ;;
    FCE5*) printf '%s/fce5-mongodb.md\n' "${PROOF_DIR}" ;;
    FCE6*) printf '%s/fce6-firebase.md\n' "${PROOF_DIR}" ;;
    FCE7*) printf '%s/fce7-cloud-functions.md\n' "${PROOF_DIR}" ;;
    FCE8*) printf '%s/fce8-convex.md\n' "${PROOF_DIR}" ;;
    FCE9*) printf '%s/fce9-adapters-facade.md\n' "${PROOF_DIR}" ;;
    FCE10*) printf '%s/fce10-closeout.md\n' "${PROOF_DIR}" ;;
    *) return 1 ;;
  esac
}

printf '\033[1mFCE verification gate - server crate extraction completion\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

cargo metadata --no-deps --format-version 1 >/tmp/nimbus-fce-metadata.json 2>/tmp/nimbus-fce-metadata.err
METADATA_STATUS=$?

step 1 "Control-plane files exist"
if [ -f "${PLAN}" ] \
   && [ -d "${PROOF_DIR}" ] \
   && [ -f "${PROOF_DIR}/fce0-baseline.md" ] \
   && [ -x "${SCRIPT}" ] \
   && [ -x "${PREDECESSOR_SCRIPT}" ]; then
  pass "Plan, proof directory, FCE0 proof, executable verifier, and predecessor verifier exist"
else
  fail "FCE control-plane files incomplete" "Expected plan, proof dir, FCE0 proof, executable ${SCRIPT}, and executable predecessor verifier"
fi

step 2 "Predecessor SSE verifier remains authoritative"
if [ "$(phase_status "FCE0")" = "completed" ] && [ "$(phase_status "FCE1")" = "completed" ]; then
  if grep -q 'Predecessor server seam extraction readiness verifier passed' "${PROOF_DIR}/fce0-baseline.md"; then
    pass "Predecessor SSE verifier passed in FCE0; FCE verifier now owns post-extraction gates"
  else
    fail "FCE0 predecessor evidence missing" "Expected FCE0 proof to record the predecessor SSE verifier pass before extraction began"
  fi
elif "${PREDECESSOR_SCRIPT}" >/tmp/nimbus-fce-predecessor-verifier.out 2>&1; then
  pass "Predecessor server seam extraction readiness verifier passed"
else
  fail "Predecessor SSE verifier failed" "Expected ${PREDECESSOR_SCRIPT} to pass before final extraction"
fi

step 3 "Phase ledger has exactly one active phase"
ACTIVE_COUNT="$(in_progress_count)"
ACTIVE_PHASE="$(current_phase)"
ACTIVE_PROOF="$(proof_for_phase "${ACTIVE_PHASE}" 2>/dev/null || true)"
if [ "${ACTIVE_COUNT}" = "1" ] \
   && [ -n "${ACTIVE_PHASE}" ] \
   && [ -n "${ACTIVE_PROOF}" ] \
   && [ -f "${ACTIVE_PROOF}" ]; then
  pass "Exactly one phase is in_progress: ${ACTIVE_PHASE}"
elif [ "${ACTIVE_COUNT}" = "0" ] \
   && all_phases_completed \
   && proof_completed "${PROOF_DIR}/fce10-closeout.md"; then
  pass "All phases are completed and FCE10 closeout proof is complete"
else
  fail "Phase ledger is not resumable" "Expected exactly one in_progress phase with a matching proof file; got count=${ACTIVE_COUNT}, phase=${ACTIVE_PHASE:-none}"
fi

step 4 "FCE0 proof records target crates and server-only shells"
FCE0_PROOF="${PROOF_DIR}/fce0-baseline.md"
MISSING_FCE0=0
if [ -f "${FCE0_PROOF}" ]; then
  for crate in ${TARGET_CRATES}; do
    if ! grep -q "\`${crate}\`" "${FCE0_PROOF}"; then
      MISSING_FCE0=1
    fi
  done
  while IFS= read -r shell; do
    [ -z "${shell}" ] && continue
    if ! grep -q "${shell}" "${FCE0_PROOF}"; then
      MISSING_FCE0=1
    fi
  done <<EOF_FCE_SHELLS
${SERVER_ONLY_SHELLS}
EOF_FCE_SHELLS
  if grep -q 'FCE-REQ-001' "${FCE0_PROOF}" \
     && grep -q 'FCE-REQ-010' "${FCE0_PROOF}" \
     && grep -q 'FCE1-FCE8 must end in actual extracted crates' "${FCE0_PROOF}" \
     && [ "${MISSING_FCE0}" -eq 0 ]; then
    pass "FCE0 proof records target crates, server-only shells, and requirement coverage"
  else
    fail "FCE0 proof evidence incomplete" "Expected target crates, server-only shells, actual-extraction requirement, and FCE requirement IDs"
  fi
else
  fail "FCE0 proof missing" "Expected ${FCE0_PROOF}"
fi

step 5 "Known architecture crates are present and server-free"
if [ "${METADATA_STATUS}" -eq 0 ] \
   && metadata_has_crate "nimbus-tenant" \
   && metadata_has_crate "nimbus-node" \
   && metadata_has_crate "nimbus-system" \
   && metadata_has_crate "nimbus-bridge" \
   && metadata_has_crate "nimbus-auth" \
   && metadata_has_crate "nimbus-license" \
   && crate_has_no_server_dependency "nimbus-tenant" \
   && crate_has_no_server_dependency "nimbus-node" \
   && crate_has_no_server_dependency "nimbus-system" \
   && crate_has_no_server_dependency "nimbus-bridge" \
   && crate_has_no_server_dependency "nimbus-auth" \
   && crate_has_no_server_dependency "nimbus-license"; then
  pass "Existing authority/system/bridge/license crates are present and do not depend on nimbus-server"
else
  fail "Existing architecture crate baseline failed" "Expected cargo metadata/tree to show nimbus-tenant,node,system,bridge,auth,license present and server-free"
fi

step 6 "Reusable target-crate helper semantics are available"
HELPER_TEXT="$(grep -E '^(crate_has_no_server_dependency|denied_imports_absent|metadata_has_crate|proof_for_phase)\(\)' "${SCRIPT}" || true)"
if printf '%s\n' "${HELPER_TEXT}" | grep -q 'crate_has_no_server_dependency' \
   && printf '%s\n' "${HELPER_TEXT}" | grep -q 'denied_imports_absent' \
   && printf '%s\n' "${HELPER_TEXT}" | grep -q 'metadata_has_crate' \
   && printf '%s\n' "${HELPER_TEXT}" | grep -q 'proof_for_phase'; then
  pass "Verifier includes reusable crate, dependency, denied-import, and proof helpers"
else
  fail "Verifier helper surface incomplete" "Expected reusable helper functions for crate existence, no-server dependency, denied imports, and proof lookup"
fi

step 7 "No premature optional aggregate adapter facade exists"
FCE9_STATUS_FOR_FACADE="$(phase_status "FCE9")"
if [ ! -d "crates/nimbus-adapters" ] \
   && ! metadata_has_crate "nimbus-adapters"; then
  pass "Optional nimbus-adapters facade has not been created before per-adapter crates are clean"
elif { [ "${FCE9_STATUS_FOR_FACADE}" = "in_progress" ] || [ "${FCE9_STATUS_FOR_FACADE}" = "completed" ]; } \
   && [ "$(phase_status "FCE5")" = "completed" ] \
   && [ "$(phase_status "FCE6")" = "completed" ] \
   && [ "$(phase_status "FCE7")" = "completed" ] \
   && [ "$(phase_status "FCE8")" = "completed" ]; then
  pass "Optional nimbus-adapters facade exists only after all per-adapter crates are complete"
else
  fail "Premature nimbus-adapters facade exists" "Expected no aggregate facade until FCE9"
fi

step 8 "Extraction target crates follow the ledger state"
TARGET_STATUS_OK=1
for crate in ${TARGET_CRATES}; do
  case "${crate}" in
    nimbus-artifacts) phase="FCE1" ;;
    nimbus-provenance) phase="FCE2" ;;
    nimbus-services) phase="FCE3" ;;
    nimbus-operator) phase="FCE4" ;;
    nimbus-mongodb) phase="FCE5" ;;
    nimbus-firebase) phase="FCE6" ;;
    nimbus-cloud-functions) phase="FCE7" ;;
    nimbus-convex) phase="FCE8" ;;
    *) phase="" ;;
  esac

  status="$(phase_status "${phase}")"
  if [ "${status}" = "completed" ]; then
    if ! metadata_has_crate "${crate}" || ! crate_has_no_server_dependency "${crate}"; then
      TARGET_STATUS_OK=0
    fi
  fi
done

if [ "${TARGET_STATUS_OK}" -eq 1 ]; then
  pass "No completed extraction phase lacks its target crate/no-server dependency proof"
else
  fail "Completed extraction phase missing crate proof" "A completed FCE1-FCE8 phase must have its target crate in metadata and no nimbus-server dependency"
fi

step 9 "FCE1 nimbus-artifacts extraction is enforced when complete"
FCE1_STATUS="$(phase_status "FCE1")"
FCE1_PROOF="${PROOF_DIR}/fce1-artifacts.md"
if [ "${FCE1_STATUS}" = "completed" ]; then
  if proof_completed "${FCE1_PROOF}" \
     && metadata_has_crate "nimbus-artifacts" \
     && crate_has_no_server_dependency "nimbus-artifacts" \
     && denied_imports_absent "crates/nimbus-artifacts" 'nimbus[-_]server|nimbus[-_]tenant|nimbus[-_]system|nimbus[-_]storage|std::process|Command::new|Stdio|axum' \
     && grep -q '6 passed; 0 failed; 0 ignored' "${FCE1_PROOF}" \
     && grep -q '2 passed; 0 failed; 0 ignored' "${FCE1_PROOF}" \
     && grep -q '37 passed; 0 failed; 0 ignored' "${FCE1_PROOF}" \
     && grep -q 'nimbus-artifacts v' "${FCE1_PROOF}" \
     && rg -n 'use nimbus_artifacts|nimbus_artifacts::' \
       crates/nimbus-server/src/artifact_verifier_effects.rs \
       crates/nimbus-server/src/artifact_verifier_effects \
       crates/nimbus-server/src/execution/invocations/provenance.rs \
       crates/nimbus-cloud-functions/src/registry.rs \
       crates/nimbus-convex/src/registry/loading.rs \
       crates/nimbus-server/src/lib.rs \
       -g '*.rs' >/tmp/nimbus-fce1-server-artifact-imports.out 2>/tmp/nimbus-fce1-server-artifact-imports.err \
     && rg -n 'ArtifactImageVerificationProvider|impl TenantImageVerificationProvider for ArtifactImageVerificationProvider' \
       crates/nimbus-tenant/src/artifact_provenance.rs >/tmp/nimbus-fce1-tenant-adapter.out 2>/tmp/nimbus-fce1-tenant-adapter.err \
     && ! rg -n 'std::process|Command::new|Stdio|ProcessArtifactVerifierCommandRunner' \
       crates/nimbus-artifacts -g '*.rs' -g 'Cargo.toml' >/tmp/nimbus-fce1-process-deny.out 2>/tmp/nimbus-fce1-process-deny.err; then
    pass "nimbus-artifacts is extracted, server-free, process-free, and covered by focused tests"
  else
    fail "FCE1 nimbus-artifacts extraction evidence incomplete" "Expected completed FCE1 proof, nimbus-artifacts crate, no nimbus-server dependency, denied imports absent, focused test counts, server imports from nimbus_artifacts, and tenant-owned image adapter"
  fi
elif [ "${FCE1_STATUS}" = "in_progress" ]; then
  if [ -f "${FCE1_PROOF}" ]; then
    pass "FCE1 is active with proof file present; completion checks will apply when marked completed"
  else
    fail "FCE1 proof missing" "Expected ${FCE1_PROOF} while FCE1 is in_progress"
  fi
else
  pass "FCE1 is ${FCE1_STATUS}; extraction checks are deferred until the phase is active or completed"
fi

step 10 "FCE2 nimbus-provenance extraction is enforced when complete"
FCE2_STATUS="$(phase_status "FCE2")"
FCE2_PROOF="${PROOF_DIR}/fce2-provenance.md"
if [ "${FCE2_STATUS}" = "completed" ]; then
  if proof_completed "${FCE2_PROOF}" \
     && metadata_has_crate "nimbus-provenance" \
     && crate_has_no_server_dependency "nimbus-provenance" \
     && denied_imports_absent "crates/nimbus-provenance" 'nimbus[-_]server|nimbus[-_]runtime|nimbus[-_]storage|std::process|Command::new|Stdio|axum|AppState|RouterBuildConfig|registry/loading|adapters/' \
     && ! rg -n 'nimbus-(artifacts|provenance|server|tenant|system|auth|bridge|node|storage|services)|nimbus_(artifacts|provenance|server|tenant|system|auth|bridge|node|storage|services)' \
       crates/nimbus-runtime/Cargo.toml >/tmp/nimbus-fce2-runtime-workspace-deps.out 2>/tmp/nimbus-fce2-runtime-workspace-deps.err \
     && grep -q '1 passed; 0 failed; 0 ignored' "${FCE2_PROOF}" \
     && grep -q '4 passed; 0 failed; 0 ignored' "${FCE2_PROOF}" \
     && grep -q '2 passed; 0 failed; 0 ignored' "${FCE2_PROOF}" \
     && grep -q 'nimbus-provenance v' "${FCE2_PROOF}" \
     && grep -q 'SLSA/SBOM verifier evidence remains in `nimbus-artifacts`' "${FCE2_PROOF}" \
     && rg -n 'use nimbus_provenance::RuntimeBundleProvenanceConfig' \
       crates/nimbus-server/src/execution/invocations/mod.rs \
       crates/nimbus-server/src/execution/invocations/provenance.rs \
       crates/nimbus-cloud-functions/src/registry.rs \
       crates/nimbus-convex/src/lib.rs \
       >/tmp/nimbus-fce2-server-provenance-imports.out 2>/tmp/nimbus-fce2-server-provenance-imports.err \
     && ! rg -n 'crate::execution::invocations::RuntimeBundleProvenanceConfig|pub\(crate\) use provenance::RuntimeBundleProvenanceConfig|pub\(crate\) use nimbus_provenance::RuntimeBundleProvenanceConfig' \
       crates/nimbus-server/src -g '*.rs' >/tmp/nimbus-fce2-server-reexport-deny.out 2>/tmp/nimbus-fce2-server-reexport-deny.err \
     && ! rg -n 'std::process|Command::new|Stdio|ProcessArtifactVerifierCommandRunner' \
       crates/nimbus-provenance -g '*.rs' -g 'Cargo.toml' >/tmp/nimbus-fce2-process-deny.out 2>/tmp/nimbus-fce2-process-deny.err; then
    pass "nimbus-provenance is extracted, runtime-free, server-free, process-free, and covered by fail-closed tests"
  else
    fail "FCE2 nimbus-provenance extraction evidence incomplete" "Expected completed FCE2 proof, nimbus-provenance crate, no nimbus-server/runtime dependency, denied imports absent, direct server imports from nimbus_provenance, focused test counts, runtime zero-workspace-dep evidence, and no process execution"
  fi
elif [ "${FCE2_STATUS}" = "in_progress" ]; then
  if [ -f "${FCE2_PROOF}" ]; then
    pass "FCE2 is active with proof file present; completion checks will apply when marked completed"
  else
    fail "FCE2 proof missing" "Expected ${FCE2_PROOF} while FCE2 is in_progress"
  fi
else
  pass "FCE2 is ${FCE2_STATUS}; extraction checks are deferred until the phase is active or completed"
fi

step 11 "FCE3 nimbus-services extraction is enforced when complete"
FCE3_STATUS="$(phase_status "FCE3")"
FCE3_PROOF="${PROOF_DIR}/fce3-services.md"
if [ "${FCE3_STATUS}" = "completed" ]; then
  if proof_completed "${FCE3_PROOF}" \
     && metadata_has_crate "nimbus-services" \
     && crate_has_no_server_dependency "nimbus-services" \
     && denied_imports_absent "crates/nimbus-services" 'nimbus[-_]server|axum|RouterBuildConfig|AppState|crate::state|crate::router|crate::system_tenant|nimbus[-_]system|nimbus_system|nimbus_engine|std::process|Command::new|Stdio' \
     && grep -q '22 passed; 0 failed; 0 ignored' "${FCE3_PROOF}" \
     && grep -q 'service_evidence_writer_records_observed_state_to_system_tenant' "${FCE3_PROOF}" \
     && grep -q 'local_admin_service_lifecycle_routes_remain_server_owned' "${FCE3_PROOF}" \
     && grep -q 'nimbus-services v' "${FCE3_PROOF}" \
     && grep -q 'HTTP service lifecycle routes remain in `nimbus-server`' "${FCE3_PROOF}" \
     && rg -n 'use nimbus_services|pub use nimbus_services' crates/nimbus-server/src -g '*.rs' \
       >/tmp/nimbus-fce3-server-service-imports.out 2>/tmp/nimbus-fce3-server-service-imports.err \
     && ! rg -n 'mod sandbox|mod service_registry|crate::sandbox|crate::service_registry|crate::service_manager::SandboxServiceManager' \
       crates/nimbus-server/src -g '*.rs' >/tmp/nimbus-fce3-server-shim-deny.out 2>/tmp/nimbus-fce3-server-shim-deny.err \
     && rg -n 'record_service_handle_async|SystemTenantServiceEvidenceWriter|attach_system_state_service' \
       crates/nimbus-server/src/service_manager.rs >/tmp/nimbus-fce3-server-evidence-writer.out 2>/tmp/nimbus-fce3-server-evidence-writer.err \
     && ! rg -n 'record_service_handle_async|SystemTenantServiceEvidenceWriter|nimbus_system|nimbus_engine' \
       crates/nimbus-services -g '*.rs' -g 'Cargo.toml' >/tmp/nimbus-fce3-services-system-deny.out 2>/tmp/nimbus-fce3-services-system-deny.err; then
    pass "nimbus-services is extracted, server-free, system-persistence-free, and covered by service lifecycle/security tests"
  else
    fail "FCE3 nimbus-services extraction evidence incomplete" "Expected completed FCE3 proof, nimbus-services crate, no nimbus-server dependency, denied imports absent, direct server imports from nimbus_services, service lifecycle/security test counts, and server-owned system evidence writer"
  fi
elif [ "${FCE3_STATUS}" = "in_progress" ]; then
  if [ -f "${FCE3_PROOF}" ]; then
    pass "FCE3 is active with proof file present; completion checks will apply when marked completed"
  else
    fail "FCE3 proof missing" "Expected ${FCE3_PROOF} while FCE3 is in_progress"
  fi
else
  pass "FCE3 is ${FCE3_STATUS}; extraction checks are deferred until the phase is active or completed"
fi

step 12 "FCE4 nimbus-operator extraction is enforced when complete"
FCE4_STATUS="$(phase_status "FCE4")"
FCE4_PROOF="${PROOF_DIR}/fce4-operator.md"
if [ "${FCE4_STATUS}" = "completed" ]; then
  if proof_completed "${FCE4_PROOF}" \
     && metadata_has_crate "nimbus-operator" \
     && crate_has_no_server_dependency "nimbus-operator" \
     && denied_imports_absent "crates/nimbus-operator" 'nimbus[-_]server|axum|RouterBuildConfig|AppState|crate::state|crate::router|crate::local_server|nimbus[-_]engine|nimbus_engine|nimbus[-_]auth|nimbus_auth|nimbus[-_]tenant|nimbus_tenant|adapters/' \
     && grep -q '29 passed; 0 failed; 0 ignored' "${FCE4_PROOF}" \
     && grep -q '12 passed; 0 failed; 0 ignored' "${FCE4_PROOF}" \
     && grep -q 'nimbus-operator v' "${FCE4_PROOF}" \
     && grep -q 'Axum middleware and route mounting remain in `nimbus-server`' "${FCE4_PROOF}" \
     && rg -n 'use nimbus_operator|pub use nimbus_operator' crates/nimbus-server/src -g '*.rs' \
       >/tmp/nimbus-fce4-server-operator-imports.out 2>/tmp/nimbus-fce4-server-operator-imports.err \
     && [ ! -f crates/nimbus-server/src/local_server/access.rs ] \
     && [ ! -f crates/nimbus-server/src/local_server/access_policy.rs ] \
     && [ ! -f crates/nimbus-server/src/local_server/audit.rs ] \
     && [ ! -f crates/nimbus-server/src/local_server/paths.rs ] \
     && [ ! -f crates/nimbus-server/src/local_server/policy.rs ] \
     && [ ! -f crates/nimbus-server/src/local_server/token.rs ]; then
    pass "nimbus-operator is extracted, server-free, auth-separated, and covered by operator security tests"
  else
    fail "FCE4 nimbus-operator extraction evidence incomplete" "Expected completed FCE4 proof, nimbus-operator crate, no nimbus-server dependency, denied imports absent, local/deploy operator test counts, direct server imports from nimbus_operator, and server-retained middleware/route shell"
  fi
elif [ "${FCE4_STATUS}" = "in_progress" ]; then
  if [ -f "${FCE4_PROOF}" ]; then
    pass "FCE4 is active with proof file present; completion checks will apply when marked completed"
  else
    fail "FCE4 proof missing" "Expected ${FCE4_PROOF} while FCE4 is in_progress"
  fi
else
  pass "FCE4 is ${FCE4_STATUS}; extraction checks are deferred until the phase is active or completed"
fi

step 13 "FCE5 nimbus-mongodb extraction is enforced when complete"
FCE5_STATUS="$(phase_status "FCE5")"
FCE5_PROOF="${PROOF_DIR}/fce5-mongodb.md"
if [ "${FCE5_STATUS}" = "completed" ]; then
  if proof_completed "${FCE5_PROOF}" \
     && metadata_has_crate "nimbus-mongodb" \
     && crate_has_no_server_dependency "nimbus-mongodb" \
     && denied_imports_absent "crates/nimbus-mongodb" 'nimbus[-_]server|AppState|RouterBuildConfig|crate::state|crate::router|crate::system_tenant|system_tenant|local_server|axum|TcpListener|TcpStream|tokio::net|route\(|Router<|State<|Extension<' \
     && grep -q '263 passed; 0 failed; 0 ignored' "${FCE5_PROOF}" \
     && grep -q '5 passed; 0 failed; 0 ignored' "${FCE5_PROOF}" \
     && grep -q '23 passed; 0 failed; 0 ignored' "${FCE5_PROOF}" \
     && grep -q 'nimbus-mongodb v' "${FCE5_PROOF}" \
     && grep -q 'TCP listener lifecycle remains in `nimbus-server`' "${FCE5_PROOF}" \
     && rg -n 'use nimbus_mongodb::|pub use nimbus_mongodb' crates/nimbus-server/src/adapters/mongodb crates/nimbus-server/src/lib.rs -g '*.rs' \
       >/tmp/nimbus-fce5-server-mongodb-imports.out 2>/tmp/nimbus-fce5-server-mongodb-imports.err \
     && rg -n 'TcpListener|run_listener_with_auth|tokio::spawn' crates/nimbus-server/src/adapters/mongodb/listener.rs crates/nimbus-server/src/construction.rs \
       >/tmp/nimbus-fce5-server-listener-shell.out 2>/tmp/nimbus-fce5-server-listener-shell.err \
     && [ ! -f crates/nimbus-server/src/adapters/mongodb/auth.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/mongodb/bson_bridge.rs ] \
     && [ ! -d crates/nimbus-server/src/adapters/mongodb/commands ] \
     && [ ! -f crates/nimbus-server/src/adapters/mongodb/connection.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/mongodb/error.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/mongodb/wire.rs ] \
     && [ -f crates/nimbus-server/src/adapters/mongodb/listener.rs ] \
     && [ -f crates/nimbus-server/src/adapters/mongodb/mod.rs ]; then
    pass "nimbus-mongodb is extracted, server-free, listener-free, and covered by adapter plus server integration tests"
  else
    fail "FCE5 nimbus-mongodb extraction evidence incomplete" "Expected completed FCE5 proof, nimbus-mongodb crate, no nimbus-server dependency, denied imports absent, adapter/server/spec test counts, direct server imports from nimbus_mongodb, and retained server listener shell"
  fi
elif [ "${FCE5_STATUS}" = "in_progress" ]; then
  if [ -f "${FCE5_PROOF}" ]; then
    pass "FCE5 is active with proof file present; completion checks will apply when marked completed"
  else
    fail "FCE5 proof missing" "Expected ${FCE5_PROOF} while FCE5 is in_progress"
  fi
else
  pass "FCE5 is ${FCE5_STATUS}; extraction checks are deferred until the phase is active or completed"
fi

step 14 "FCE6 nimbus-firebase extraction is enforced when complete"
FCE6_STATUS="$(phase_status "FCE6")"
FCE6_PROOF="${PROOF_DIR}/fce6-firebase.md"
if [ "${FCE6_STATUS}" = "completed" ]; then
  if proof_completed "${FCE6_PROOF}" \
     && metadata_has_crate "nimbus-firebase" \
     && crate_has_no_server_dependency "nimbus-firebase" \
     && denied_imports_absent "crates/nimbus-firebase" 'nimbus[-_]server|AppState|RouterBuildConfig|crate::state|crate::router|crate::system_tenant|system_tenant|local_server|crate::application_auth|resolve_application_auth|record_authenticated_usage|nimbus[-_]auth|nimbus_auth|WebSocket|WebSocketUpgrade|Router<|route\(|State<|Extension<' \
     && grep -q '42 passed; 0 failed; 0 ignored' "${FCE6_PROOF}" \
     && grep -q '98 passed; 0 failed; 0 ignored' "${FCE6_PROOF}" \
     && grep -q 'nimbus-firebase v' "${FCE6_PROOF}" \
     && grep -q 'Firestore proto tree and generated tonic types moved to `nimbus-firebase`' "${FCE6_PROOF}" \
     && grep -q 'tonic service construction, WebSocket upgrade, auth resolution, and usage recording remain in `nimbus-server`' "${FCE6_PROOF}" \
     && rg -n 'nimbus_firebase::grpc|pub use nimbus_firebase::FirebaseConfig|use nimbus_firebase|pub\(crate\) use nimbus_firebase' \
       crates/nimbus-server/src/adapters/firebase crates/nimbus-server/src/lib.rs -g '*.rs' \
       >/tmp/nimbus-fce6-server-firebase-imports.out 2>/tmp/nimbus-fce6-server-firebase-imports.err \
     && rg -n 'tonic_build|protoc_bin_vendored|include_file\("firebase_grpc.rs"\)|compile_protos' \
       crates/nimbus-firebase/build.rs >/tmp/nimbus-fce6-adapter-proto-build.out 2>/tmp/nimbus-fce6-adapter-proto-build.err \
     && ! rg -n 'tonic_build|protoc_bin_vendored|include_file\("firebase_grpc.rs"\)|compile_protos' \
       crates/nimbus-server/build.rs crates/nimbus-server/Cargo.toml >/tmp/nimbus-fce6-server-proto-build-deny.out 2>/tmp/nimbus-fce6-server-proto-build-deny.err \
     && [ -d crates/nimbus-firebase/proto/google/firestore/v1 ] \
     && [ ! -d crates/nimbus-server/proto ] \
     && rg -n 'pub async fn handle_commit|pub fn write_response_stream|pub fn listen_response_stream|struct ActiveWriteRequestStream|struct ActiveListenRequestStream|struct RetainedListenTargetKey' \
       crates/nimbus-firebase/src/grpc -g '*.rs' >/tmp/nimbus-fce6-adapter-grpc-core.out 2>/tmp/nimbus-fce6-adapter-grpc-core.err \
     && ! rg -n 'tenant_context_for_database|lower_write_batch|ActiveWriteRequestStream|ActiveListenRequestStream|RetainedListenTargetKey|decode_nimbus_value_from_grpc|proto_document|lower_structured_query' \
       crates/nimbus-server/src/adapters/firebase/grpc/unary.rs \
       crates/nimbus-server/src/adapters/firebase/grpc/write_stream.rs \
       crates/nimbus-server/src/adapters/firebase/grpc/listen_stream.rs \
       >/tmp/nimbus-fce6-server-grpc-core-deny.out 2>/tmp/nimbus-fce6-server-grpc-core-deny.err \
     && rg -n 'resolve_bearer_auth|record_authenticated_usage|FirestoreServer::new|WebSocketUpgrade|listen_websocket' \
       crates/nimbus-server/src/adapters/firebase/grpc crates/nimbus-server/src/adapters/firebase/mod.rs \
       >/tmp/nimbus-fce6-server-transport-shell.out 2>/tmp/nimbus-fce6-server-transport-shell.err; then
    pass "nimbus-firebase is extracted, server-free, proto-owning, and covered by adapter plus server Firebase tests"
  else
    fail "FCE6 nimbus-firebase extraction evidence incomplete" "Expected completed FCE6 proof, nimbus-firebase crate, no nimbus-server dependency, denied imports absent, adapter/server Firebase test counts, proto ownership in adapter, gRPC core in adapter, and retained server transport/auth shell"
  fi
elif [ "${FCE6_STATUS}" = "in_progress" ]; then
  if [ -f "${FCE6_PROOF}" ]; then
    pass "FCE6 is active with proof file present; completion checks will apply when marked completed"
  else
    fail "FCE6 proof missing" "Expected ${FCE6_PROOF} while FCE6 is in_progress"
  fi
else
  pass "FCE6 is ${FCE6_STATUS}; extraction checks are deferred until the phase is active or completed"
fi

step 15 "FCE7 nimbus-cloud-functions extraction is enforced when complete"
FCE7_STATUS="$(phase_status "FCE7")"
FCE7_PROOF="${PROOF_DIR}/fce7-cloud-functions.md"
if [ "${FCE7_STATUS}" = "completed" ]; then
  if proof_completed "${FCE7_PROOF}" \
     && metadata_has_crate "nimbus-cloud-functions" \
     && crate_has_no_server_dependency "nimbus-cloud-functions" \
     && denied_imports_absent "crates/nimbus-cloud-functions" 'nimbus[-_]server|AppState|RouterBuildConfig|crate::state|crate::router|crate::system_tenant|system_tenant|std::process|Command::new|Stdio|axum|WebSocket|WebSocketUpgrade|route\(|Router<|State<|Extension<|crate::execution|artifact_verifier_effects' \
     && grep -q '`cargo test -p nimbus-cloud-functions -- --nocapture`: 20 passed; 0 failed; 0 ignored' "${FCE7_PROOF}" \
     && grep -q '`cargo test -p nimbus-server cloud_functions -- --nocapture`: 20 passed; 0 failed; 0 ignored' "${FCE7_PROOF}" \
     && grep -q 'nimbus-cloud-functions v' "${FCE7_PROOF}" \
     && grep -q 'Cloud Functions app contract, manifests, target binding, registry, runtime API, host bridge, trigger executor, and neutral HTTP request/response shaping moved to `nimbus-cloud-functions`' "${FCE7_PROOF}" \
     && grep -q 'Axum route mounting, active deployment lookup, callable auth/usage, deploy activation, and process-backed codegen fixtures remain in `nimbus-server`' "${FCE7_PROOF}" \
     && rg -n 'CloudFunctionsHostBridge|CloudFunctionsRuntimeInvoker|CloudFunctionsRuntimeInvocation|CloudFunctionsTriggerExecutor|build_http_request_args|build_callable_request_args|build_http_response_parts|CloudFunctionsRegistry|dispatch_runtime_extension_call' \
       crates/nimbus-cloud-functions/src -g '*.rs' >/tmp/nimbus-fce7-adapter-core.out 2>/tmp/nimbus-fce7-adapter-core.err \
     && rg -n 'ServerCloudFunctionsRuntimeInvoker|impl CloudFunctionsRuntimeInvoker for ServerCloudFunctionsRuntimeInvoker|with_runtime_bundle_provenance_gate|execute_adapter_http_target|build_callable_request_args|build_http_request_args|record_authenticated_usage|verify_optional_application_auth_from_headers_in_deployment' \
       crates/nimbus-server/src/adapters/cloud_functions crates/nimbus-server/src/state.rs >/tmp/nimbus-fce7-server-shell.out 2>/tmp/nimbus-fce7-server-shell.err \
     && rg -n 'cloud_functions_trigger_executor_fails_closed_when_runtime_bundle_provenance_is_rejected|runtime bundle provenance admission failed|provenance rejection must happen before runtime side effects' \
       crates/nimbus-server/src/adapters/cloud_functions/execution.rs >/tmp/nimbus-fce7-provenance-test.out 2>/tmp/nimbus-fce7-provenance-test.err \
     && rg -n 'pub mod host_calls|nimbus_bridge::host_calls' \
       crates/nimbus-bridge/src/lib.rs crates/nimbus-cloud-functions/src/host_bridge.rs crates/nimbus-server/src/adapters/convex/host_bridge/async_bridge/mod.rs \
       >/tmp/nimbus-fce7-host-calls.out 2>/tmp/nimbus-fce7-host-calls.err \
     && [ -f crates/nimbus-server/src/adapters/cloud_functions/mod.rs ] \
     && [ -f crates/nimbus-server/src/adapters/cloud_functions/http.rs ] \
     && [ -f crates/nimbus-server/src/adapters/cloud_functions/http/callable.rs ] \
     && [ -f crates/nimbus-server/src/adapters/cloud_functions/http/tenant.rs ] \
     && [ -f crates/nimbus-server/src/adapters/cloud_functions/http/response.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/cloud_functions/app_contract.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/cloud_functions/registry.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/cloud_functions/host_bridge.rs ] \
     && [ ! -d crates/nimbus-server/src/adapters/cloud_functions/runtime_api ] \
     && [ ! -f crates/nimbus-server/src/adapters/cloud_functions/http/request.rs ] \
     && ! rg -n 'crate::execution::host_calls|mod host_calls|pub\(crate\) mod host_calls' \
       crates/nimbus-server/src -g '*.rs' >/tmp/nimbus-fce7-server-host-calls-deny.out 2>/tmp/nimbus-fce7-server-host-calls-deny.err; then
    pass "nimbus-cloud-functions is extracted, server-free, runtime-bridge-owning, and covered by adapter plus Cloud Functions security tests"
  else
    fail "FCE7 nimbus-cloud-functions extraction evidence incomplete" "Expected completed FCE7 proof, nimbus-cloud-functions crate, no nimbus-server dependency, denied imports absent, adapter/server Cloud Functions test counts, runtime bridge and neutral HTTP shaping in adapter, server-retained transport/auth/deploy/codegen shell, host-call helpers in nimbus-bridge, and provenance fail-closed coverage"
  fi
elif [ "${FCE7_STATUS}" = "in_progress" ]; then
  if [ -f "${FCE7_PROOF}" ]; then
    pass "FCE7 is active with proof file present; completion checks will apply when marked completed"
  else
    fail "FCE7 proof missing" "Expected ${FCE7_PROOF} while FCE7 is in_progress"
  fi
else
  pass "FCE7 is ${FCE7_STATUS}; extraction checks are deferred until the phase is active or completed"
fi

step 16 "FCE8 nimbus-convex extraction is enforced when complete"
FCE8_STATUS="$(phase_status "FCE8")"
FCE8_PROOF="${PROOF_DIR}/fce8-convex.md"
if [ "${FCE8_STATUS}" = "completed" ]; then
  if proof_completed "${FCE8_PROOF}" \
     && metadata_has_crate "nimbus-convex" \
     && crate_has_no_server_dependency "nimbus-convex" \
     && denied_imports_absent "crates/nimbus-convex" 'nimbus[-_]server|AppState|RouterBuildConfig|crate::state|crate::router|crate::local_server|crate::system_tenant|crate::application_auth|crate::execution|axum|WebSocket|WebSocketUpgrade|Router<|State<|Extension<|record_deployment_state_async|upsert_system_document|SystemDeploymentRecordInput|std::process|process::Command|Command::new' \
     && grep -q '`cargo test -p nimbus-convex -- --nocapture`: 6 passed; 0 failed; 0 ignored' "${FCE8_PROOF}" \
     && grep -q '`cargo test -p nimbus-server convex -- --nocapture`' "${FCE8_PROOF}" \
     && grep -q 'lib target: 126 passed; 0 failed; 5 ignored' "${FCE8_PROOF}" \
     && grep -q 'reactive_loop target: 18 passed; 0 failed; 0 ignored' "${FCE8_PROOF}" \
     && grep -q 'nimbus-convex v' "${FCE8_PROOF}" \
     && grep -q 'Convex registry, auth verifier, manifest/schema parsing, request models, document identity, host-call contract/envelopes, neutral HTTP templates, and subscription transform models moved to `nimbus-convex`' "${FCE8_PROOF}" \
     && grep -q 'Axum handlers, WebSocket session lifecycle, concrete runtime invocation, request cancellation, local operator audit, and `_nimbus` deployment persistence remain in `nimbus-server`' "${FCE8_PROOF}" \
     && rg -n 'pub struct ConvexRegistry|struct ConvexAuthVerifier|pub enum ConvexHostCallOperation|pub struct ConvexHostCallRequest|pub enum ConvexSubscriptionTransform|pub struct ConvexHttpRequestContext|pub enum ConvexRuntimeResponseEnvelope|pub fn subscription_plan_for_named_query|pub fn resolve_http_template|pub fn encode_convex_document_id' \
       crates/nimbus-convex/src -g '*.rs' >/tmp/nimbus-fce8-adapter-core.out 2>/tmp/nimbus-fce8-adapter-core.err \
     && rg -n 'pub use nimbus_convex::ConvexRegistry|pub\(crate\) use nimbus_convex::\*|convex_system_deployment_record_input|ConvexHttpRouteRequest|WebSocketUpgrade|handle_convex_socket_for_tenant|RuntimeInvocationContext|record_authenticated_usage|authorize_local_server_request' \
       crates/nimbus-server/src/adapters/convex crates/nimbus-server/src/router.rs crates/nimbus-server/src/http/deploy.rs -g '*.rs' \
       >/tmp/nimbus-fce8-server-shell.out 2>/tmp/nimbus-fce8-server-shell.err \
     && rg -n 'runtime_host_bridge_rejects_wrong_table_convex_document_ids|host_bridge_service_lookup_rejects_service_missing_from_decision_grants|convex_route_rejects_application_bearer_for_different_tenant|system_tenant_convex_routes_require_local_admin_auth_when_configured|production_convex_node_runtime_rejects_loopback_network_grants_before_invocation' \
       crates/nimbus-server/src/adapters/convex/tests crates/nimbus-server/src/tests -g '*.rs' \
       >/tmp/nimbus-fce8-security-tests.out 2>/tmp/nimbus-fce8-security-tests.err \
     && [ -f crates/nimbus-server/src/adapters/convex/mod.rs ] \
     && [ -f crates/nimbus-server/src/adapters/convex/handlers/mod.rs ] \
     && [ -f crates/nimbus-server/src/adapters/convex/subscriptions/socket/mod.rs ] \
     && [ -f crates/nimbus-server/src/adapters/convex/execution/runtime_backed/invoke/mod.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/auth/mod.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/document_identity.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/manifest.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/registry/mod.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/requests/mod.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/templates/mod.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/host_bridge/contract.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/host_bridge/payloads/mod.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/host_bridge/responses.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/host_bridge/pagination.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/subscriptions/types.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/subscriptions/transforms/planner.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/subscriptions/transforms/bounds.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/subscriptions/transforms/state.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/subscriptions/transforms/runtime_backed/builtins.rs ] \
     && [ ! -f crates/nimbus-server/src/adapters/convex/subscriptions/transforms/runtime_backed/selection.rs ]; then
    pass "nimbus-convex is extracted, server-free, protocol/auth/registry-owning, and covered by Convex security plus reactive-loop tests"
  else
    fail "FCE8 nimbus-convex extraction evidence incomplete" "Expected completed FCE8 proof, nimbus-convex crate, no nimbus-server dependency, denied imports absent, adapter/server Convex test counts, reactive-loop counts, moved Convex owner modules, retained server route/WebSocket/runtime/system shells, security test coverage, and old server-owned copies removed"
  fi
elif [ "${FCE8_STATUS}" = "in_progress" ]; then
  if [ -f "${FCE8_PROOF}" ]; then
    pass "FCE8 is active with proof file present; completion checks will apply when marked completed"
  else
    fail "FCE8 proof missing" "Expected ${FCE8_PROOF} while FCE8 is in_progress"
  fi
else
  pass "FCE8 is ${FCE8_STATUS}; extraction checks are deferred until the phase is active or completed"
fi

step 17 "FCE9 optional nimbus-adapters facade is disciplined"
FCE9_STATUS="$(phase_status "FCE9")"
FCE9_PROOF="${PROOF_DIR}/fce9-adapters-facade.md"
if [ "${FCE9_STATUS}" = "completed" ]; then
  if proof_completed "${FCE9_PROOF}" \
     && metadata_has_crate "nimbus-adapters" \
     && crate_has_no_server_dependency "nimbus-adapters" \
     && denied_imports_absent "crates/nimbus-adapters" 'nimbus[-_]server|AppState|RouterBuildConfig|crate::state|crate::router|crate::local_server|crate::system_tenant|crate::application_auth|crate::execution|axum|tower|tonic|WebSocket|WebSocketUpgrade|listener|shutdown|record_deployment_state_async|upsert_system_document|SystemDeploymentRecordInput|_nimbus|std::process|process::Command|Command::new' \
     && grep -q '`cargo check -p nimbus-adapters`: passed' "${FCE9_PROOF}" \
     && grep -q '`cargo test -p nimbus-adapters -- --nocapture`: 0 passed; 0 failed; 0 ignored' "${FCE9_PROOF}" \
     && grep -q 'nimbus-adapters v' "${FCE9_PROOF}" \
     && grep -q 're-export-only facade' "${FCE9_PROOF}" \
     && grep -q 'default = \[\]' crates/nimbus-adapters/Cargo.toml \
     && grep -q 'cloud-functions = \["dep:nimbus-cloud-functions"\]' crates/nimbus-adapters/Cargo.toml \
     && grep -q 'convex = \["dep:nimbus-convex"\]' crates/nimbus-adapters/Cargo.toml \
     && grep -q 'firebase = \["dep:nimbus-firebase"\]' crates/nimbus-adapters/Cargo.toml \
     && grep -q 'mongodb = \["dep:nimbus-mongodb"\]' crates/nimbus-adapters/Cargo.toml \
     && rg -n 'pub mod cloud_functions|pub mod convex|pub mod firebase|pub mod mongodb|pub use nimbus_cloud_functions::\*|pub use nimbus_convex::\*|pub use nimbus_firebase::\*|pub use nimbus_mongodb::\*' \
       crates/nimbus-adapters/src/lib.rs >/tmp/nimbus-fce9-facade-exports.out 2>/tmp/nimbus-fce9-facade-exports.err \
     && ! rg -n '(^|[[:space:]])(fn|struct|enum|trait|impl)[[:space:]]|macro_rules!|tokio::|std::process|process::Command|Command::new|axum|AppState|RouterBuildConfig|WebSocket|listener|shutdown|record_deployment_state_async|upsert_system_document|SystemDeploymentRecordInput|_nimbus' \
       crates/nimbus-adapters/src crates/nimbus-adapters/Cargo.toml >/tmp/nimbus-fce9-facade-logic-deny.out 2>/tmp/nimbus-fce9-facade-logic-deny.err; then
    pass "nimbus-adapters is a feature-gated re-export facade with no server/effect/composition logic"
  else
    fail "FCE9 nimbus-adapters facade evidence incomplete" "Expected completed FCE9 proof, nimbus-adapters crate, no nimbus-server dependency, denied imports absent, feature-gated adapter re-exports, no implementation logic, cargo check/test counts, and cargo tree evidence"
  fi
elif [ "${FCE9_STATUS}" = "in_progress" ]; then
  if [ -f "${FCE9_PROOF}" ]; then
    pass "FCE9 is active with proof file present; completion checks will apply when marked completed"
  else
    fail "FCE9 proof missing" "Expected ${FCE9_PROOF} while FCE9 is in_progress"
  fi
else
  pass "FCE9 is ${FCE9_STATUS}; facade checks are deferred until the phase is active or completed"
fi

step 18 "FCE10 final closeout is complete when ledger is complete"
FCE10_STATUS="$(phase_status "FCE10")"
FCE10_PROOF="${PROOF_DIR}/fce10-closeout.md"
if [ "${FCE10_STATUS}" = "completed" ]; then
  if proof_completed "${FCE10_PROOF}" \
     && all_phases_completed \
     && grep -q 'Final verifier result: 18 passed; 0 failed' "${FCE10_PROOF}" \
     && grep -q '`cargo fmt --all --check`: passed' "${FCE10_PROOF}" \
     && grep -q '`CARGO_INCREMENTAL=0 cargo check --workspace`: passed' "${FCE10_PROOF}" \
     && grep -q '`CARGO_INCREMENTAL=0 cargo test -p nimbus-artifacts -p nimbus-provenance -p nimbus-services -p nimbus-operator -p nimbus-mongodb -p nimbus-firebase -p nimbus-cloud-functions -p nimbus-convex -p nimbus-system -p nimbus-adapters`: passed' "${FCE10_PROOF}" \
     && grep -q 'nimbus-artifacts: 6 passed; 0 failed; 0 ignored' "${FCE10_PROOF}" \
     && grep -q 'nimbus-provenance: 1 passed; 0 failed; 0 ignored' "${FCE10_PROOF}" \
     && grep -q 'nimbus-services: 22 passed; 0 failed; 0 ignored' "${FCE10_PROOF}" \
     && grep -q 'nimbus-operator: 29 passed; 0 failed; 0 ignored' "${FCE10_PROOF}" \
     && grep -q 'nimbus-mongodb: 263 passed; 0 failed; 0 ignored' "${FCE10_PROOF}" \
     && grep -q 'nimbus-firebase: 42 passed; 0 failed; 0 ignored' "${FCE10_PROOF}" \
     && grep -q 'nimbus-cloud-functions: 20 passed; 0 failed; 0 ignored' "${FCE10_PROOF}" \
     && grep -q 'nimbus-convex: 6 passed; 0 failed; 0 ignored' "${FCE10_PROOF}" \
     && grep -q 'nimbus-system: 8 passed; 0 failed; 0 ignored' "${FCE10_PROOF}" \
     && grep -q 'nimbus-adapters: 0 passed; 0 failed; 0 ignored' "${FCE10_PROOF}" \
     && grep -q 'Authority flow' "${FCE10_PROOF}" \
     && grep -q 'Side-effect ownership' "${FCE10_PROOF}" \
     && grep -q 'Dependency direction' "${FCE10_PROOF}" \
     && grep -q 'No required phase is blocked' "${FCE10_PROOF}" \
     && grep -q 'convex_runtime_query_starts_real_krun_service_under_manager_and_tears_it_down' "${FCE10_PROOF}" \
     && grep -q 'verification_harness_required_generated_history_seed_corpus_matches_model_on_convex_demo_surface' "${FCE10_PROOF}" \
     && grep -q 'nimbus-server v' "${FCE10_PROOF}" \
     && grep -q 'nimbus-adapters v' "${FCE10_PROOF}"; then
    pass "FCE10 records final verifier, focused tests, formatting, workspace check, ignored-test reasons, and enterprise-trust review"
  else
    fail "FCE10 closeout evidence incomplete" "Expected completed FCE10 proof, all phases completed, final verifier result, moved-crate focused test counts, cargo fmt/check evidence, ignored-test reasons, dependency graph evidence, and enterprise-trust review"
  fi
elif [ "${FCE10_STATUS}" = "in_progress" ]; then
  if [ -f "${FCE10_PROOF}" ]; then
    pass "FCE10 is active with proof file present; completion checks will apply when marked completed"
  else
    fail "FCE10 proof missing" "Expected ${FCE10_PROOF} while FCE10 is in_progress"
  fi
else
  pass "FCE10 is ${FCE10_STATUS}; closeout checks are deferred until the phase is active or completed"
fi

printf '\n\033[1mSummary\033[0m: %s passed, %s failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf ' - %s\n' "${detail}"
  done
  exit 1
fi

exit 0
