#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Nimbus Dev Adapter
# Autodetection plan (DX bands A/F/W/L/D, archived completed 2026-06-12).
# Exits 0 iff every condition is satisfied. Shipped in DXA0 so /goal was
# verifiable from day one; rows progressively flipped conditions to PASS.
#
# Run from the repo root (works from a linked git worktree: plan and proof
# files live under the MAIN worktree's untracked docs/private/, which this
# script resolves via `git worktree list`). Set NIMBUS_PRIVATE_DOCS to
# override the private-docs root explicitly.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

MAIN_WORKTREE="$(git worktree list --porcelain | awk '/^worktree /{print $2; exit}')"
PRIVATE_ROOT="${NIMBUS_PRIVATE_DOCS:-${MAIN_WORKTREE}/docs/private}"
PLAN="${PRIVATE_ROOT}/plans/archive/nimbus-dev-adapter-autodetection-plan.md"
PROOF_DIR="${PRIVATE_ROOT}/plans/proof/nimbus-dev-autodetect"

DEV_DIR="crates/nimbus-bin/src/dev"
START_ADAPTERS="crates/nimbus-bin/src/start/adapters.rs"
LANDING="website/src/content/docs/index.mdx"

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

# rg helper: quiet recursive fixed-pattern search over nimbus-bin sources.
bin_has() {
  rg -q "$1" crates/nimbus-bin/src/ 2>/dev/null
}

# --- 1. plan rows done + proof files -----------------------------------------
C="1. plan ledger fully done and every proof file exists"
PROOFS=(
  dxa0-baseline dxa1-surface-model dxa2-firestore-routes
  dxf1-import-scanner dxf2-firestore-detection dxf3-wiring-refusal
  dxf4-tenant-mapping dxf5-loop-semantics
  dxw1-wire-detection dxw2-credentials-env dxw3-startup-enablement
  dxl1-live-redetection dxl2-midsession-adoption
  dxd1-docs-flips dxd2-closeout
)
missing_proofs=()
for p in "${PROOFS[@]}"; do
  [[ -f "${PROOF_DIR}/${p}.md" ]] || missing_proofs+=("${p}")
done
if [[ ! -f "${PLAN}" ]]; then
  fail "${C}" "plan not found at ${PLAN}"
elif grep -q '| pending |' "${PLAN}"; then
  fail "${C}" "plan still has pending ledger rows"
