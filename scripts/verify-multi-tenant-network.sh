#!/usr/bin/env bash
# Aggregate completion-gate verifier for the Multi-Tenant-Per-Node Network plan
# (`docs/private/plans/multi-tenant-node-network-plan.md`).
#
# Exits 0 iff every landed-band condition holds. Created in MTN4; MTN4..MTN7
# progressively add conditions and flip them FAIL->PASS. Structural (anchors on
# type/trait/test names + call sites, not token presence — the M21 lesson);
# behavioral proof is the crates' own tests (`make test`) and the MTN5 KVM
# cross-tenant deny-proof.
#
# The plan doc lives under untracked docs/private/; this script and the routing
# pointer are the tracked artifacts. Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 2

NET="crates/nimbus-core/src/net.rs"
SEG="crates/nimbus-sandbox/src/backends/oci/network/segment.rs"
LAYOUT="crates/nimbus-sandbox/src/backends/oci/network/layout.rs"
NETAVARK="crates/nimbus-sandbox/src/backends/oci/network/netavark.rs"
NETWORK="crates/nimbus-sandbox/src/backends/oci/network.rs"
CONTAINER_RT="crates/nimbus-sandbox/src/backends/container/runtime.rs"
KRUN_VM="crates/nimbus-sandbox/src/backends/krun/vm.rs"

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
    FAIL_DETAIL+=("$1 — $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
}

# has FILE PATTERN... -> all patterns present
has_all() {
  local file="$1"
  shift
  [ -f "${file}" ] || return 1
  local p
  for p in "$@"; do
    grep -qE "${p}" "${file}" || return 1
  done
  return 0
}

# -------- MTN1: nimbus-core vocabulary --------------------------------------

step 1 "MTN1 nimbus-core Cidr/NetworkSegment/NetworkId vocabulary (zero-I/O)"
if has_all "${NET}" 'pub struct Cidr' 'fn nth_subnet' 'pub struct NetworkSegment' 'pub struct NetworkId'; then
  pass "Cidr + nth_subnet carve + NetworkSegment/NetworkId present"
else
  fail "MTN1 vocabulary missing" "expected Cidr/nth_subnet/NetworkSegment/NetworkId in ${NET}"
fi

# -------- MTN2: allocator seam ---------------------------------------------

step 2 "MTN2 NetworkSegmentAllocator trait + fail-closed SingleNodeSegmentAllocator"
if has_all "${SEG}" 'trait NetworkSegmentAllocator' 'struct SingleNodeSegmentAllocator' \
  'pool is unassigned' 'pool exhausted' 'segments\.json'; then
  pass "allocator trait + single-node impl + fail-closed (unassigned/exhausted) present"
else
  fail "MTN2 allocator incomplete" "expected trait + SingleNodeSegmentAllocator + fail-closed gates in ${SEG}"
fi

# -------- MTN3: tenant-threaded per-tenant segments (M1 dead) ---------------

step 3 "MTN3 network_config(tenant) resolves a per-tenant segment in both backends"
if grep -qE 'fn network_config\(&self, tenant' "${CONTAINER_RT}" \
  && grep -qE 'fn network_config\(&self, tenant' "${KRUN_VM}"; then
  pass "both backends thread tenant through network_config"
else
  fail "network_config not tenant-parameterized" "expected fn network_config(&self, tenant ...) in both backends"
fi

step 4 "MTN3 per-tenant netavark network_id (no DEFAULT_NETWORK_ID alias)"
if grep -qE 'pub network_id: String' "${LAYOUT}" \
  && grep -qE 'id: config\.network_id\.clone\(\)' "${NETAVARK}" \
  && ! grep -qE 'id: DEFAULT_NETWORK_ID' "${NETAVARK}"; then
  pass "OciNetworkConfig.network_id threaded into build_bridge_network"
else
  fail "network_id not wired" "expected network_id field + build_bridge_network consuming config.network_id"
fi

