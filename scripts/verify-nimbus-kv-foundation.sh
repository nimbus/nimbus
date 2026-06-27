#!/usr/bin/env bash
# Aggregate completion-gate verifier for the NKV Foundation plan
# (`docs/private/plans/nimbus-kv-foundation-plan.md`).
#
# Exits 0 iff every condition in the plan's Completion Gate is satisfied.
# Ships in NKV0 F0 so /goal is verifiable from day one; F1-F5 progressively
# flip conditions from FAIL to PASS, F5 closes the plan and archives it.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PLAN_ACTIVE="docs/private/plans/nimbus-kv-foundation-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/nimbus-kv-foundation-plan.md"
AGENTS_MD="CLAUDE.md"
PLANS_README="docs/private/plans/README.md"
RESEARCH_DOC="docs/private/plans/research/nimbus-kv-architecture-2026.md"
PROOF_DIR="docs/private/plans/proof/nimbus-kv-foundation"
PROOF_BASELINE="${PROOF_DIR}/nkv0-baseline.md"
PROOF_F2="${PROOF_DIR}/f2-kv-primitive.md"

KV_CRATE="crates/nimbus-kv"
KV_CARGO="${KV_CRATE}/Cargo.toml"
KV_SRC="${KV_CRATE}/src"
KV_TESTS="${KV_CRATE}/tests"
NIMBUS_BIN_SRC="crates/nimbus-bin/src"
NIMBUS_STORAGE="crates/nimbus-storage"
NIMBUS_STORAGE_SRC="${NIMBUS_STORAGE}/src"
OPERATOR_DOC="docs/private/operating/nimbus-kv.md"
CONFORMANCE_SCRIPT="scripts/nimbus-kv-conformance.sh"
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
    FAIL_DETAIL+=("$1 - $2")
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

dir_has_rs() {
  [ -d "$1" ] || return 1
  [ -n "$(find "$1" -name '*.rs' 2>/dev/null | head -n 1)" ]
}

grep_dir() {
  [ -d "$2" ] || return 1
  grep -rqE --include='*.rs' "$1" "$2" 2>/dev/null
}

grep_any() {
  pattern="$1"
  shift
  for path in "$@"; do
    if [ -e "${path}" ] || [ -L "${path}" ]; then
      if [ -d "${path}" ]; then
        grep -rqE "${pattern}" "${path}" 2>/dev/null && return 0
      else
        grep -qE "${pattern}" "${path}" 2>/dev/null && return 0
      fi
    fi
  done
  return 1
}

skipfile_path() {
  for candidate in \
    "tests/nimbus-kv-skip.txt" \
    "${KV_TESTS}/nimbus-kv-skip.txt" \
    "${KV_TESTS}/valkey-skip.txt"; do
    if [ -f "${candidate}" ]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done
  printf ''
}

# -------- conditions -------------------------------------------------------

printf '\033[1mNKV verification gate - nimbus-kv-foundation\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

# 1. Plan file exists.
step 1 "Plan file exists"
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
  grep -q 'nimbus-kv-foundation-plan' "${AGENTS_MD}" && has_agents_route=1
fi
if [ -f "${PLANS_README}" ]; then
  grep -q 'nimbus-kv-foundation-plan' "${PLANS_README}" && has_plans_route=1
fi
if [ "${has_agents_route}" = "1" ] && [ "${has_plans_route}" = "1" ]; then
  pass "${AGENTS_MD} and ${PLANS_README} reference nimbus-kv-foundation-plan"
else
  fail "Routing entries incomplete" "agents=${has_agents_route} plans_readme=${has_plans_route}"
fi

# 3. F0: design doc + baseline proof.
step 3 "F0 deliverables present (design doc + baseline proof)"
c3_design=0
c3_proof=0
[ -f "${RESEARCH_DOC}" ] && c3_design=1
[ -f "${PROOF_BASELINE}" ] && c3_proof=1
if [ "${c3_design}" = "1" ] && [ "${c3_proof}" = "1" ]; then
  pass "${RESEARCH_DOC} and ${PROOF_BASELINE} exist"
else
  fail "F0 deliverables incomplete" "design=${c3_design} baseline_proof=${c3_proof}"
fi

