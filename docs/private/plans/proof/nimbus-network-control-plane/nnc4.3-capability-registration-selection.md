# NNC4.3 Capability Registration And Exact Selection Proof

Status: `complete; R1-R10 green; exact item commit pending`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Scope

NNC4.3 registers source-owned capability facts for the two provider roles
needed by the first local composition:

- sandbox-owned host-managed workload attachment; and
- server-owned local ingress.

It also adds a transport-free registry that selects one explicitly requested,
pre-admitted composition of those roles. This item does not instantiate the
process-wide manager/store (NNC4.6), perform provider effects, infer provider
readiness, compose PEP readiness (NNC4.5), or add a provider trait.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| R1 | `nimbus-network` exposes only closed registration, selection, bundle, registry, mismatch, and error values. The registry contains no callbacks, handles, sockets, async operations, effect traits, environment probes, or upper-crate dependency. Workspace metadata still reports exactly `nimbus-network -> nimbus-core`. |
| R2 | A selection names exactly one attachment registration and one ingress registration by stable `NetworkProviderId`. The registry admits only complete bundles and never manufactures a Cartesian combination from individually known providers. |
| R3 | Registry construction is deterministic and fail closed: identical repeated bundles are idempotent; a provider ID used in both roles, a divergent report under one role/ID, or a divergent duplicate composition returns a typed error. Input order does not change the diagnostic identity. |
| R4 | `select_exact` returns only the requested registered composition. An unknown provider, missing role, or known-but-unregistered pair is rejected. A satisfying alternative is diagnostic evidence only and is never selected automatically. |
| R5 | Requirement ownership is explicit: management/attachment/isolation are checked against the attachment role; endpoint/ingress/forwarding against ingress; address family, lifecycle, and sovereignty are independently checked against both roles. One role cannot mask missing evidence in the other. |
| R6 | Unsatisfied diagnostics preserve fixed capability-dimension order within attachment then ingress role order, name the exact role/provider, and include only complete registered compositions that independently satisfy the same requirements. Alternatives are sorted and deduplicated by attachment then ingress provider ID. |
| R7 | Container and krun expose distinct Execute-only host-managed attachment registrations. `PlanOnly`, unsupported build targets, failed startup reconciliation, and container machine-forwarder mode cannot be advertised as ready/selectable local attachment providers. Registration remains capability evidence, not permission to execute an effect. |
| R8 | The container registration reports isolated namespace plus tenant/workload isolation and IPv4. The krun registration additionally reports VM guest attachment. Neither reports provider-managed networking, host networking, provider virtual networking, provider boundary, or IPv6. Lifecycle/sovereignty facts are limited to source-proven local attachment reconciliation. |
| R9 | Server exposes one Nimbus-owned local-ingress registration using the existing `nimbus-server.tcp-listener` provider identity. It reports host TCP, IPv4/IPv6, loopback/private/public bind classes, exact/provider-assigned ports, path routing, WebSocket, streaming, and TLS only when configured. It does not report host routing, UDP, range allocation, isolated bind realms, forwarding, public publication, certificate issuance, DNS, relay, cloud control plane, or hosted certificate dependencies. |
| R10 | `NetworkPlan` remains provider-neutral: registry membership, bundle order, and exact selection do not alter its digest. Focused happy/edge/error tests, serialization rejection, affected-crate suites, Clippy/check/rustdoc, dependency/effect scans, static verifier, docs gates, format/diff checks, and exactly one candidate-frozen Sol/xhigh/fast structured review all pass with recorded counts. |

## Source-Grounded Ownership Census

### Attachment registrations

The shared OCI network module is an implementation toolkit, not a selectable
provider. Container and krun retain distinct composition roots, manifests,
backend identities, and lifecycle authority:

