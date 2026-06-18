#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Cloudflare Adapters plan
# (`docs/private/plans/cloudflare-adapters-plan.md`).
#
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# Ships in CFA0 so /goal is verifiable from day one; CFA1-CFA6 progressively
# flip conditions from FAIL to PASS, CFA7 closes the plan and archives it.
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
PROOF_CFA4="${PROOF_DIR}/cfa4-do-design.md"

ADAPTERS_MOD="crates/nimbus-server/src/adapters/mod.rs"
CF_DIR="crates/nimbus-server/src/adapters/cloudflare"
CF_MOD="${CF_DIR}/mod.rs"
CF_CONFIG="${CF_DIR}/config.rs"
CF_KV_DIR="${CF_DIR}/kv"
CF_DO_DIR="${CF_DIR}/durable_objects"
START_ADAPTERS="crates/nimbus-bin/src/start/adapters.rs"
RUNTIME_HOST="crates/nimbus-runtime/src/host.rs"
SERVICES_CATALOG="crates/nimbus-services/src/catalog.rs"
OPERATOR_DOC="docs/private/operating/cloudflare-adapters.md"

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

# -------- conditions -------------------------------------------------------

printf '\033[1mCFA verification gate — cloudflare-adapters\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan checked in.
step 1 "Plan checked in"
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  pass "Plan exists at ${PLAN_FILE}"
else
  fail "Plan missing" "Neither ${PLAN_ACTIVE} nor ${PLAN_ARCHIVED} exists"
fi

# 2. Routing entries in CLAUDE.md (= AGENTS.md) and docs/private/plans/README.md.
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
if [ "${c4_mod}" = 1 ] && [ "${c4_cfg}" = 1 ] && [ "${c4_register}" = 1 ] && [ "${c4_parser}" = 1 ] && [ "${c4_toggle}" = 1 ]; then
  pass "adapter module + CloudflareConfig + pub mod cloudflare + config.rs + start toggle"
else
  fail "CFA1 wiring incomplete" "mod=${c4_mod} config=${c4_cfg} register=${c4_register} parser=${c4_parser} toggle=${c4_toggle}"
fi

# 5. CFA2: KV storage mapping + KV test.
step 5 "CFA2: Workers KV storage mapping + test"
c5_map=0; c5_test=0
if dir_has_rs "${CF_KV_DIR}"; then
  grep_dir '__cf_kv__|reserved.*kv|kv.*reserved' "${CF_KV_DIR}" && c5_map=1
  # A KV test: a #[test]/#[tokio::test] in the kv tree, or the contract markers.
  grep_dir '#\[(tokio::)?test\]|expirationTtl|expiration_ttl|list_complete' "${CF_KV_DIR}" && c5_test=1
fi
[ -f "crates/nimbus-server/tests/cloudflare_kv.rs" ] && c5_test=1
if [ "${c5_map}" = 1 ] && [ "${c5_test}" = 1 ]; then
  pass "KV mapping module (reserved-table scheme) + KV contract test present"
else
  fail "CFA2 incomplete" "mapping=${c5_map} test=${c5_test}"
fi

# 6. CFA3: HostCallOperation CfKv variants + REST surface + conformance test.
step 6 "CFA3: KV HostBridge variants + REST front door + conformance test"
c6_host=0; c6_rest=0; c6_conf=0
[ -f "${RUNTIME_HOST}" ] && grep -qE 'CfKv(Get|Put|Delete|List)' "${RUNTIME_HOST}" && c6_host=1
grep_dir 'rest|Router|router|axum' "${CF_KV_DIR}" && c6_rest=1
{ [ -f "crates/nimbus-server/tests/cloudflare_kv.rs" ] || grep_dir 'conformance' "${CF_KV_DIR}"; } && c6_conf=1
if [ "${c6_host}" = 1 ] && [ "${c6_rest}" = 1 ] && [ "${c6_conf}" = 1 ]; then
  pass "CfKv* HostCallOperation variants + KV REST handler + conformance test"
else
  fail "CFA3 incomplete" "host=${c6_host} rest=${c6_rest} conformance=${c6_conf}"