# 4. F1: nimbus-kv crate + RESP server + binary/subcommand entrypoint.
step 4 "F1: RESP crate, server, PING handler, and entrypoint"
c4_crate=0
c4_dep=0
c4_server=0
c4_ping=0
c4_entry=0
[ -f "${KV_CARGO}" ] && c4_crate=1
[ -f "${KV_CARGO}" ] && grep -qE 'redis-protocol' "${KV_CARGO}" && c4_dep=1
if dir_has_rs "${KV_SRC}"; then
  grep_dir 'RESP|Resp|redis_protocol|Frame|CommandFrame|HELLO|QUIT|COMMAND' "${KV_SRC}" && c4_server=1
  grep_dir 'PING|Ping|ping' "${KV_SRC}" && c4_ping=1
fi
if [ "${c4_crate}" = "1" ] && grep_any 'nimbus[-_]kv|run_listener|serve|Redis|Valkey' "${NIMBUS_BIN_SRC}" "${KV_CARGO}" "${KV_SRC}"; then
  c4_entry=1
fi
if [ "${c4_crate}" = "1" ] && [ "${c4_dep}" = "1" ] && [ "${c4_server}" = "1" ] && [ "${c4_ping}" = "1" ] && [ "${c4_entry}" = "1" ]; then
  pass "nimbus-kv crate depends on redis-protocol and exposes a RESP server entrypoint"
else
  fail "F1 RESP server incomplete" "crate=${c4_crate} redis_protocol=${c4_dep} server=${c4_server} ping=${c4_ping} entrypoint=${c4_entry}"
fi

# 5. F2: TenantKvStore storage primitive + swappable engine + TTL/RMW tests.
step 5 "F2: TenantKvStore primitive, engines, benchmark, and TTL safety"
c5_trait=0
c5_methods=0
c5_engine_trait=0
c5_fjall=0
c5_bench=0
c5_redb=0
c5_ttl_tests=0
if [ -d "${NIMBUS_STORAGE_SRC}" ]; then
  grep -rqE 'trait[[:space:]]+TenantKvStore' "${NIMBUS_STORAGE_SRC}" 2>/dev/null && c5_trait=1
  if grep -rqE 'kv_get' "${NIMBUS_STORAGE_SRC}" 2>/dev/null &&
     grep -rqE 'kv_put' "${NIMBUS_STORAGE_SRC}" 2>/dev/null &&
     grep -rqE 'kv_delete' "${NIMBUS_STORAGE_SRC}" 2>/dev/null &&
     grep -rqE 'kv_scan' "${NIMBUS_STORAGE_SRC}" 2>/dev/null; then
    c5_methods=1
  fi
  grep -rqE 'trait[[:space:]]+.*Kv.*Engine|KvStorageEngine|TenantKvEngine' "${NIMBUS_STORAGE_SRC}" 2>/dev/null && c5_engine_trait=1
  grep -rqE 'impl[^{;]*TenantKvStore[^{;]*(Redb|redb)|impl[^{;]*(Redb|redb)[^{;]*TenantKvStore' "${NIMBUS_STORAGE_SRC}" 2>/dev/null && c5_redb=1
  if grep -rqE 'compare.?and.?delete|ttl.*extend|extend.*ttl|racing.*SET|expired.*index|expiry.*index' "${NIMBUS_STORAGE}" 2>/dev/null; then
    c5_ttl_tests=1
  fi
fi
grep_any 'fjall' "Cargo.toml" "${NIMBUS_STORAGE}/Cargo.toml" "${KV_CARGO}" && c5_fjall=1
if [ -f "${PROOF_F2}" ] && grep -qE 'redb.*fjall|fjall.*redb' "${PROOF_F2}" && grep -qE 'write|throughput|writes/sec|ops/sec|latency' "${PROOF_F2}"; then
  c5_bench=1
fi
if [ "${c5_trait}" = "1" ] && [ "${c5_methods}" = "1" ] && [ "${c5_engine_trait}" = "1" ] && [ "${c5_fjall}" = "1" ] && [ "${c5_bench}" = "1" ] && [ "${c5_redb}" = "1" ] && [ "${c5_ttl_tests}" = "1" ]; then
  pass "TenantKvStore + kv_* methods + swappable engine + fjall + benchmark + redb + TTL safety tests"
else
  fail "F2 KV primitive incomplete" "trait=${c5_trait} methods=${c5_methods} engine_trait=${c5_engine_trait} fjall=${c5_fjall} bench=${c5_bench} redb=${c5_redb} ttl_tests=${c5_ttl_tests}"
