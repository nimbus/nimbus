# NNC4.6a Backend Network-Root Separation

Date: 2026-07-28

Status: `complete`

Source commit before the item:
`2bf1b62520be9bea9eef77ae6e45eb3ae0e21663`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Unit Of Value

NNC4.6a separates two authorities that container and krun currently conflate:

```text
workload_state_root
  -> manifests, conmon state, netns pins/status, trust anchors,
     decision logs, quotas, volumes, and provider artifacts

network_state_root
  -> the one serialized node authority containing segment allocations,
     tenant IPAM, and host-global port leases
```

The separation is structural and transport-free. Provider effects remain in
`nimbus-sandbox`; the portable store remains in `nimbus-network`. NNC4.6b owns
selecting and injecting one production node root across start/compose/machine/
KV/server. NNC4.6c owns the mechanical production constructor/root census.

Direct constructors keep one explicit deterministic behavior: when the caller
supplies only a backend root, workload and network state both resolve to its
`state` child. Callers that know the node root use the explicit
`with_network_state_root` builder. There is no legacy field, serde alias, or
fallback heuristic.

## Audit Result

### Current ownership

```text
ContainerSandboxBackendConfig.state_root
KrunSandboxBackendConfig.state_root
  ├─ workload manifests and conmon/provider artifacts
  ├─ ConfiguredSegmentAllocator
  ├─ OciNetworkLayout artifacts and IPAM authority
  ├─ OciPortLeaseCoordinator
  ├─ EgressProxyRegistry durable network state
  └─ startup manifest + network reconciliation
```

`OciNetworkLayout.state_root` has the same conflation: it derives workload
artifact paths and is also opened as the tenant-IPAM durable authority.
`reconcile_terminal_container_ipam_releases` enumerates manifests and opens
IPAM through one parameter. Container runner execution persists only that one
root and validates it against the conflated layout.

### Target ownership

```text
backend config
  ├─ workload_state_root
  └─ network_state_root
       |
       +-> ConfiguredSegmentAllocator
       +-> OciPortLeaseCoordinator
       +-> LocalNetworkStateStore tenant IPAM
       +-> EgressProxyRegistry network-state handle

OciNetworkLayout
  ├─ workload_state_root -> artifact paths
  └─ network_state_root  -> portable authority

container runner config
  ├─ workload_state_root
  └─ network_state_root
```

Startup reconciliation must therefore accept both roots: enumerate and
authenticate lifecycle manifests under the workload root, then compare-delete
only the exact terminal IPAM evidence under the network root.

Krun has no separate runner DTO. Its persisted `OciNetworkLayout` is the root
witness. Every parsed krun manifest must match the backend's exact configured
workload and network roots before inspection can lead to provider execution or
cleanup.

### Durable tenant-prefix defect

`SegmentState` currently authenticates `supernet_cidr` and `supernet_epoch`,
but not the `tenant_prefix`. Reopening durable slot zero as `/25` silently
reinterprets a live `/24` allocation as `10.0.0.0/25`. The prefix becomes a
required durable authority field:

- empty state may adopt the configured prefix on first allocation;
- non-empty state without a prefix fails closed;
- stored and requested prefixes must match on inspect, acquire, growth,
  cleanup, and restart;
- diagnostics name both `/stored` and `/requested`;
- there is no migration shim because Nimbus is pre-launch.

## Frozen Acceptance

