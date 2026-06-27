#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Cloudflare Adapters plan
# (`docs/private/plans/cloudflare-adapters-plan.md`).
#
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# Ships in CFA0 so /goal is verifiable from day one; CFA1-CFA9 progressively
# flip conditions from FAIL to PASS, CFA9 closes the plan and archives it.
#
# Primitives-first: CFA builds a Nimbus KV primitive (`TenantKvStore` in
# nimbus-storage) and a durable-object substrate, then thin Cloudflare surfaces
# over them, plus a minimal Workers-runtime slice so `env.NS` is proven inside a
# real Worker.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/cloudflare-adapters-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/cloudflare-adapters-plan.md"
AGENTS_MD="CLAUDE.md"
PLANS_README="docs/private/plans/README.md"
RESEARCH_DOC="docs/private/plans/research/cloudflare-adapters-2026.md"
PROOF_DIR="docs/private/plans/proof/cloudflare-adapters"
PROOF_CFA0="${PROOF_DIR}/cfa0-baseline.md"
PROOF_CFA5="${PROOF_DIR}/cfa5-env-ns-e2e.md"
PROOF_CFA6="${PROOF_DIR}/cfa6-do-primitive.md"

ADAPTERS_MOD="crates/nimbus-server/src/adapters/mod.rs"
CF_DIR="crates/nimbus-server/src/adapters/cloudflare"
CF_MOD="${CF_DIR}/mod.rs"
CF_CONFIG="${CF_DIR}/config.rs"
CF_KV_DIR="${CF_DIR}/kv"
CF_DO_DIR="${CF_DIR}/durable_objects"
START_ADAPTERS="crates/nimbus-bin/src/start/adapters.rs"
NIMBUS_STORAGE="crates/nimbus-storage"
NIMBUS_RUNTIME="crates/nimbus-runtime"
RUNTIME_HOST="${NIMBUS_RUNTIME}/src/host.rs"
SERVICES_CATALOG="crates/nimbus-services/src/catalog.rs"
OPERATOR_DOC="docs/private/operating/cloudflare-adapters.md"
COMMON_BIND_GUARD_PATHS="crates/nimbus-core/src crates/nimbus-net/src"

PASS=0
FAIL=0
FAIL_DETAIL=()

# -------- helpers ----------------------------------------------------------