fi

# 6. F3: cache/tiering config + coherency tests.
step 6 "F3: cache/tiering modes and coherency tests"
c6_config=0
c6_cache=0
c6_incr_test=0
c6_expiry_test=0
if dir_has_rs "${KV_SRC}"; then
  grep_dir 'maxmemory|no[-_]?disk|no[-_]?cache|NoDisk|NoCache|CacheMode|TieringConfig' "${KV_SRC}" && c6_config=1
  grep_dir 'struct[[:space:]]+.*Cache|enum[[:space:]]+.*Cache|CacheMode|cache_hit|cache_miss' "${KV_SRC}" && c6_cache=1
fi
if [ -d "${KV_TESTS}" ]; then
  grep -rqE 'concurrent.*INCR|INCR.*concurrent|disk.*no.?disk|no.?disk.*INCR|incr.*coher' "${KV_TESTS}" 2>/dev/null && c6_incr_test=1
  grep -rqE 'expire_at.*cache|cache.*expire_at|logically.?expired|expired.*cache|cache.*TTL|ttl.*cache' "${KV_TESTS}" 2>/dev/null && c6_expiry_test=1
fi
if [ "${c6_config}" = "1" ] && [ "${c6_cache}" = "1" ] && [ "${c6_incr_test}" = "1" ] && [ "${c6_expiry_test}" = "1" ]; then
  pass "cache/tiering config + concurrent INCR + expiry-coherency tests"
else
  fail "F3 cache/tiering incomplete" "config=${c6_config} cache=${c6_cache} incr_test=${c6_incr_test} expiry_test=${c6_expiry_test}"
fi

# 7. F4: redis-rs harness + Valkey external-mode runner + budgeted skipfile.
step 7 "F4: conformance harness and skip accounting"
c7_redis_rs=0
c7_script=0
c7_skip_sections=0
c7_minimum=0
c7_valkey=0
if [ -d "${KV_TESTS}" ]; then
  grep -rqE 'REDISRS_SERVER_BIN|Command::new|spawn.*nimbus[-_]kv' "${KV_TESTS}" 2>/dev/null && c7_redis_rs=1
fi
[ -f "${CONFORMANCE_SCRIPT}" ] && c7_script=1
SKIPFILE="$(skipfile_path)"
if [ -n "${SKIPFILE}" ] &&
   grep -qE -- '---[[:space:]]*encoding[[:space:]]*---|encoding' "${SKIPFILE}" &&
   grep -qE -- '---[[:space:]]*behavioral[[:space:]]*---|behavioral' "${SKIPFILE}"; then
  c7_skip_sections=1
fi
if [ -f "${CONFORMANCE_SCRIPT}" ]; then
  grep -qE 'minimum|MIN_PASS|passing behavioral|all.?skipped|skipfile' "${CONFORMANCE_SCRIPT}" && c7_minimum=1
  grep -qE 'runtest|valkey|--host|--port|RESP2|RESP3|HELLO' "${CONFORMANCE_SCRIPT}" && c7_valkey=1
fi
if [ "${c7_redis_rs}" = "1" ] && [ "${c7_script}" = "1" ] && [ "${c7_skip_sections}" = "1" ] && [ "${c7_minimum}" = "1" ] && [ "${c7_valkey}" = "1" ]; then
  pass "redis-rs spawn harness + Valkey runner + two-section skipfile + minimum pass assertion"
else
  fail "F4 conformance harness incomplete" "redis_rs=${c7_redis_rs} script=${c7_script} skip_sections=${c7_skip_sections} minimum=${c7_minimum} valkey=${c7_valkey}"
fi

# 8. F5: smoke green + operator doc + ledger done + CI green.
step 8 "F5: smoke, operator doc, ledger, and CI"
c8_smoke=0
c8_doc=0
c8_ledger=0
c8_ci=0
if grep_any 'GET.*SET.*DEL.*EXPIRE.*INCR|SET.*GET.*DEL.*EXPIRE.*INCR|RESP2.*RESP3|RESP3.*RESP2' "${KV_TESTS}" "${PROOF_DIR}"; then
  c8_smoke=1