| Registration | Stable key | Composition root | Admitted facts |
| --- | --- | --- | --- |
| Container host-managed attachment | `nimbus-sandbox.container.host-managed-attachment` | `ContainerSandboxBackendConfig` / `ContainerSandboxBackend` | Execute-only local Netavark attachment; isolated namespace; workload namespace and tenant segment; IPv4. |
| Krun host-managed attachment | `nimbus-sandbox.krun.host-managed-attachment` | `KrunSandboxBackendConfig` / `KrunSandboxBackend` | Execute-only local Netavark attachment; isolated namespace plus VM guest; workload namespace and tenant segment; IPv4. |

The IDs deliberately do not reuse port-effect or operation-attempt identities
such as `nimbus-sandbox.netavark`,
`nimbus-sandbox.machine-port-proxy`,
`nimbus-sandbox.egress-pep`, or
`nimbus-sandbox.oci.netavark-operation`. Those identify narrower provider
effects or individual attempts, not one complete attachment composition.

Both Execute backends create tenant/workload-scoped persistent namespaces,
realize an IPv4-only tenant segment through Netavark, persist generation-fenced
IPAM/provider state, reconcile startup state, and prove detach before releasing
authority. `PlanOnly` carries no network effect authority. Container
machine-forwarder mode adds a different provider composition and remains
unregistered until its owning item can describe that composition honestly.

### Ingress registration

`ServeOptions` and `serve_leased` are the server composition root. Main,
sibling, prebound, and external listeners share one
`ServerListenerLeaseAuthority`, but NNC4.3 registers only the Nimbus-owned local
listener mode:

| Registration | Stable key | Composition root | Admitted facts |
| --- | --- | --- | --- |
| Server local ingress | existing `nimbus-server.tcp-listener` | `ServeOptions` / `serve_leased` | Host TCP; IPv4/IPv6; loopback/private/public bind classes; exact/provider-assigned ports; path/WebSocket/streaming; optional configured TLS. |

Main and sibling listener names remain resource identities below that provider
identity. External inherited descriptors retain provider-managed ownership and
are not silently covered by the Nimbus-owned delete claim. `Public` is only a
host-address exposure class: every current server lease remains
`HostInternal` and `Unpublished`, so the registration is not public-DNS,
load-balancer, tunnel, or cloud-reachability evidence.

## Architectural Decision

The singular NNC4.1 capability report was sufficient to prove the fourteen
dimension matchers, but it cannot truthfully represent a multi-owner network
plan. Sandbox owns attachment effects and server owns ingress effects. Unioning
their sets would allow one owner to mask missing lifecycle or sovereignty
evidence in the other; requiring either individual report to satisfy the whole
plan would make the first local composition impossible.

NNC4.3 therefore uses role-scoped registrations and an explicit admitted
bundle:

```text
NetworkCapabilitySelection
  { attachment_provider_id, ingress_provider_id }
          |
          v exact lookup only
NetworkCapabilityBundle
  { attachment registration, ingress registration }
          |
          v role-scoped satisfaction
NetworkCapabilityRequirements
```

There is no synthetic bundle provider ID and no god provider. The ordered pair
is the stable selection identity. A bundle is compatibility evidence admitted
by the upper composition owner; two separately registered providers never
imply compatibility.

The runtime registry container is not desired state and is not serialized into
`NetworkPlan`. A later durable selected-provider record, if required, needs its
own generation-scoped and domain-separated evidence rather than reusing the
provider-neutral plan digest.

## Exact Requirement Ownership

| Dimension | Required evidence |
| --- | --- |
| Management mode | Attachment registration |
| Attachment mode | Attachment registration |
| Isolation mode | Attachment registration |
| Address family | Both registrations independently |
| Bind realm | Ingress registration |
| Exposure | Ingress registration |
| Protocol | Ingress registration |
| Port assignment | Ingress registration |
| Ingress feature | Ingress registration |
| Forwarding feature | Ingress registration |
| Lifecycle feature | Both registrations independently |
| Control-plane locality | Both registrations independently |
| External dependency | Both registrations independently |
| Offline restart | Both registrations independently |