| ID | Verifiable success criterion |
| --- | --- |
| R1 | Container and krun configs expose only explicit `workload_state_root` and `network_state_root` concepts; the ambiguous config `state_root` field is deleted. |
| R2 | `under_root` and two-argument `plan_only` deterministically set both roots to the same workload-state path; `with_network_state_root` is the sole explicit split operation. |
| R3 | Container manifests, conmon state, netns/status files, trust anchors, decision logs, quotas, and tenant artifacts remain under `workload_state_root`. |
| R4 | Krun manifests, conmon state, netns/status files, trust anchors, decision logs, quotas, volumes, and tenant artifacts remain under `workload_state_root`. |
| R5 | Container and krun segment allocation, tenant IPAM, port leases, and egress PEP lease state use `network_state_root`; distinct roots create no network control-plane authority under the workload root. |
| R6 | `OciNetworkLayout` persists both exact roots and derives artifacts only from the workload root while IPAM/finality use only the network root. |
| R7 | Container runner execution config serializes and reconstructs both exact roots. Substituting either root fails before backend construction and before Netavark, PEP, socket, pointer removal, launch-artifact cleanup, or durable lease mutation. |
| R8 | In-process container manifest reads/writes authenticate both roots and exact tenant-qualified layouts before effects. |
| R9 | Krun manifest reload authenticates both configured roots before any provider execution, restart, detach, or cleanup effect. |
| R10 | Startup terminal-IPAM reconciliation enumerates trusted manifests under the workload root and mutates only the separately supplied network authority. Cross-root/substituted layouts fail without mutation. |
| R11 | Durable segment state records `tenant_prefix`; a changed prefix fails closed with stored/requested diagnostics before any CIDR reinterpretation or mutation. |
| R12 | Focused happy/edge/error tests, full sandbox tests, affected check/strict Clippy/rustdoc, format/diff, dependency/effect verifier, and docs gates pass with exact counts; `nimbus-network -> nimbus-core` remains the only network workspace edge. |

## Exact Fail-Before

### Prefix substitution

The first command was incorrectly invoked with `--exact` and a non-qualified
test name. It executed zero tests and is not evidence:

```text
cargo test -p nimbus-sandbox \
  durable_segment_authority_rejects_tenant_prefix_substitution \
  -- --exact --nocapture
=> 0 passed; 718 filtered out
```

The corrected command executed the named test:

```text
timeout 600 cargo test -p nimbus-sandbox \
  durable_segment_authority_rejects_tenant_prefix_substitution \
  -- --nocapture
=> exit 101; 0 passed; 1 failed; 717 filtered out
```

Failure was exact:

```text
durable /24 state must not be reinterpreted through a /25 allocator
actual: slot zero was returned as 10.0.0.0/25
```

### Missing root seam

Tests for both backend configs were added before implementation:

```text
timeout 600 cargo test -p nimbus-sandbox root_ownership --no-run
=> exit 101
```

All six errors were the intended missing contract:

- absent container `workload_state_root`;
- absent container `network_state_root`;
- absent container `with_network_state_root`;
- absent krun `workload_state_root`;
- absent krun `network_state_root`;
- absent krun `with_network_state_root`.

No unrelated compile error was present. Known vendored Brotli warnings remain
outside the touched owner.

## Implementation Bands

1. Replace the two config fields and deterministic constructors; mechanically
   classify every old config `state_root` use as workload or network.
2. Split `OciNetworkLayout`, IPAM/finality, and startup reconciliation
   parameters. Keep the layout a small persisted DTO, not a manager.
3. Persist/authenticate both container runner roots and both container/krun
   manifest contexts before effects.
4. Add durable segment-prefix authentication and corruption/missing-field
   negatives.
5. Run a source-derived old-field/root census and the full affected gates.

No band is complete until its behavior proof is green. No structured review
runs on a partial band.

## Implementation Result

The backend configs now name only `workload_state_root` and
`network_state_root`. Their direct constructors retain the deliberate
same-root default, while `with_network_state_root` is the one explicit split
operation. `OciNetworkLayout` persists both exact roots: every artifact path is
derived from the workload root and every IPAM/finality store operation opens
the network root.

Container runner manifests serialize both roots. Runner admission and every
in-process manifest read compare the configured roots, runner roots, exact
tenant-qualified network layout, and conmon layout before constructing a
backend or entering provider/cleanup code. Krun uses a concept-owned
`root_authentication` module; every reopened manifest is authenticated before
inspection can trigger its current restart/provider behavior.

Startup reconciliation now takes both roots explicitly. It enumerates
manifests under workload state, authenticates the embedded layout against both
configured roots, and compare-deletes only exact terminal IPAM evidence in the
network authority. Segment state now persists `tenant_prefix` beside
`supernet_cidr` and `supernet_epoch`; every typed read and transaction rejects
a stored/requested mismatch or a non-empty missing-prefix state without
changing authority bytes.

### Frozen-acceptance disposition