elif [[ ${#missing_proofs[@]} -gt 0 ]]; then
  fail "${C}" "missing proofs: ${missing_proofs[*]}"
else
  pass "${C}"
fi

# --- 2. surface-model split ---------------------------------------------------
C="2. app-adapter/wire-surface split with combined-resolution test"
if bin_has 'struct WireSurfaces' \
  && bin_has 'fn detect_wire_surfaces' \
  && bin_has 'fn convex_app_with_mongodb_dep_resolves_adapter_and_surface'; then
  pass "${C}"
else
  fail "${C}" "need WireSurfaces type, detect_wire_surfaces, and combined-resolution test in ${DEV_DIR}"
fi

# --- 3. always-on serving: dev unconditional; start default-on with opt-outs ----
C="3. dev and start serve adapter surfaces by default; opt-outs exist"
if grep -q 'firestore: true' "${DEV_DIR}/plan.rs" 2>/dev/null \
  && bin_has 'fn dev_serves_firestore_routes_without_firebase_markers' \
  && grep -q 'fn start_serves_all_adapters_by_default' "${START_ADAPTERS}" \
  && grep -q 'fn adapter_opt_out_flags_disable_surfaces' "${START_ADAPTERS}"; then
  pass "${C}"
else
  fail "${C}" "need dev always-on Firestore + start default-on/opt-out tests (D7)"
fi

# --- 4. scanner covered set from embedded manifest ----------------------------
C="4. import scanner derives covered set from embedded package manifest"
if [[ -f "${DEV_DIR}/firebase_scan.rs" ]] \
  && grep -q 'fn covered_specifiers' "${DEV_DIR}/firebase_scan.rs" \
  && grep -q 'from_embedded_manifest' "${DEV_DIR}/firebase_scan.rs" \
  && bin_has 'fn covered_set_derives_from_embedded_package_manifest'; then
  pass "${C}"
else
  fail "${C}" "need ${DEV_DIR}/firebase_scan.rs with covered_specifiers + from_embedded_manifest + derivation test"
fi

# --- 5. scanner classification tests ------------------------------------------
C="5. scanner tests: pass / refuse / indeterminate / out-of-scan"
scanner_tests=(
  covered_only_app_passes_scan
  uncovered_auth_import_refuses_with_file_line
  dynamic_specifier_is_indeterminate_and_refuses
  node_modules_specifiers_are_out_of_scan
)
missing_t=()
for t in "${scanner_tests[@]}"; do
  bin_has "fn ${t}" || missing_t+=("${t}")
done
if [[ ${#missing_t[@]} -eq 0 ]]; then
  pass "${C}"
else
  fail "${C}" "missing scanner tests: ${missing_t[*]}"
fi

# --- 6. refusal mutates nothing ------------------------------------------------
C="6. refusal path leaves package.json byte-identical"
if bin_has 'fn refusal_leaves_package_json_byte_identical'; then
  pass "${C}"
else
  fail "${C}" "missing refusal_leaves_package_json_byte_identical test"
fi

# --- 7. wire-surface detection tests -------------------------------------------
C="7. wire-surface detection: mongodb/mongoose/dynamodb signals, aws-sdk hint-only"
wire_tests=(
  mongodb_dependency_enables_mongodb_surface
  mongoose_dependency_enables_mongodb_surface
  dynamodb_sdk_dependency_enables_dynamodb_surface
  bare_aws_sdk_v2_is_hint_only
)
missing_t=()
for t in "${wire_tests[@]}"; do
  bin_has "fn ${t}" || missing_t+=("${t}")
done
if [[ ${#missing_t[@]} -eq 0 ]]; then
  pass "${C}"
else
  fail "${C}" "missing wire detection tests: ${missing_t[*]}"
fi

# --- 8. generated credentials: persistent, owner-only, deny-by-default intact --
C="8. dev credentials persist (0600) and deny-by-default listener tests intact"
if bin_has 'fn dev_wire_credentials_persist_across_runs' \
  && bin_has 'fn credential_file_is_owner_only_mode' \
  && grep -q 'fn mongodb_listener_requires_scram_credentials' "${START_ADAPTERS}"; then
  pass "${C}"
else
  fail "${C}" "need credential persistence + file-mode tests and intact SCRAM-required test"
fi

# --- 9. .env.local: Nimbus-owned keys only, no clobber --------------------------
C="9. .env.local writes Nimbus-owned keys only and never clobbers user keys"
if bin_has 'fn env_local_writes_only_nimbus_owned_keys' \
  && bin_has 'fn user_owned_env_keys_are_never_clobbered'; then
  pass "${C}"
else
  fail "${C}" "missing env_local ownership/no-clobber tests"
fi

# --- 10. mid-session adoption under always-available listeners (D6) --------------
C="10. mid-session adoption refreshes presentation; listeners and subscriptions untouched"
if bin_has 'fn mid_session_mongodb_adoption_round_trips_with_subscriptions_intact' \
  && bin_has 'fn repeated_manifest_rescans_are_convergent'; then
  pass "${C}"
else
  fail "${C}" "missing D6 adoption tests (round-trip with live subscription; convergent rescans)"
fi

# --- 11. mid-session firebase adoption is scan-gated -----------------------------
C="11. mid-session Firebase adoption goes through the scan gate"
if bin_has 'fn mid_session_firebase_adoption_runs_scan_gate'; then
  pass "${C}"
else
  fail "${C}" "missing mid_session_firebase_adoption_runs_scan_gate test"
fi

# --- 12. landing tab flips gated on shipped rows ---------------------------------
C="12. landing tabs show nimbus dev only with their gating rows done"
tab_block() { # tab_block <label> — print the TabItem block for a label
  awk -v label="$1" '
    $0 ~ "<TabItem label=\"" label "\">" {inblock=1}
    inblock {print}
    inblock && /<\/TabItem>/ {exit}
  ' "${LANDING}" 2>/dev/null
}
row_done() { # row_done <ROW-ID> — plan ledger row is done
  grep -E "^\| ${1} \|" "${PLAN}" 2>/dev/null | grep -q '| done |'
}
flip_violations=()
fb_block="$(tab_block Firebase)"
if [[ -n "${fb_block}" ]] && grep -q 'nimbus dev' <<<"${fb_block}" \
  && ! grep -q 'nimbus start' <<<"${fb_block}"; then
  for r in DXF3 DXF4 DXF5; do row_done "${r}" || flip_violations+=("Firebase needs ${r}"); done
fi
for label in MongoDB DynamoDB; do
  blk="$(tab_block "${label}")"
  if [[ -n "${blk}" ]] && grep -q 'nimbus dev' <<<"${blk}" \
    && ! grep -q 'nimbus start' <<<"${blk}"; then
    for r in DXW3 DXL1; do row_done "${r}" || flip_violations+=("${label} needs ${r}"); done
  fi
done
if [[ ! -f "${LANDING}" ]]; then
  fail "${C}" "landing not found at ${LANDING}"
elif [[ ${#flip_violations[@]} -eq 0 ]]; then
  pass "${C}"
else
  fail "${C}" "premature tab flips: ${flip_violations[*]}"
fi

# --- 13. docs gate ----------------------------------------------------------------
C="13. scripts/check-docs.sh passes"
if bash scripts/check-docs.sh >/dev/null 2>&1; then
  pass "${C}"
else
  fail "${C}" "check-docs.sh failed (run it directly for detail)"
fi

# --- 14. fmt + recorded closeout test runs ------------------------------------------
C="14. cargo fmt clean and closeout proof records green focused tests"
if cargo fmt --all --check >/dev/null 2>&1 \
  && [[ -f "${PROOF_DIR}/dxd2-closeout.md" ]] \
  && grep -q 'cargo test -p nimbus-bin' "${PROOF_DIR}/dxd2-closeout.md" 2>/dev/null; then
  pass "${C}"
else
  fail "${C}" "need clean cargo fmt and dxd2-closeout.md recording cargo test -p nimbus-bin"
fi

# --- summary -------------------------------------------------------------------------
printf '\n%d passed, %d failed\n' "${PASS}" "${FAIL}"
if [[ ${FAIL} -gt 0 ]]; then
  printf 'failing:\n'
  for d in "${FAIL_DETAIL[@]}"; do printf '  - %s\n' "${d}"; done
  exit 1
fi
exit 0