Empty sets are explicit unsupported facts, never omitted values. The registry
does not infer readiness from a capability registration: local binary/device
availability, TLS file validation, current socket bind success, provider
inspection, and PEP readiness remain owned runtime evidence.

## Failure Matrix

| Case | Required result |
| --- | --- |
| Exact admitted pair satisfies every scoped requirement | Return that exact selection. |
| One role ID is unknown | Typed unregistered-composition error naming the requested pair. |
| Both IDs are known but their pair was not admitted | Typed unregistered-composition error; no synthesized pair. |
| Attachment lacks a required attachment/isolation fact | Attachment-role mismatch even if ingress has an analogous fact. |
| Either role lacks a required address family | That exact role/provider fails. |
| Either role lacks lifecycle/offline evidence | That exact role/provider fails; the stronger half cannot mask it. |
| Ingress lacks endpoint/ingress/forwarding evidence | Ingress-role mismatch. |
| Requested bundle fails while another complete bundle succeeds | Reject requested bundle and list the other as a safe diagnostic alternative. |
| Multiple complete bundles satisfy | Exact request decides; availability order never decides. |
| Same provider ID appears in attachment and ingress roles | Typed role conflict. |
| Same role/provider ID has divergent facts | Typed report conflict. |
| Same selection repeats identically | Idempotent construction. |
| Same selection repeats with divergent facts | Typed conflict. |
| Serialized selection/bundle omits a role or adds a field | Serde rejection. |
| Provider IDs are malformed/noncanonical | Existing typed ID deserialization rejection. |
| Registry order or selection changes | `NetworkPlan::digest()` remains unchanged. |

## Fail-Before Test Packet

Before production source changes, tests will reference the missing registry,
role registration, bundle, and exact-selection APIs and must fail for those
missing contracts rather than for unrelated compilation errors. The packet
will prove:

1. exact container-plus-server and krun-plus-server selections;
2. incomplete and known-but-unregistered pairs;
3. cross-role masking prevention for address family, lifecycle, and
   sovereignty;
4. no automatic alternative selection;
5. complete-only, stable-order alternatives;
6. idempotent duplicates plus cross-role/divergent-report conflicts;
7. canonical serialization and malformed/missing/unknown-field rejection;
8. provider-neutral plan digest;
9. PlanOnly/machine-forwarder/platform/reconciliation registration refusal;
10. distinct container/krun facts and stable IDs;
11. exact conservative server facts with conditional TLS; and
12. static absence of provider traits/effects and forbidden dependency edges.

## Owned Paths

The admitted implementation may touch only:

- `crates/nimbus-network/src/capability.rs` and its concept-owned tests;
- a new concept-owned registry module and tests in `nimbus-network`;
- `crates/nimbus-network/src/lib.rs`;
- the public capability integration test;
- concept-owned sandbox attachment-registration modules plus narrow
  composition-root exports/tests;
- a concept-owned server local-ingress registration module plus narrow
  `ServeOptions`/listener identity exports/tests;
- this proof, the canonical plan, and the routing index at closeout.

No Netavark, netns, nftables, gvproxy, socket, router, TLS effect, protocol,
PEP/PDP, service-name, system-projection, machine-provider, cluster-transport,
or cloud-provider implementation moves into `nimbus-network`.

## Evidence Ledger