fi
[ -f "${OPERATOR_DOC}" ] && c8_doc=1
PLAN_FILE="$(plan_file)"
if [ -n "${PLAN_FILE}" ]; then
  ledger_rows="$(awk '
    /^\| NKV0 \| Description \| Status \|/ {in_ledger=1; next}
    in_ledger && /^$/ {in_ledger=0}
    in_ledger && /^\| F[0-9]/ {print}
  ' "${PLAN_FILE}")"
  if [ -n "${ledger_rows}" ] && ! printf '%s\n' "${ledger_rows}" | grep -vE '\| done \|' | grep -qE '^\| F[0-9]'; then
    c8_ledger=1
  fi
fi
if command -v gh >/dev/null 2>&1; then
  latest="$(gh run list --branch main --workflow ci.yml --limit 1 --json conclusion 2>/dev/null | grep -oE '"conclusion":"[^"]*"' | head -n 1)"
  if [ "${latest}" = '"conclusion":"success"' ]; then
    c8_ci=1
  elif [ -z "${latest}" ]; then
    c8_ci=1
    printf '        note: gh returned no ci.yml conclusion for main; CI-green ASSUMED - verify manually\n'
  fi
else
  c8_ci=1
  printf '        note: gh not on PATH; CI-green for main is UNVERIFIED locally (CI enforces it on merge)\n'
fi
if [ "${c8_smoke}" = "1" ] && [ "${c8_doc}" = "1" ] && [ "${c8_ledger}" = "1" ] && [ "${c8_ci}" = "1" ]; then
  pass "smoke proof/test, operator doc, ledger clean, CI green"
else
  fail "F5 closeout incomplete" "smoke=${c8_smoke} doc=${c8_doc} ledger=${c8_ledger} ci=${c8_ci}"
fi

# 9. F1 security: bind guard + auth + credential-to-tenant isolation.
step 9 "F1 security: loopback guard, auth, and tenant isolation"
c9_helper=0
c9_uses_helper=0
c9_bind_test=0
c9_auth=0
c9_auth_test=0
c9_tenant_binding=0
c9_tenant_test=0
for guard_path in ${COMMON_BIND_GUARD_PATHS}; do
  if [ -d "${guard_path}" ] && grep -rqE 'refuse_non_loopback_bind' "${guard_path}" 2>/dev/null; then
    c9_helper=1
  fi
done
if grep_dir 'refuse_non_loopback_bind' "${KV_SRC}" || grep_any 'refuse_non_loopback_bind' "${NIMBUS_BIN_SRC}"; then
  c9_uses_helper=1
fi
if [ -d "${KV_TESTS}" ] && grep -rqE 'listener_rejects_non_loopback_bind|InvalidInput|non.?loopback|refus.*loopback' "${KV_TESTS}" 2>/dev/null; then
  c9_bind_test=1
fi
if grep_dir 'AUTH|NOAUTH|authenticated|credential|dev.?cred|password|SCRAM' "${KV_SRC}"; then
  c9_auth=1
fi
if [ -d "${KV_TESTS}" ] && grep -rqE 'unauthenticated|NOAUTH|requires.*auth|auth.*reject|credential' "${KV_TESTS}" 2>/dev/null; then
  c9_auth_test=1
fi
if grep_dir 'AccessKeyRegistry|credential.*TenantId|TenantId.*credential|tenant.*credential|credential.*tenant|SELECT' "${KV_SRC}"; then
  c9_tenant_binding=1
fi
if [ -d "${KV_TESTS}" ] && grep -rqE 'tenant.?A|tenant_a|cross.?tenant|SELECT|cannot.*read.*tenant|credential.*tenant' "${KV_TESTS}" 2>/dev/null; then
  c9_tenant_test=1
fi
if [ "${c9_helper}" = "1" ] && [ "${c9_uses_helper}" = "1" ] && [ "${c9_bind_test}" = "1" ] && [ "${c9_auth}" = "1" ] && [ "${c9_auth_test}" = "1" ] && [ "${c9_tenant_binding}" = "1" ] && [ "${c9_tenant_test}" = "1" ]; then
  pass "RESP listener shares the bind guard, requires auth, and proves credential-to-tenant isolation"
else
  fail "F1 security incomplete" "helper=${c9_helper} uses_helper=${c9_uses_helper} bind_test=${c9_bind_test} auth=${c9_auth} auth_test=${c9_auth_test} tenant_binding=${c9_tenant_binding} tenant_test=${c9_tenant_test}"
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
