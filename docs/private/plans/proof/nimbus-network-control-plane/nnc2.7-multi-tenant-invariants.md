# NNC2.7 Multi-Tenant Network Invariant Proof

Date: 2026-07-24

Status: `passed`

Source commit before the item:
`b2cb26e7ac57e563bc9be5dc1a7be2b50d7834a2`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

The completed multi-tenant network guarantees remain intact after extracting
portable segment authority to `nimbus-network`:

- node and tenant segments remain disjoint and carry stable identities;
- exhaustion and stale epochs fail closed without authority mutation;
- placement searches all existing blocks before CAS-fenced growth;
- orphan discovery quarantines incomplete evidence rather than freeing it;
- concurrent acquire/release, growth, and finalization converge exactly once;
- container and krun both use the same hold, quarantine, detach, release, reap,
  and identity-fenced finalization choreography;
- a current privileged Linux run exercises the real Netavark/nft/container
  provider path; and
- the existing real-KVM cross-tenant and grown-block proofs remain the
  KVM-specific evidence. The local non-KVM host is reported as unavailable,
  never as a passing provider run.

## Stale-Verifier Fail-Before

The tracked aggregate verifier initially exited `1` with 10 passed and 6
failed conditions:

```text
bash scripts/verify-multi-tenant-network.sh
# 10 passed, 6 failed
```

All six failures were stale structural authority, not failed behavior:

1. portable segment vocabulary was still expected in `nimbus-core`;
2. the allocator trait and `segments.json` were still expected in
   `nimbus-sandbox`;
3. teardown expected the deleted `ReleaseOutcome::TenantDrained`;
4. backend wiring expected direct bridge-reaper calls instead of the
   quarantine/release/finalize composition;
5. orphan reconciliation still claimed netns absence reclaimed an allocation;
6. the cluster check searched the old sandbox trait and pre-NNC2.6 test name.

The verifier now anchors on the canonical `nimbus-network` contract, checks
container and krun call sites independently, requires non-creating inspection
and orphan quarantine, requires CAS growth, and proves the live-create versus
durable-cleanup cluster split. It passes:

```text
bash scripts/verify-multi-tenant-network.sh
# 16 passed; 0 failed
```

The KVM rows also require an asserted proof-host guard. The static condition
rejects restoration of the old early-return helper that could report an
explicitly selected provider test as passed without booting a guest.

## Current Behavioral Matrix

The focused current-branch contract suite ran 74 selected tests:

```text
timeout 900 cargo test -p nimbus-sandbox --lib \
  'backends::oci::network::' -- --nocapture
# 71 passed; 0 failed; 3 ignored
```

The three ignored cases are the two named NNC0.7 fail-before cases owned by
NNC5.2a/NNC8.3 and the explicit NNC0.9 scale characterization. They are not
counted as NNC2.7 evidence.

The 71 executed tests include:

| Invariant | Named current tests |
| --- | --- |
| Disjointness | `disjoint_node_supernets_never_alias_segment_identity_and_restart_stably`, `two_nodes_with_disjoint_leases_carve_disjoint_tenant_subnets`, `per_tenant_segments_give_distinct_subnets_so_two_tenants_never_collide` |
| Exhaustion | `exhaustion_fails_closed`, `grow_block_fails_closed_at_pool_exhaustion`, `segment_exhausted_node_is_rejected_fail_closed` |
| Existing-block reuse and growth | `placement_must_reuse_free_capacity_in_an_existing_secondary_block`, `placement_reuses_any_free_existing_block_before_growth`, `placement_grows_onto_a_new_block_when_the_first_is_full`, `grown_block_allocates_within_its_own_subnet_not_the_shared_cursor` |
| Epoch fencing | `a_stale_epoch_carve_fails_closed_on_load`, `stale_epoch_rejects_every_create_and_growth_entrypoint_without_mutation`, `reclaimed_supernet_new_epoch_fails_closed_until_recarve` |
| Orphan/cleanup safety | `reconcile_orphans_quarantines_leaked_holds_without_reusing_allocations`, `reconcile_quarantines_holds_whose_netns_is_gone_and_keeps_live_ones`, `failed_bridge_cleanup_must_fence_segment_from_reuse` |
| Thread concurrency | `concurrent_acquire_release_across_threads_stays_consistent_under_the_lock`, `two_growers_from_one_observation_append_exactly_one_block`, `concurrent_exhaustion_grows_only_the_required_block_set`, `concurrent_finalization_releases_one_identity_exactly_once` |

The portable state/identity suite and placement-capacity consumer are also
green:

```text
timeout 600 cargo test -p nimbus-network --all-features
# 63 passed; 0 failed; 0 ignored

timeout 600 cargo test -p nimbus-workloads scheduling -- --nocapture
# 2 passed; 0 failed; 0 ignored
```

The full Darwin sandbox suite remains green:

```text
timeout 900 cargo test -p nimbus-sandbox
# library: 269 passed; 0 failed; 10 ignored
# helper binary: 2 passed; 0 failed; 0 ignored
# Linux-only integration binaries: 0 executable cases on Darwin
```

The ten ignored library cases remain named later-NNC fail-before tests, child
roles, and explicit scale characterizations. The Darwin zero-test integration
binaries are recorded only as platform information, not provider evidence.

## Live Netavark Provider Proof

A dedicated privileged `ubuntu:24.04` container ran on Docker LinuxKit
`aarch64`. The source tree was mounted read-only and Linux artifacts were kept
in a separate named volume. The environment used:

```text
netavark 1.4.0
aardvark-dns 1.4.0
buildah 1.33.7
conmon 2.1.10
crun 1.14.1
nftables 1.0.9
Rust 1.97.1 stable
```

The first provider run failed both cases with Netavark's
`No such file or directory` diagnostic because the minimal Ubuntu container,
unlike the hosted runner image, did not preinstall Netavark's `iptables`
helper. Installing `iptables` made the environment match the declared provider
contract. A fresh state root then passed both real cases:

```text
timeout 600 cargo test -p nimbus-sandbox \
  --test container_linux_egress \
  -- --ignored --nocapture --test-threads=1
# 2 passed; 0 failed; 0 ignored
```

`container_execute_mode_denies_direct_external_egress` proves a real container
starts through Buildah/conmon/crun/Netavark and direct guest egress is denied.
`container_execute_mode_enforces_proxy_policy_and_live_reload` proves the
positive allowed path, L7 denial, loopback-default denial, direct-bypass denial,
policy reload, newly allowed path, old-path withdrawal, internal-DNS denial,
and cleanup through the real provider effects.

## KVM Evidence Without False Green

The Darwin host and its Docker LinuxKit guest both lack `/dev/kvm`. The
historical proof host name `minicloud` is no longer resolvable from this
environment. Therefore this item does not claim that the current worktree ran a
microVM.

The KVM-specific preservation evidence is durable in the commits that
introduced the unchanged required test bodies:

- `67201a51a784529e40402f09e2be3779aaac3041` records a real `/dev/kvm`
  two-tenant run with `own_egress=allowed` and
  `cross_tenant_reach=denied`.
- `2953925849fb39d6b21181dbe8e61bed47ce491b` records a real `/dev/kvm`
  grown-block run with `own_egress=allowed`,
  `sibling_pep_reach=denied`, and both block bridges present.

Before the NNC2.7 proof-host cleanup, the required test source had no committed
change after `295392584`; current NNC2 allocation/growth behavior is covered by
the named current tests above, and its shared OCI provider path is exercised by
the current live Netavark run. This preserves the KVM-specific proof rather
than pretending to recreate KVM on LinuxKit.

NNC2.7 found that all six ignored KVM tests used:

```text
if !egress_proof_preconditions_met() { return; }
```

On LinuxKit without `/dev/kvm`, an explicit run printed “skipping” but Cargo
reported:

```text
1 passed; 0 failed
```

The harness now asserts root plus readable/writable `/dev/kvm` access. A normal
Linux run exercises the host-independent classifier:

```text
cargo test -p nimbus-sandbox --test krun_linux_egress
# 1 passed; 0 failed; 6 ignored
```

An explicitly selected provider case on the invalid host now exits `101` with:

```text
KVM proof precondition failed: /dev/kvm must exist and be readable/writable
... must fail, never report a skipped lane as passed
```

Linux strict Clippy for the complete integration target passes:

```text
cargo clippy -p nimbus-sandbox --test krun_linux_egress -- -D warnings
# exit 0; only pre-existing vendored Brotli warnings
```

## Quality, Dependency, And Documentation Gates

```text
bash -n scripts/verify-multi-tenant-network.sh
shellcheck scripts/verify-multi-tenant-network.sh
cargo fmt --all --check
git diff --check

timeout 1200 cargo check -p nimbus-network -p nimbus-sandbox \
  -p nimbus-workloads --all-targets --all-features

timeout 1200 cargo clippy -p nimbus-network -p nimbus-sandbox \
  -p nimbus-workloads --all-targets --all-features -- -D warnings

timeout 300 cargo doc -p nimbus-network --no-deps
# all exit 0; only pre-existing vendored Brotli warnings are emitted
```

Dependency metadata still reports the exact allowed workspace edge:

```json
[{"name":"nimbus-core","kind":null}]
```

Static and documentation gates:

```text
bash scripts/verify-nimbus-network-control-plane.sh --self-test
# 15 passed; 0 failed

bash scripts/verify-nimbus-network-control-plane.sh
# 14 passed; 1 failed; exit 1 exactly as expected
# only NNCV005, the deliberately later NNC3 port-authority migration

bash scripts/check-docs.sh
# 108 pages link-clean; source map resolves; private fence intact; titles unique

bash scripts/verify-nimbus-docs-site.sh
# 17/17 conditions green
```

## Ownership And Recovery

`nimbus-network` remains transport- and provider-free. Netavark, nftables,
namespace, KVM, and runtime effects remain in `nimbus-sandbox`; cluster
transport remains outside this allocator seam. The allocator's current
dependency metadata remains the NNC2.6-proven exact
`nimbus-network -> nimbus-core` edge.

The draft sandbox/egress owner plan and older KME proof files exist only as
ignored private files in the original checkout. They were read without
modification and were not copied into this owner plan. Durable Git commit
evidence is used above so recovery does not depend on those hidden files.

The original checkout remains untouched with its four pre-existing user-owned
paths. The isolated Linux test container and its cargo-target volume held only
disposable verification state and were removed after the proof completed. No
push or pull request was performed.

## Independent Review

The repository autoreview workflow reviewed the complete 45,158-byte local
bundle with Claude Opus 4.8 at maximum reasoning:

```text
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local --engine claude --model claude-opus-4-8 \
  --thinking max --stream-engine-output

autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.8)
```

The reviewer independently traced the asserted root/KVM prerequisite, pure
classifier coverage, prior-proof preservation, each strengthened verifier
anchor, both backend call-site checks, quarantine semantics, CAS growth, and
the live-create/durable-cleanup split. It found no actionable blocker.
