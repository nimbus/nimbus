#!/usr/bin/env bash
# Aggregate completion-gate verifier for the multi-tenant network invariants
# now preserved by the canonical Nimbus network control-plane plan
# (`docs/private/plans/nimbus-network-control-plane-plan.md`).
#
# Exits 0 iff every landed-band structural condition holds. This verifier was
# created in MTN4 and is retained as a regression guard through the
# nimbus-network extraction. It anchors on canonical type/trait/test names and
# concrete call sites, not loose token presence (the M21 lesson). Behavioral
# proof remains in the named Rust tests plus the live KVM/Netavark lanes.
#
# Run from the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 2

NET="crates/nimbus-core/src/net.rs"
NETWORK_IDENTITY="crates/nimbus-network/src/identity.rs"
NETWORK_SEGMENT="crates/nimbus-network/src/segment.rs"
SEG="crates/nimbus-sandbox/src/backends/oci/network/segment.rs"
LAYOUT="crates/nimbus-sandbox/src/backends/oci/network/layout.rs"
NETAVARK="crates/nimbus-sandbox/src/backends/oci/network/netavark.rs"
NETWORK="crates/nimbus-sandbox/src/backends/oci/network.rs"
CONTAINER_RT="crates/nimbus-sandbox/src/backends/container/runtime.rs"
KRUN_VM="crates/nimbus-sandbox/src/backends/krun/vm.rs"
KRUN_LIFECYCLE="crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs"
EGRESS_PROOF="crates/nimbus-sandbox/tests/krun_linux_egress.rs"
REAPER="crates/nimbus-sandbox/src/backends/oci/network/reaper.rs"
SCHEDULING="crates/nimbus-workloads/src/scheduling.rs"
PLACEMENT="crates/nimbus-sandbox/src/backends/oci/network/placement.rs"
KRUN_START="crates/nimbus-sandbox/src/backends/krun/vm/start.rs"
CLUSTER="crates/nimbus-sandbox/src/backends/oci/network/cluster.rs"

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

# -------- MTN1: portable vocabulary -----------------------------------------

step 1 "MTN1 zero-I/O CIDR math in core; stable segment identity in nimbus-network"
if has_all "${NET}" 'pub struct Cidr' 'pub fn nth_subnet' \
  && has_all "${NETWORK_SEGMENT}" 'pub struct AllocatedSegment' \
  && has_all "${NETWORK_IDENTITY}" 'NetworkSegmentId' 'pub struct NetworkLeaseEpoch' \
  && ! grep -qE 'pub struct (NetworkSegment|NetworkId)' "${NET}"; then
  pass "core owns CIDR math; nimbus-network owns stable segment identity + lease epoch"
else
  fail "MTN1 vocabulary ownership is stale" "expected Cidr/nth_subnet only in core and AllocatedSegment/NetworkSegmentId/NetworkLeaseEpoch in nimbus-network"
fi

# -------- MTN2: allocator seam ---------------------------------------------

step 2 "MTN2 portable allocator contract + fail-closed OCI realization"
if has_all "${NETWORK_SEGMENT}" 'pub trait NetworkSegmentAllocator' \
  'fn inspect_segments' 'NetworkAttachmentId' \
  && has_all "${SEG}" 'struct SingleNodeSegmentAllocator' \
  'LocalNetworkStateStore' 'pool is unassigned' 'pool exhausted'; then
  pass "portable allocator contract + non-creating inspection + durable fail-closed OCI implementation present"
else
  fail "MTN2 allocator incomplete" "expected public contract in nimbus-network plus LocalNetworkStateStore-backed fail-closed SingleNodeSegmentAllocator"
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

step 6 "MTN4 allocator quarantines attachments before effect cleanup and finalizes by fence"
if has_all "${NETWORK_SEGMENT}" 'fn acquire' 'fn quarantine' 'fn release' \
  'fn finalize_release' 'pub enum NetworkSegmentReleaseOutcome' \
  'CleanupPending' 'NetworkAttachmentId'; then
  pass "attachment hold + quarantine/release + identity-fenced finalization contract present"
else
  fail "MTN4 teardown authority missing" "expected acquire/quarantine/release/finalize_release with CleanupPending in nimbus-network"