| Checkpoint | Evidence |
| --- | --- |
| Read-only source audit | Three bounded audits completed with zero edits: registry/value shape; container/krun attachment ownership; server/local ingress ownership. |
| Expected red | `timeout 600 cargo test -p nimbus-network --test capability_registry` exited 101 with E0432 naming all five absent registry contracts. `timeout 600 cargo test -p nimbus-sandbox --test capability_registration` exited 101 solely with four E0599 missing registration methods after target-specific imports were corrected. `timeout 600 cargo test -p nimbus-server --test network_capability_registration` exited 101 with two E0599 missing `ServeOptions` registration methods. No production Rust source had changed. Owner inspection then found an R3 edge before candidate review: the crossed-role/order regression exited 101 because opposite input orders named different conflicting provider IDs. Canonical prevalidation/sorting corrected it; the exact test now passes. |
| Focused behavior | Registry unit lane: 13 passed, 0 failed, 160 filtered. Registry integration: 3 passed, including forward/reverse multi-bundle order, different membership, and both exact selections in the provider-neutral digest proof. Public satisfaction integration: 3 passed. Target-independent sandbox registration matrix: 3 passed, 707 filtered. Local sandbox integration: 2 passed. Local server registration integration: 2 passed; its third Linux-only case composes both actual sandbox registrations with actual server ingress and is not counted on macOS. Happy, edge, error, stable-order, crossed-role, wire-rejection, no-fallback, PlanOnly, startup-reconciliation, machine-forwarder, target, conditional-TLS, and plan-digest cases are all asserted. |
| Affected suites | `cargo test -p nimbus-network --all-features`: 185 passed (173 unit + 3 registry + 3 satisfaction + 6 conflict), 0 failed/ignored. `cargo test -p nimbus-sandbox --all-features`: 690 passed, 0 failed, 24 declared ignored. The complete server nextest lane ran 599 tests: 594 passed, 5 failed, 26 skipped. All five failures were reproduced independently and are outside the owned behavior: the two NNC3 listener load-timeout cases pass 1/1 exactly; the deploy-admin and cloud-functions cases are recorded NNC3.8 base failures; the runtime-metrics artifact expectation also fails exactly without this diff. Excluding only those five named baseline cases yields 594 passed, 0 failed, 31 skipped, including both new registration tests. |
| Platform truth | The local host is macOS. The target-independent sandbox unit matrix proves the intended Linux capability facts and fail-closed guards without constructing a provider. The two local integration cases prove PlanOnly/unsupported-target refusal. A new upper-level `cfg(target_os = "linux")` test constructs both actual Execute backends, bundles each actual registration with the actual `ServeOptions` ingress registration, and selects both pairs against one explicit requirement set. It is not misreported as locally executed. Native cross-checking reached target C dependencies but cannot complete without the absent `aarch64-linux-gnu-gcc`; a Zig attempt also fails in existing CMake/aws-lc dependencies before Rust type-checking. Hosted Linux remains the execution evidence for that case. |
| Quality gates | Affected all-target/all-feature `cargo check` passes. Affected strict Clippy passes with `-D warnings`; only existing vendored Brotli diagnostics are emitted outside the affected crates. Warning-denied rustdoc passes for network, sandbox, and server. After review correction, full network remains 185/185 and network/server all-target/all-feature check plus strict Clippy pass. Final format and diff checks are part of correction freeze. |
| Static boundaries | Workspace metadata reports exactly `nimbus-network -> nimbus-core`. Public upper crates have zero uses of the NNC4.1 all-dimension matcher; it remains a private test oracle only. Registry modules contain no provider effect, callback, socket, async, environment-probe, transport, policy, naming, forwarding, cluster, or cloud SDK seam. New concept-owned files are 827, 585, 346, and 56 lines, all below modularity thresholds. The bind inventory was refreshed for four mechanically shifted server line numbers; the live verifier then passed 15/15 and its adversarial self-test passed 45/45. |
| Docs gates | `scripts/check-docs.sh` passes 108 link-clean pages with source map/private fence/title checks. `scripts/verify-nimbus-docs-site.sh` passes 17/17 conditions. |
| Structured review | One full GPT-5.6 Sol/xhigh/fast review covered the 145,859-byte frozen bundle in one pass and reported three findings at 0.98. All were accepted and corrected. The one allowed narrow correction review covered the 152,754-byte correction bundle in one pass, reported no accepted/actionable findings, and returned `patch is correct (0.98)`. No further review is warranted or permitted. |
| Final commit/tree | Reviewed correction candidate diff SHA-256: `cebb861c5a249fba0090277f190eb5c5caaf92ebb9e0eb61737b713f9c63d076`. The exact item commit is the first commit containing this completed proof; its SHA/tree are recorded in the next Recovery Header transition immediately after creation. |