pass() {
  PASS=$((PASS + 1))
  printf '  \033[32mPASS\033[0m  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  if [ $# -ge 2 ]; then
    printf '        %s\n' "$2"
    FAIL_DETAIL+=("$1 — $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
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

# True if directory exists and contains at least one *.rs file.
# (Portable to macOS bash 3.2 — no mapfile/readarray.)
dir_has_rs() {
  [ -d "$1" ] || return 1
  [ -n "$(find "$1" -name '*.rs' 2>/dev/null | head -n 1)" ]
}

# True if any *.rs file under directory ($2) matches the ERE pattern ($1).
grep_dir() {
  [ -d "$2" ] || return 1
  grep -rqE --include='*.rs' "$1" "$2" 2>/dev/null
}

ci_branch() {
  if [ -n "${NIMBUS_VERIFY_CI_BRANCH:-}" ]; then
    printf '%s\n' "${NIMBUS_VERIFY_CI_BRANCH}"
    return 0
  fi

  branch="$(git branch --show-current 2>/dev/null || true)"
  if [ -n "${branch}" ]; then
    printf '%s\n' "${branch}"
  else
    printf 'main\n'
  fi
}

ci_workflow_green() {
  branch="$1"
  if ! command -v gh >/dev/null 2>&1; then
    printf '        note: gh not on PATH; ci.yml for %s is UNVERIFIED\n' "${branch}"
    return 1
  fi

  latest="$(gh run list --branch "${branch}" --workflow ci.yml --limit 1 --json conclusion,status,databaseId,headSha 2>/dev/null || true)"
  if [ -z "${latest}" ] || [ "${latest}" = "[]" ]; then
    printf '        note: no ci.yml run found for branch %s\n' "${branch}"
    return 1
  fi

  conclusion="$(printf '%s\n' "${latest}" | grep -oE '"conclusion":"[^"]*"' | head -n 1 | cut -d: -f2 | tr -d '"')"
  status="$(printf '%s\n' "${latest}" | grep -oE '"status":"[^"]*"' | head -n 1 | cut -d: -f2 | tr -d '"')"
  run_id="$(printf '%s\n' "${latest}" | grep -oE '"databaseId":[0-9]+' | head -n 1 | cut -d: -f2)"

  if [ "${conclusion}" = "success" ]; then
    return 0
  fi

  printf '        note: latest ci.yml for %s is status=%s conclusion=%s run=%s\n' \
    "${branch}" "${status:-unknown}" "${conclusion:-none}" "${run_id:-unknown}"
  return 1
}

# -------- conditions -------------------------------------------------------

printf '\033[1mCFA verification gate — cloudflare-adapters (primitives-first)\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan checked in.
step 1 "Plan checked in"
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  pass "Plan exists at ${PLAN_FILE}"
else
  fail "Plan missing" "Neither ${PLAN_ACTIVE} nor ${PLAN_ARCHIVED} exists"
fi

# 2. Routing entries.
step 2 "Routing entries exist"
has_agents_route=0
has_plans_route=0
if [ -f "${AGENTS_MD}" ] || [ -L "${AGENTS_MD}" ]; then
  grep -q 'cloudflare-adapters-plan' "${AGENTS_MD}" && has_agents_route=1
fi
if [ -f "${PLANS_README}" ]; then
  grep -q 'cloudflare-adapters-plan' "${PLANS_README}" && has_plans_route=1
fi
if [ "${has_agents_route}" = "1" ] && [ "${has_plans_route}" = "1" ]; then
  pass "${AGENTS_MD} and ${PLANS_README} reference cloudflare-adapters-plan"
else
  fail "Routing entries incomplete" "agents=${has_agents_route} plans_readme=${has_plans_route}"
fi

# 3. CFA0: research doc + baseline proof.
step 3 "CFA0 deliverables present (research doc + baseline proof)"
cfa0_ok=1
[ -f "${RESEARCH_DOC}" ] || cfa0_ok=0
[ -f "${PROOF_CFA0}" ] || cfa0_ok=0
if [ "${cfa0_ok}" = "1" ]; then
  pass "${RESEARCH_DOC} and ${PROOF_CFA0} exist"
else
  details=""
  [ ! -f "${RESEARCH_DOC}" ] && details="${details} missing ${RESEARCH_DOC};"
  [ ! -f "${PROOF_CFA0}" ] && details="${details} missing ${PROOF_CFA0};"
  fail "CFA0 deliverables incomplete" "${details}"
fi

# 4. CFA1: adapter skeleton + config + wiring + toggle.
step 4 "CFA1: cloudflare adapter module + config + wiring"
c4_mod=0; c4_cfg=0; c4_register=0; c4_parser=0; c4_toggle=0
[ -f "${CF_MOD}" ] && grep -qE 'struct CloudflareConfig' "${CF_MOD}" && c4_mod=1 && c4_cfg=1
[ -f "${ADAPTERS_MOD}" ] && grep -qE '^[[:space:]]*pub mod cloudflare;' "${ADAPTERS_MOD}" && c4_register=1
[ -f "${CF_CONFIG}" ] && c4_parser=1
[ -f "${START_ADAPTERS}" ] && grep -qiE 'cloudflare' "${START_ADAPTERS}" && c4_toggle=1
if [ "${c4_mod}" = 1 ] && [ "${c4_register}" = 1 ] && [ "${c4_parser}" = 1 ] && [ "${c4_toggle}" = 1 ]; then
  pass "adapter module + CloudflareConfig + pub mod cloudflare + config.rs + start toggle"
else
  fail "CFA1 wiring incomplete" "mod=${c4_mod} register=${c4_register} parser=${c4_parser} toggle=${c4_toggle}"
fi

# 5. CFA2: the KV PRIMITIVE — TenantKvStore in nimbus-storage + conformance test.
step 5 "CFA2: KV primitive (TenantKvStore in nimbus-storage)"
c5_trait=0; c5_test=0
grep -rqE 'TenantKvStore' "${NIMBUS_STORAGE}/src" 2>/dev/null && c5_trait=1
grep -rqE 'kv_(put|get|list)' "${NIMBUS_STORAGE}" 2>/dev/null && c5_test=1
if [ "${c5_trait}" = 1 ] && [ "${c5_test}" = 1 ]; then
  pass "TenantKvStore trait + kv_* methods present in nimbus-storage"
else
  fail "CFA2 KV primitive incomplete" "trait=${c5_trait} kv_methods=${c5_test}"
fi

# 6. CFA3: Workers KV ADAPTER over the primitive + CfKv* host ops + REST + test.
step 6 "CFA3: Workers KV adapter over the primitive"
c6_map=0; c6_host=0; c6_rest=0; c6_test=0
if dir_has_rs "${CF_KV_DIR}"; then
  grep_dir 'TenantKvStore|kv_(put|get|list)' "${CF_KV_DIR}" && c6_map=1
  grep_dir 'rest|Router|router|axum' "${CF_KV_DIR}" && c6_rest=1
fi
[ -f "${RUNTIME_HOST}" ] && grep -qE 'CfKv(Get|Put|Delete|List)' "${RUNTIME_HOST}" && c6_host=1
{ [ -f "crates/nimbus-server/tests/cloudflare_kv.rs" ] || grep_dir 'expiration_ttl|list_complete|#\[(tokio::)?test\]' "${CF_KV_DIR}"; } && c6_test=1
if [ "${c6_map}" = 1 ] && [ "${c6_host}" = 1 ] && [ "${c6_rest}" = 1 ] && [ "${c6_test}" = 1 ]; then
  pass "KV adapter over TenantKvStore + CfKv* host ops + REST surface + contract test"
else
  fail "CFA3 incomplete" "map=${c6_map} host=${c6_host} rest=${c6_rest} test=${c6_test}"
fi

# 7. CFA4: minimal Workers runtime profile in nimbus-runtime.
step 7 "CFA4: Workers runtime slice (module-worker fetch dispatch + env)"
c7=0
if grep -rqE 'CloudflareWorker|WorkersRuntime|WorkersProfile|module_worker|workers_fetch|worker_fetch|WorkerEntrypoint' "${NIMBUS_RUNTIME}/src" 2>/dev/null; then
  c7=1
fi
if [ "${c7}" = 1 ]; then
  pass "Workers runtime profile present in nimbus-runtime"
else
  fail "CFA4 incomplete" "no Workers runtime profile marker in ${NIMBUS_RUNTIME}/src"
fi

# 8. CFA5: env.NS end-to-end inside a real Worker (proof + test).
step 8 "CFA5: env.NS end-to-end inside a real Worker"
c8_proof=0; c8_test=0
[ -f "${PROOF_CFA5}" ] && c8_proof=1
if grep -rqE 'env\.NS|env\["NS"\]|cloudflare_kv_worker|cf_kv.*e2e|env_ns' crates 2>/dev/null; then
  c8_test=1
fi
if [ "${c8_proof}" = 1 ] && [ "${c8_test}" = 1 ]; then
  pass "env.NS real-Worker end-to-end test + ${PROOF_CFA5}"
else
  fail "CFA5 incomplete" "proof=${c8_proof} e2e_test=${c8_test}"
fi

# 9. CFA6: durable-object PRIMITIVE — design proof + catalog single-instance.
step 9 "CFA6: durable-object primitive (catalog single-instance resource)"
c9_proof=0; c9_catalog=0
[ -f "${PROOF_CFA6}" ] && c9_proof=1
[ -f "${SERVICES_CATALOG}" ] && grep -qiE 'DurableObjectInstance|durable.?object' "${SERVICES_CATALOG}" && c9_catalog=1
if [ "${c9_proof}" = 1 ] && [ "${c9_catalog}" = 1 ]; then
  pass "DO design proof + DurableObjectInstance catalog resource"
else
  fail "CFA6 incomplete" "proof=${c9_proof} catalog=${c9_catalog}"
fi

# 10. CFA7+CFA8: DO storage/lifecycle/RPC + alarms + WS hibernation + tests.
step 10 "CFA7+CFA8: DO storage/lifecycle/RPC + alarms + WebSocket hibernation"
c10_mod=0; c10_alarm=0; c10_ws=0; c10_test=0
if dir_has_rs "${CF_DO_DIR}"; then
  grep_dir 'sql.exec|sql_exec|SqlStorageCursor|storage|transaction' "${CF_DO_DIR}" && c10_mod=1
  grep_dir 'set_alarm|setAlarm|fn alarm|alarm\(' "${CF_DO_DIR}" && c10_alarm=1
  grep_dir 'accept_web_socket|acceptWebSocket|serialize_attachment|serializeAttachment|hibernat' "${CF_DO_DIR}" && c10_ws=1
  grep_dir '#\[(tokio::)?test\]|single.?instance|per.?instance' "${CF_DO_DIR}" && c10_test=1
fi
[ -f "crates/nimbus-server/tests/cloudflare_durable_objects.rs" ] && c10_test=1
if [ "${c10_mod}" = 1 ] && [ "${c10_alarm}" = 1 ] && [ "${c10_ws}" = 1 ] && [ "${c10_test}" = 1 ]; then
  pass "DO storage/lifecycle/RPC + alarms + WebSocket hibernation + tests"
else
  fail "CFA7+CFA8 incomplete" "module=${c10_mod} alarm=${c10_alarm} ws=${c10_ws} test=${c10_test}"
fi

# 11. CFA9: operator doc + ledger all done + CI green.
step 11 "CFA9: operator doc + ledger green + CI green"
c11_doc=0; ledger_clean=0; ci_green=0
c11_ci_branch="$(ci_branch)"
[ -f "${OPERATOR_DOC}" ] && c11_doc=1
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  ledger_rows="$(awk '
    /^\| CFA \| Description \| Status \|/ {in_ledger=1; next}
    in_ledger && /^$/ {in_ledger=0}
    in_ledger && /^\| CFA[0-9]/ {print}
  ' "${PLAN_FILE}")"
  if [ -n "${ledger_rows}" ] && ! printf '%s\n' "${ledger_rows}" | grep -vE '\| done \|' | grep -qE '^\| CFA[0-9]'; then
    ledger_clean=1
  fi
fi
ci_workflow_green "${c11_ci_branch}" && ci_green=1
if [ "${c11_doc}" = 1 ] && [ "${ledger_clean}" = 1 ] && [ "${ci_green}" = 1 ]; then
  pass "operator doc present, ledger clean, CI green"
else
  fail "CFA9 closeout incomplete" "doc=${c11_doc} ledger=${ledger_clean} ci=${ci_green} ci_branch=${c11_ci_branch}"
fi

# 12. Security posture: fail-closed bind/auth/tenant behavior.
step 12 "Security posture: loopback guard + auth + tenant isolation"
c12_helper=0; c12_uses_helper=0; c12_bind_test=0; c12_auth_test=0; c12_tenant_binding=0; c12_cross_tenant_do=0
for guard_path in ${COMMON_BIND_GUARD_PATHS}; do
  if [ -d "${guard_path}" ] && grep -rqE 'refuse_non_loopback_bind' "${guard_path}" 2>/dev/null; then
    c12_helper=1
  fi
done
if grep_dir 'refuse_non_loopback_bind' "${CF_DIR}" || { [ -f "${START_ADAPTERS}" ] && grep -qiE 'cloudflare.*refuse_non_loopback_bind|refuse_non_loopback_bind.*cloudflare' "${START_ADAPTERS}"; }; then
  c12_uses_helper=1
fi
if { grep_dir 'non.?loopback|loopback.*refus|refus.*loopback' "${CF_DIR}" || grep -rqE 'cloudflare.*non.?loopback|non.?loopback.*cloudflare|refus.*cloudflare.*loopback' crates/nimbus-server/tests 2>/dev/null; }; then
  c12_bind_test=1
fi
if { grep_dir 'unauthenticated|requires.*auth|dev.?cred|credential' "${CF_DIR}" || grep -rqE 'cloudflare.*unauthenticated|unauthenticated.*cloudflare|cloudflare.*requires.*auth' crates/nimbus-server/tests 2>/dev/null; }; then
  c12_auth_test=1
fi
if grep_dir 'AccessKeyRegistry|credential.*TenantId|TenantId.*credential|tenant.*credential|credential.*tenant' "${CF_DIR}"; then
  c12_tenant_binding=1
fi
if { grep_dir 'cross.?tenant|tenant_a|tenant.?A|idFromString|id_from_string|forged.*64' "${CF_DO_DIR}" || grep -rqE 'cross.?tenant.*cloudflare|cloudflare.*cross.?tenant|tenant_a.*durable|idFromString|id_from_string|forged.*64' crates/nimbus-server/tests 2>/dev/null; }; then
  c12_cross_tenant_do=1
fi
if [ "${c12_helper}" = 1 ] && [ "${c12_uses_helper}" = 1 ] && [ "${c12_bind_test}" = 1 ] && [ "${c12_auth_test}" = 1 ] && [ "${c12_tenant_binding}" = 1 ] && [ "${c12_cross_tenant_do}" = 1 ]; then
  pass "Cloudflare ingress surfaces share the bind guard, require auth, and prove tenant isolation"
else
  fail "Security posture incomplete" "helper=${c12_helper} uses_helper=${c12_uses_helper} bind_test=${c12_bind_test} auth_test=${c12_auth_test} tenant_binding=${c12_tenant_binding} cross_tenant_do=${c12_cross_tenant_do}"
fi

# -------- summary ----------------------------------------------------------

printf '\n\033[1m%d passed, %d failed\033[0m\n' "${PASS}" "${FAIL}"

if [ "${FAIL}" -gt 0 ]; then
  printf '\nFailures:\n'
  for d in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${d}"
  done
  exit 1
fi

exit 0
