#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Launch Readiness plan
# (`docs/private/plans/launch-readiness-plan.md`, LR0..LR13).
#
# Exits 0 iff every condition in the plan's Verification section is satisfied.
# Ships in LR0 so /goal is verifiable from day one; LR1..LR12 progressively
# flip conditions from FAIL to PASS.
#
# This script is also the CONTRACT for canonical test names: where a condition
# requires a named test, the implementing phase must use the exact name below.
#   LR2:  deploy_round_trip_with_local_admin_token
#         deploy_rejects_missing_admin_token
#   LR4:  public_bind_requires_explicit_rotation
#         public_bind_restart_does_not_retrip_freshness
#   LR7:  rest_client_route_parity            (JS test, packages/nimbus)
#   LR9:  backup_restore_round_trip
#   LR12: node_run_converges_transient_unit   (Linux-gated)
#
# Checks are static/cheap (greps, file existence) plus `cargo fmt --check`.
# Heavy gates (make check/clippy/test, npm test) live in phase completion
# evidence and CI. Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PROOF_DIR="docs/private/plans/proof/launch-readiness"
START_DIR="crates/nimbus-bin/src/start"
BIN_SRC="crates/nimbus-bin/src"
SERVER_SRC="crates/nimbus-server/src"
ROUTER="crates/nimbus-server/src/router.rs"
DEPLOY_RS="crates/nimbus-bin/src/deploy.rs"
REST_TS="packages/nimbus/src/rest.ts"
PKG_SCRIPT="scripts/build-linux-release-packages.sh"
UNIT_FILE="packaging/systemd/nimbus.service"
API_KEY_DECISION="docs/private/decisions/api-key-credential.md"

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf 'PASS  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  FAIL_DETAIL+=("$1: $2")
  printf 'FAIL  %s\n      %s\n' "$1" "$2"
}

# Search helper: fixed-string grep across workspace Rust crates.
crates_grep() {
  grep -rq --include='*.rs' "$1" crates/ 2>/dev/null
}

# --- 1. plan registration (LR0) ----------------------------------------------
C="1. plan registered: AGENTS.md mentions launch-readiness; proof dir exists"
if grep -q 'launch-readiness' AGENTS.md 2>/dev/null \
  && [[ -d "${PROOF_DIR}" ]] \
  && [[ -f "${PROOF_DIR}/README.md" ]]; then
  pass "${C}"
else
  fail "${C}" "missing AGENTS.md entry, ${PROOF_DIR}/, or its README.md"
fi

# --- 2. boot.rs naming cleanup (LR1) ------------------------------------------
C="2. no 'Service initialization' comments or 'let service' bindings under ${START_DIR}/"
if [[ -d "${START_DIR}" ]] \
  && ! grep -rq 'Service initialization' "${START_DIR}" 2>/dev/null \
  && ! grep -rqE '\blet (mut )?(shutdown_)?service\b' "${START_DIR}" 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "stale Service->Engine naming residue remains in ${START_DIR}/"
fi

# --- 3. deploy admin handshake (LR2) ------------------------------------------
C="3. nimbus deploy sends the admin-token header; round-trip + rejection tests exist"
if [[ -f "${DEPLOY_RS}" ]] \
  && grep -qEi 'LOCAL_ADMIN_HEADER_NAME|x-nimbus-admin-token' "${DEPLOY_RS}" \
  && crates_grep 'deploy_round_trip_with_local_admin_token' \
  && crates_grep 'deploy_rejects_missing_admin_token'; then
  pass "${C}"
else
  fail "${C}" "deploy.rs lacks the admin header or the canonical LR2 tests are missing"
fi

# --- 4. X-Nimbus-Api-Key decision (LR3) ----------------------------------------
C="4. api-key decision recorded; header either server-handled or gone from packages/"
server_handles_key=false
if grep -rqi 'x-nimbus-api-key' crates/ 2>/dev/null; then
  server_handles_key=true
fi
packages_send_key=false
if grep -rqi 'x-nimbus-api-key' packages/ 2>/dev/null; then
  packages_send_key=true
fi
if [[ -f "${API_KEY_DECISION}" ]] \
  && { [[ "${server_handles_key}" == true ]] || [[ "${packages_send_key}" == false ]]; }; then
  pass "${C}"
else
  fail "${C}" "missing ${API_KEY_DECISION}, or packages/ still send a header no server code reads"
fi