step 5 "MTN3 M1 collision test proves DISTINCT per-tenant subnets"
if has_all "${NETWORK}" 'per_tenant_segments_give_distinct_subnets' '10\.0\.0\.2' '10\.0\.1\.2' \
  && ! grep -qE 'each tenant gets an independent network/IPAM namespace' "${NETWORK}"; then
  pass "collision test asserts distinct 10.0.0.2 vs 10.0.1.2 (old both-10.89.0.2 assertion removed)"
else
  fail "M1 proof still vacuous" "expected distinct-subnet test + removal of the old shared-10.89.0.2 assertion"
fi

# -------- MTN4: crash-safe reaper + manifest persistence + legacy purge -----

step 6 "MTN4 allocator refcounts live sandboxes + frees index on last release"
if has_all "${SEG}" 'enum ReleaseOutcome' 'TenantDrained' 'fn acquire' 'fn release'; then
  pass "acquire/release refcount + ReleaseOutcome::TenantDrained present"
else
  fail "MTN4 reaper refcount missing" "expected acquire/release + ReleaseOutcome::TenantDrained in ${SEG}"
fi

step 7 "MTN4 the resolved network config is persisted in the manifest"
if grep -rqE 'network_config: OciNetworkConfig' crates/nimbus-sandbox/src/backends/container/ \
  && grep -rqE 'network_config: OciNetworkConfig' crates/nimbus-sandbox/src/backends/krun/; then
  pass "both manifests persist the resolved OciNetworkConfig (teardown never re-assigns)"
else
  fail "manifest does not persist the segment" "expected network_config: OciNetworkConfig on both manifests"
fi

step 8 "MTN4 reaper WIRED into the backends (acquire hold, reap on drain, legacy purge)"
# Anchor on CALL SITES in the backend start/teardown (container/ + krun/), not the
# reaper.rs definitions under oci/network/ — a defined-but-unwired reaper is vacuous.
CB="crates/nimbus-sandbox/src/backends/container/ crates/nimbus-sandbox/src/backends/krun/"
if grep -rqE '\.acquire\(&' ${CB} \
  && grep -rqE 'ReleaseOutcome::TenantDrained' ${CB} \
  && grep -rqE 'reap_tenant_bridge\(&' ${CB} \
  && grep -rqE 'purge_legacy_nimbus0_once\(' ${CB}; then
  pass "acquire hold + ReleaseOutcome::TenantDrained -> reap_tenant_bridge + legacy purge wired in both backends"
else
  fail "MTN4 reaper not wired into the backends" "expected .acquire + ReleaseOutcome::TenantDrained + reap_tenant_bridge + purge_legacy_nimbus0_once call sites in container/ + krun/"
fi

# -------- MTN5: host-side inter-tenant isolation + DNS-off -----------------

step 9 "MTN5 every tenant bridge sets the netavark isolate option (FORWARD DROP)"
if grep -qE 'NETAVARK_OPTION_ISOLATE' "${NETAVARK}" \
  && grep -qE 'build_bridge_network_isolates' "${NETWORK}"; then
  pass "build_bridge_network sets isolate=true + a test proves it"
else
  fail "MTN5 isolate not wired" "expected the isolate option in build_bridge_network + a test"
fi

step 10 "MTN5 DNS-off on BOTH backends (no in-subnet aardvark resolver)"
if grep -qE 'enable_dns: false' "${CONTAINER_RT}" \
  && grep -qE 'enable_dns: false' "${KRUN_VM}" \
  && ! grep -qE 'enable_dns: true' "${CONTAINER_RT}" "${KRUN_VM}"; then
  pass "both backends resolve names via the PEP (enable_dns=false)"
else
  fail "MTN5 DNS not off on both backends" "expected enable_dns: false in both network_config methods"
fi

# -------- summary ----------------------------------------------------------

printf '\n\033[1m========= multi-tenant-network verifier =========\033[0m\n'
printf '  %d passed, %d failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -gt 0 ]; then
  printf '\n  Outstanding:\n'
  for d in "${FAIL_DETAIL[@]}"; do
    printf '   - %s\n' "${d}"
  done
  exit 1
fi
exit 0