fi

step 7 "MTN4 the resolved network config is persisted in the manifest"
if grep -rqE 'network_config: OciNetworkConfig' crates/nimbus-sandbox/src/backends/container/ \
  && grep -rqE 'network_config: OciNetworkConfig' crates/nimbus-sandbox/src/backends/krun/; then
  pass "both manifests persist the resolved OciNetworkConfig (teardown never re-assigns)"
else
  fail "manifest does not persist the segment" "expected network_config: OciNetworkConfig on both manifests"
fi

step 8 "MTN4 teardown saga WIRED in both backends (hold, quarantine, detach, release, finalize)"
# Anchor on each backend's concrete call sites plus the shared reaper composition.
# An aggregate tree search can pass when only one backend is wired.
if has_all "${CONTAINER_RT}" '\.acquire\(' 'quarantine_network_segment_hold\(' \
  'release_network_segment_hold\(' 'purge_legacy_nimbus0_once\(' \
  && has_all "${KRUN_LIFECYCLE}" '\.acquire\(' 'quarantine_network_segment_hold\(' \
  'release_network_segment_hold\(' 'purge_legacy_nimbus0_once\(' \
  && has_all "${REAPER}" 'NetworkSegmentReleaseOutcome::CleanupPending' \
  'reap_bridge_interface\(segment\.network_interface\(\)\)' \
  'allocator\.finalize_release\(&cleanup\)'; then
  pass "container + krun both hold/quarantine/release; shared reaper cleans all blocks and identity-fences finalization"
else
  fail "MTN4 teardown saga not wired into both backends" "expected hold/quarantine/release/legacy-purge call sites in each backend and reap/finalize composition in reaper.rs"
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

step 11 "MTN5 two-tenant cross-tenant KVM deny-proof present (with positive control)"
if grep -qE 'fn krun_two_tenants_cannot_reach_each_others_sandbox' "${EGRESS_PROOF}" \
  && grep -qE 'own_egress=allowed' "${EGRESS_PROOF}" \
  && grep -qE 'cross_tenant_reach=denied' "${EGRESS_PROOF}" \
  && grep -qE 'fn assert_egress_proof_preconditions' "${EGRESS_PROOF}" \
  && grep -qE 'must fail, never report a skipped lane as passed' "${EGRESS_PROOF}" \
  && ! grep -qE 'egress_proof_preconditions_met' "${EGRESS_PROOF}"; then
  pass "cross-tenant deny-proof has a positive control and fails loudly on a non-KVM host"
else
  fail "MTN5 cross-tenant KVM proof missing" "expected positive/negative controls plus an asserted, non-skipping KVM prerequisite"
fi

# -------- MTN6: startup orphan quarantine -----------------------------------

step 12 "MTN6 startup orphan quarantine WIRED into both backends' new()"
# Netns absence is incomplete deletion evidence: startup may quarantine leaked
# holds, but only the later evidence-aware reconciler may release/finalize them.
if grep -qE 'reconcile_network_segment_orphans\(&config\.state_root' "${CONTAINER_RT}" \
  && grep -qE 'reconcile_network_segment_orphans\(&config\.state_root' "${KRUN_VM}" \
  && grep -qE 'fn reconcile_orphans' "${SEG}" \
  && grep -qE 'reconcile_quarantines_holds_whose_netns_is_gone_and_keeps_live_ones' "${REAPER}" \
  && grep -qE 'absence is only incomplete orphan evidence' "${REAPER}"; then
  pass "both backends quarantine netns-absent holds without manufacturing provider-deletion proof"
else
  fail "MTN6 orphan quarantine not wired" "expected both startup call sites + quarantine test + explicit incomplete-evidence guard"
fi

step 13 "MTN6 remaining-segment dimension on NodeCapacity (fail-closed placement)"
if grep -qE 'remaining_segments' "${SCHEDULING}" \
  && grep -qE 'segment pool is exhausted' "${SCHEDULING}" \
  && grep -qE 'segment_exhausted_node_is_rejected_fail_closed' "${SCHEDULING}"; then
  pass "NodeCapacity.remaining_segments + fail-closed placement when exhausted + test"