## Acceptance Traceability

| Clause | Candidate evidence |
| --- | --- |
| R1 | The public registry is closed data plus deterministic evaluation. Metadata reports only the core workspace edge; effect/dependency scans and the 15/15 verifier pass. |
| R2 | `NetworkCapabilitySelection` names exactly one attachment and ingress provider. Only `NetworkCapabilityBundle` enters `NetworkCapabilityRegistry::new`; there is no per-role insertion API. |
| R3 | Repeated identical bundles are idempotent. Cross-role identity, divergent report, and reordered divergent-input tests return the same typed diagnostic without a partially observable registry. |
| R4 | Exact, unknown, missing-role, known-but-unregistered, and satisfying-alternative tests prove rejection without fallback or Cartesian synthesis. |
| R5 | Attachment and ingress evaluators own disjoint dimensions and independently check the three shared lifecycle/sovereignty dimensions. Cross-role masking tests pass. |
| R6 | The full mismatch vector, role order, dimension order, selection wire order, and sorted complete alternatives are pinned by tests. |
| R7 | Container and krun expose distinct Execute registrations. Unit and integration matrices reject PlanOnly, unsupported target, startup-reconciliation failure, and container machine-forwarder mode without performing an effect. |
| R8 | Exact container/krun positive and negative fact sets are asserted. Neither registration overclaims provider-managed, host, IPv6, forwarding, or external-dependency capabilities. |
| R9 | Server reuses `nimbus-server.tcp-listener`; exact address/bind/exposure/protocol/port/ingress/lifecycle/sovereignty facts and conditional TLS are asserted. Unsupported forwarding/publication/certificate/DNS/relay/cloud facts stay empty. |
| R10 | Provider-neutral digest tests, focused and full affected suites, quality gates, dependency/effect scans, verifier/self-test, modularity checks, docs 108, and site 17/17 pass. The full item review is complete; final correction docs/format/diff gates precede exactly one narrow review of its accepted findings. |

## Structured Review Disposition

| Finding | Disposition and correction |
| --- | --- |
| P2: real provider pairs were not selected together | Accepted. Separate source-fact tests did not prove the actual cross-crate compositions. A Linux-only server integration case now constructs actual container and krun Execute registrations plus actual `ServeOptions` ingress, registers both complete bundles, and selects both exact pairs against explicit independent requirements. |
| P2: digest proof did not vary order or membership | Accepted. The strengthened network integration case constructs forward and reverse two-bundle registries, selects different exact pairs, constructs a different one-bundle membership, and asserts the same provider-neutral plan digest after every operation. |
| P3: stale file counts | Accepted. The static ledger now records 827, 585, 346, and 56 lines for the four concept-owned implementation/test modules. |

## Baseline Failure Disposition

No NNC4.3 code path is implicated by the five aggregate server failures:

1. `nnc3_7a_prebound_sibling_is_adopted_without_rebind` and
   `nnc3_5_sibling_bind_is_claimed_before_guard_and_serves_identical_bytes`
   time out only under aggregate load and each passes exactly in isolation.
2. `deploy_admin_requires_local_admin_header_even_with_deploy_bearer` and
   `cloud_functions_passes_runtime_owner_lifecycle_conformance` are named
   NNC3.8 base failures.
3. `runtime_metrics_route_returns_limits_and_metrics_when_convex_support_is_enabled`
   expects `missing_artifact` but receives `not_linked` and reproduces exactly
   outside the NNC4.3 paths.

The filtered 594-test lane is therefore an honest affected-behavior gate, not
a claim that the unmodified baseline is globally green.