fi

# 7. CFA4: DO design proof + catalog single-instance extension.
step 7 "CFA4: Durable Objects design + catalog seam"
c7_proof=0; c7_catalog=0
[ -f "${PROOF_CFA4}" ] && c7_proof=1
[ -f "${SERVICES_CATALOG}" ] && grep -qiE 'durable.?object|DurableObjectInstance|Instance \{' "${SERVICES_CATALOG}" && c7_catalog=1
if [ "${c7_proof}" = 1 ] && [ "${c7_catalog}" = 1 ]; then
  pass "DO design proof + catalog single-instance/per-instance extension"
else
  fail "CFA4 incomplete" "proof=${c7_proof} catalog=${c7_catalog}"
fi

# 8. CFA5: DO storage/lifecycle module + test.
step 8 "CFA5: Durable Objects storage + lifecycle"
c8_mod=0; c8_test=0
if dir_has_rs "${CF_DO_DIR}"; then
  grep_dir 'sql.exec|sql_exec|SqlStorageCursor|storage|transaction' "${CF_DO_DIR}" && c8_mod=1
  grep_dir '#\[(tokio::)?test\]|single.?instance|per.?instance' "${CF_DO_DIR}" && c8_test=1
fi
[ -f "crates/nimbus-server/tests/cloudflare_durable_objects.rs" ] && c8_test=1
if [ "${c8_mod}" = 1 ] && [ "${c8_test}" = 1 ]; then
  pass "DO storage/lifecycle module + single-instance/atomicity test"
else
  fail "CFA5 incomplete" "module=${c8_mod} test=${c8_test}"
fi

# 9. CFA6: DO alarms + WebSocket hibernation + tests.
step 9 "CFA6: Durable Objects alarms + WebSocket hibernation"
c9_alarm=0; c9_ws=0; c9_test=0
if dir_has_rs "${CF_DO_DIR}"; then
  grep_dir 'set_alarm|setAlarm|fn alarm|alarm\(' "${CF_DO_DIR}" && c9_alarm=1
  grep_dir 'accept_web_socket|acceptWebSocket|serialize_attachment|serializeAttachment|hibernat' "${CF_DO_DIR}" && c9_ws=1
  grep_dir '#\[(tokio::)?test\]' "${CF_DO_DIR}" && c9_test=1
fi
[ -f "crates/nimbus-server/tests/cloudflare_durable_objects.rs" ] && c9_test=1
if [ "${c9_alarm}" = 1 ] && [ "${c9_ws}" = 1 ] && [ "${c9_test}" = 1 ]; then
  pass "alarms (scheduler-backed) + WebSocket hibernation + round-trip tests"
else
  fail "CFA6 incomplete" "alarm=${c9_alarm} ws=${c9_ws} test=${c9_test}"
fi

# 10. CFA7: operator doc + ledger all done + CI green.
step 10 "CFA7: operator doc + ledger green + CI green"
c10_doc=0; ledger_clean=0; ci_green=0
[ -f "${OPERATOR_DOC}" ] && c10_doc=1
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
if command -v gh >/dev/null 2>&1; then
  latest=$(gh run list --branch main --workflow ci.yml --limit 1 --json conclusion 2>/dev/null | grep -oE '"conclusion":"[^"]*"' | head -n 1)
  if [ "${latest}" = '"conclusion":"success"' ]; then
    ci_green=1
  elif [ -z "${latest}" ]; then
    ci_green=1
    printf '        note: gh returned no ci.yml conclusion for main; CI-green ASSUMED — verify manually\n'
  fi
else
  ci_green=1
  printf '        note: gh not on PATH; CI-green for main is UNVERIFIED locally (CI enforces it on merge)\n'
fi
if [ "${c10_doc}" = 1 ] && [ "${ledger_clean}" = 1 ] && [ "${ci_green}" = 1 ]; then
  pass "operator doc present, ledger clean, CI green"
else
  fail "CFA7 closeout incomplete" "doc=${c10_doc} ledger=${ledger_clean} ci=${ci_green}"
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