else
  fail "MTN6 NodeCapacity segment dimension missing" "expected remaining_segments + fail-closed placement + test in ${SCHEDULING}"
fi

step 14 "MTN6 block-aware placement wired into BOTH backends (CAS-fenced growth)"
# Anchor on the shared placement helper + portable growth contract + call sites
# in both backends + the non-vacuous grow test — a defined-but-unwired placement
# is vacuous (M21).
if grep -qE 'fn place_sandbox_on_block' "${PLACEMENT}" \
  && grep -qE 'placement_grows_onto_a_new_block' "${PLACEMENT}" \
  && grep -qE 'fn grow_block_if_current' "${NETWORK_SEGMENT}" \
  && grep -qE 'place_sandbox_config\(&resolved_spec\.tenant_id' "${CONTAINER_RT}" \
  && grep -qE 'place_sandbox_config\(&resolved_launch\.spec\.tenant_id' "${KRUN_START}"; then
  pass "place_sandbox_on_block + CAS-fenced growth contract + both planning call sites + grow test"
else
  fail "MTN6 multi-block placement not wired" "expected place_sandbox_on_block + grow_block_if_current + both planning call sites + grow test"
fi

step 15 "MTN6 KVM grow-on-exhaustion proof (tenant-prefix knob + grow test)"
if grep -qE 'node_tenant_subnet_prefix' "${KRUN_VM}" \
  && grep -qE 'fn krun_tenant_grows_onto_a_second_block_when_the_first_is_full' "${EGRESS_PROOF}" \
  && grep -qE 'own_egress=allowed' "${EGRESS_PROOF}" \
  && grep -qE 'sibling_pep_reach=denied' "${EGRESS_PROOF}" \
  && grep -qE 'host_bridge_exists\("nb-1"\)' "${EGRESS_PROOF}" \
  && grep -qE 'grown_block_allocates_within_its_own_subnet' "${NETWORK}"; then
  pass "grow KVM proof: grown block reaches its OWN PEP + is DENIED the sibling PEP (H1) + nb-1 exists + shared-cursor IPAM regression test"
else
  fail "MTN6 KVM grow proof missing" "expected node_tenant_subnet_prefix knob + krun_tenant_grows_onto_a_second_block test with guest_ip/own_egress/nb-1 assertions"
fi

# -------- MTN7: cluster allocator seam (lease-gated, behind the SAME trait) --

step 16 "MTN7 transport-free cluster allocator: live-create lease + durable-cleanup authority"
# The concrete raft-backed ClusterLeaseProvider is the HS lane's impl of the seam;
# allocation/cleanup split + fencing/admission logic + the disjointness invariant
# are built and tested here against an in-memory provider. Cluster transport and
# promotion remain owned by the horizontal-scaling plan.
if grep -qE 'struct ClusterSegmentAllocator' "${CLUSTER}" \
  && grep -qE 'trait ClusterLeaseProvider' "${CLUSTER}" \
  && grep -qE 'fn assert_cluster_admission' "${CLUSTER}" \
  && grep -qE 'fn live_inner' "${CLUSTER}" \
  && grep -qE 'fn cleanup_inner' "${CLUSTER}" \
  && grep -qE 'fn requires_cluster_lease' "${NETWORK_SEGMENT}" \
  && grep -qE 'two_nodes_with_disjoint_leases_carve_disjoint_tenant_subnets' "${CLUSTER}" \
  && grep -qE 'expired_lease_self_fences' "${CLUSTER}" \
  && grep -qE 'no_committed_lease_fails_closed' "${CLUSTER}" \
  && grep -qE 'reclaimed_supernet_new_epoch_fails_closed_until_recarve' "${CLUSTER}" \
  && grep -qE 'expired_lease_must_fence_creation_but_allow_cleanup_of_a_durable_hold' "${CLUSTER}"; then
  pass "cluster allocator separates live creation from durable cleanup and proves lease/epoch fencing + disjointness"
else
  fail "MTN7 cluster seam incomplete" "expected transport-free lease seam, live/create split, durable cleanup, admission fence, and named disjointness/fencing tests"
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