| Criterion | Result |
| --- | --- |
| R1-R2 | Both backend config types expose only the two named roots. `under_root`, `Default`, and two-argument `plan_only` choose one deterministic same-root layout; the builder is the sole split seam. |
| R3-R4 | Manifest, conmon, provider-marker, netns/status, image/rootfs, trust-anchor, decision-log, quota, volume, and tenant cleanup paths route through `workload_state_root`. |
| R5 | Segment allocation, tenant IPAM, host ports, and the egress registry's lease state route through `network_state_root`. Split-root container/krun tests prove no portable authority is created under workload state. |
| R6 | `OciNetworkLayout::with_roots` persists both roots; its same-root convenience is compiled only for tests. Production builds therefore cannot silently reconstruct a conflated layout. |
| R7-R8 | The runner DTO round-trips both roots. Workload-root and network-root substitution each fail before backend construction, authority mutation, provider effects, pointer removal, rootfs cleanup, or trust-anchor cleanup. In-process reads/writes run the same exact context authentication. |
| R9 | Krun reload authenticates sandbox identity, tenant identity, both layout roots, and conmon layout before lifecycle/provider work. A real executable probe proves a substituted network root produces no runtime effect. |
| R10 | Split-root startup reconciliation finds manifests only in workload state, mutates only network authority, is idempotent, and rejects copied/foreign layouts without mutation. |
| R11 | The original `/24`→`/25` fail-before now passes. Inspect, assignment read, acquire, growth, restart reconciliation, cleanup inspect, and cleanup reconcile all reject substitution with stored/requested diagnostics; missing-prefix non-empty state also fails byte-stably. |
| R12 | Focused, full affected, quality, dependency/effect, formatting, and docs evidence is recorded below. |

### Owner-inspection correction

The final manual root-routing census found two provider-cleanup assertions that
still opened port records through `workload_state_root`. They passed only
because those fixtures used the deterministic same-root default. The
assertions now use `network_state_root`, and both provider-drift fixtures use
explicit split roots so the tests would fail on any recurrence. An attempted
`--exact` invocation with a non-qualified Rust test name ran zero tests and is
rejected; the corrected commands each ran and passed one test.

### Structured item review and accepted corrections

The one full item review ran over the candidate-frozen executable diff with
GPT-5.6 Sol, xhigh reasoning, and fast mode. It reported two findings, both
accepted after source-grounded reproduction:

1. The startup segment-orphan census was incorrectly given
   `network_state_root`. Persistent netns evidence is a workload artifact, so
   a split-root backend treated a live attachment as an orphan and
   quarantined/fenced it at every restart. The review's stronger claim that
   this immediately reclaimed and reassigned the CIDR is not supported by the
   implementation; quarantine is nevertheless a material availability and
   lifecycle-integrity defect. The new split-root test failed before the fix
   at exit 101 (0 passed, 1 failed, 725 filtered) and passes after the census
   reads `workload_state_root` (1 passed, 0 failed, 725 filtered).
2. A krun launch-compensation fixture called `claim_bind_attempts` through
   `workload_state_root`. Its same-root default masked the error. After making
   the fixture explicitly split-root, the exact test failed before the fix at
   exit 101 because the expected network-root lease did not exist (0 passed,
   1 failed). Passing `network_state_root` now makes the exact test pass
   (1 passed, 0 failed).

The correction-class sibling census found no remaining segment, IPAM, port, or
PEP authority construction through `workload_state_root`. Its only workload
root matches are deliberate negative assertions that no authority exists
there and the segment-orphan census itself, which must read workload-owned
netns evidence. Startup reconciliation passes 12/12 and the complete krun
launch-compensation slice passes 23/23 after the fixes.

Because the first correction changes production root routing, the item permits
exactly one narrow correction review after its affected acceptance gates are
green. That review is limited to these two accepted defects and their
regressions; it does not reopen unrelated NNC4.6a design.

The one permitted narrow correction review then ran with GPT-5.6 Sol, xhigh
reasoning, and fast mode. It reported zero findings and judged the patch
correct at 0.98 confidence. It independently confirmed that startup orphan
reconciliation reads workload-owned netns evidence while all portable
authority remains on `network_state_root`, and that the explicitly split krun
fixture claims PEP authority through that network root. No further NNC4.6a
review is warranted.

### Modularity threshold disposition

No file was split mechanically for this extraction. Changed files in the
1,500-1,999-line review band remain cohesive owners:

| File | Lines | Ownership justification |
| --- | ---: | --- |
| `container/runtime.rs` | 1,594 | Existing container backend composition/state-machine owner; NNC4.6a adds only explicit routing at established call sites, while new authentication logic remains in child modules. |
| `container/runtime/planning.rs` | 1,658 | Concept-owned planning/fail-before matrix; the split-root proof extends an existing runner-manifest test rather than adding another proof group. |
| `container/runtime/tests/provider_cleanup.rs` | 1,526 | Concept-owned cleanup reliability matrix; the two split-root drift cases belong with the provider-context guarantees they exercise. |
| `krun/vm/lifecycle.rs` | 1,879 | Existing deep krun lifecycle state machine; new root comparison logic is isolated in the 65-line `root_authentication.rs` child. |
| `oci/network/ipam.rs` | 1,886 | One IPAM transaction/reconciliation owner with colocated crash and authority tests; the root split changes its authority parameter without creating a second IPAM seam. |

The 1,360-line segment test child and 1,322-line segment owner remain below the
review threshold. The new krun root proof is a separate concept-owned child and
does not deepen the already-large inline krun test owner.

## Modularity And Seam Constraints

- Do not add provider effects or a provider interface to `nimbus-network`.
- Do not move workload artifacts into the node network root.
- Do not move port/IPAM/segment authority back into a project root.
- Keep Egress PDP/PEP, Netavark, nftables, gvproxy, sockets, and cleanup effects
  in their current owners.
- Do not add serde aliases, duplicate legacy fields, or path-guessing shims.
- Add new root tests in concept-owned children. The krun inline test owner is
  already 1,987 lines and must not receive another proof group.
- The 1,626-line container planning matrix receives no new root proof group.
- Prefer one root-validation function per backend and call it from deep
  read/publish/provider-context seams rather than duplicating comparisons.

## Final Proof Commands

Focused tests will include:

```text
cargo test -p nimbus-sandbox root_ownership -- --nocapture
cargo test -p nimbus-sandbox tenant_prefix_substitution -- --nocapture
cargo test -p nimbus-sandbox substituted_execution_context -- --nocapture
cargo test -p nimbus-sandbox startup_reconciliation -- --nocapture
```

Closeout gates:

```text
cargo test -p nimbus-sandbox --all-features -- --test-threads=1
cargo check -p nimbus-sandbox --all-targets --all-features
cargo clippy -p nimbus-sandbox --all-targets --all-features \
  --no-deps -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-sandbox --no-deps --all-features
cargo metadata --format-version 1 --no-deps
bash scripts/verify-nimbus-network-control-plane.sh
cargo fmt --all --check
git diff --check
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
```

Exactly one GPT-5.6 Sol, xhigh, fast structured review runs only after R1-R12
and every closeout gate are green on the candidate-frozen item. An accepted
material executable finding permits one narrow correction review after its
affected proofs rerun.

## Current Checkpoint

| Field | Value |
| --- | --- |
| Owned paths | Canonical plan/routing/this proof; container and krun config/root composition; OCI layout/IPAM/startup reconciliation; container runner root persistence; krun manifest root validation; segment durable prefix and focused tests. |
| Frozen executable diff | Corrected executable SHA-256 `26bad585c1365a23c7809034657e5bf727f51754525bba82266676ec6f071400` over the staged `crates/nimbus-cli` and `crates/nimbus-sandbox` binary diff. The reviewed pre-correction digest was `7df7fb62cd09b0a3fc4ca594384d56492a8923d198dc11d2b063401fd6cfed38`. Documentation-only checkpoint edits are outside both digests. |
| Last green | R1-R12; focused roots 5/0/0, prefix 3/0/0, substitutions 2/0/0, split-root provider drift 2/0/0, CLI compose 43/0/0, accepted fixes 1/0/0 each, startup reconciliation 12/0/0, and krun launch compensation 23/0/0. The exact corrected full sandbox run is 706 executed/0 failed/24 ignored. Affected all-target check, combined strict Clippy, warning-denied sandbox rustdoc, metadata/core-only dependency, live verifier 15/15 plus prior unchanged self-test 45/45, format/diff, docs 108, and site 17/17 pass. |
| Fail-before | Prefix bug: 0/1/0 at exit 101 after correcting a zero-test invocation. Root seam: compile exit 101 with exactly six absent API errors. Review P1: live split-root netns 0/1/0 at exit 101. Review P2: split-root krun PEP claim 0/1/0 at exit 101. |
| Next | NNC4.6b's read-only production composition/root substitution audit begins only after this exact proof and item checkpoint commit. |
| Blocker | None. |