# --- 5. token freshness semantics (LR4) ----------------------------------------
C="5. never-rotated public-bind refusal + restart-no-retrip tests exist"
if crates_grep 'public_bind_requires_explicit_rotation' \
  && crates_grep 'public_bind_restart_does_not_retrip_freshness'; then
  pass "${C}"
else
  fail "${C}" "canonical LR4 freshness tests not found under crates/"
fi

# --- 6. configurable CORS (LR5) --------------------------------------------------
C="6. --cors-allow-origin on nimbus start; build_cors_layer accepts configured origins"
if grep -rqE 'cors[-_]allow[-_]origin' "${BIN_SRC}" 2>/dev/null \
  && grep -qE 'fn build_cors_layer\s*\([^)]' "${ROUTER}" 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "CLI flag or parameterized build_cors_layer missing"
fi

# --- 7. adapter CLI wiring (LR6) --------------------------------------------------
C="7. firestore/mongodb/dynamodb enablement on nimbus start; README caveats dropped"
if grep -rqi 'firestore' "${START_DIR}" 2>/dev/null \
  && grep -rqE 'mongodb[-_](port|host)' "${BIN_SRC}" 2>/dev/null \
  && grep -rqE 'dynamodb[-_](port|host)' "${BIN_SRC}" 2>/dev/null \
  && ! grep -qE 'no CLI flag|embedding API, not a CLI' README.md 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "adapter flags missing from the start surface or README still carries the caveats"
fi

# --- 8. rest.ts parity (LR7) -------------------------------------------------------
C="8. rest.ts covers paginated query + journal routes; route-parity test exists"
if [[ -f "${REST_TS}" ]] \
  && grep -q 'query/paginated' "${REST_TS}" \
  && grep -q '/journal' "${REST_TS}" \
  && grep -rq 'rest_client_route_parity' packages/nimbus/ 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "rest.ts misses server routes or the rest_client_route_parity guard is absent"
fi

# --- 9. TLS termination (LR8) -------------------------------------------------------
C="9. --tls-cert/--tls-key on nimbus start; rustls dependency present"
if grep -rqE 'tls[-_]cert' "${BIN_SRC}" 2>/dev/null \
  && grep -rqE 'tls[-_]key' "${BIN_SRC}" 2>/dev/null \
  && grep -qE 'rustls' crates/nimbus-server/Cargo.toml crates/nimbus-bin/Cargo.toml 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "TLS flags or rustls dependency missing"
fi

# --- 10. backup command (LR9) ---------------------------------------------------------
C="10. nimbus backup subcommand exists; backup_restore_round_trip test exists"
if grep -rq 'mod backup' "${BIN_SRC}" 2>/dev/null \
  && crates_grep 'backup_restore_round_trip'; then
  pass "${C}"
else
  fail "${C}" "backup module or canonical round-trip test missing"
fi

# --- 11. systemd unit shipped (LR10) ----------------------------------------------------
C="11. ${UNIT_FILE} exists and is shipped by the nFPM packaging script"
if [[ -f "${UNIT_FILE}" ]] \
  && grep -q 'nimbus.service' "${PKG_SCRIPT}" 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "unit file missing or not referenced by ${PKG_SCRIPT}"
fi

# --- 12. distribution truth-up (LR11) ----------------------------------------------------
C="12. lr11-distribution.md proof records live apt Release + install.sh 200 evidence"
if [[ -f "${PROOF_DIR}/lr11-distribution.md" ]] \
  && grep -q '200' "${PROOF_DIR}/lr11-distribution.md"; then
  pass "${C}"
else
  fail "${C}" "missing ${PROOF_DIR}/lr11-distribution.md with recorded 200 evidence"
fi

# --- 13. reconciler production caller (LR12) ----------------------------------------------
C="13. NodeWorkloadReconciler constructed in production bin/server code; Linux-gated test exists"
if grep -rq 'NodeWorkloadReconciler' "${BIN_SRC}" "${SERVER_SRC}" 2>/dev/null \
  && crates_grep 'node_run_converges_transient_unit'; then
  pass "${C}"
else
  fail "${C}" "no production NodeWorkloadReconciler caller or canonical LR12 test missing"
fi

# --- 14. formatting (always) -----------------------------------------------------------------
C="14. cargo fmt --all --check passes"
if cargo fmt --all --check >/dev/null 2>&1; then
  pass "${C}"
else
  fail "${C}" "cargo fmt --all --check reported diffs"
fi

# --- summary ---------------------------------------------------------------------------------
printf '\n%d/14 conditions green\n' "${PASS}"
if [[ ${FAIL} -gt 0 ]]; then
  exit 1
fi
exit 0
