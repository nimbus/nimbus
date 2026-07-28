# NNC4.2 Capability Interface Substitution Review

Date: 2026-07-28

Status: `complete`

Starting checkpoint:
`8907ccada3004238b6442ccbb3e5c9e7f79dff8d`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

NNC4.2 promotes no new public product-effect interface.

That is the evidence-based result required by the item, not an omitted
implementation. Capability vocabulary and deterministic satisfaction may
exist before an effect interface. Every newly proposed attachment, ingress,
name, certificate, forwarding, machine, lifecycle, and registry interface
either lacks two real production substitutions or would duplicate/collapse an
existing upper owner's semantics.

The already-earned `NetworkSegmentAllocator` remains the sole public
production trait in `nimbus-network`. NNC2.2 already proved its substitution
through materially different single-node and lease-gated cluster allocators,
injected container and krun consumers, and test substitutes. NNC4.2 neither
duplicates nor changes that contract.

## Written Acceptance

NNC4.2 is complete only when all of these clauses pass:

| ID | Required result | Direct proof |
| --- | --- | --- |
| S1 | The review inventories every capability-interface candidate implied by the plan and every existing network-facing trait that could be mistaken for one. | The decision table covers capability satisfaction/registry, segment allocation, attachment/lifecycle, ingress/listeners, logical naming, certificate authority, forwarding/egress, machine networking, lease/store authority, observations, and cluster transport. |
| S2 | A new public product interface is promoted only with at least two real production adapters or two materially different production consumers that share one substitutable behavioral contract. | Each of the thirteen candidates records exact production implementations and consumer groups. Test fakes, enum variants, value reports, opaque handles, ownership modes, mere shared use, and unrelated interfaces do not count. |
| S3 | Candidates without that evidence remain concrete and concept-owned. | No new trait, registry interface, compatibility shim, provider effect, or source dependency is added by this item. |
| S4 | Existing earned seams are reused without wrapping or broadening them. | `NetworkSegmentAllocator`, `SandboxBackend`, `RuntimeServiceRegistry`, and server-local adapter traits retain their current owners and purposes. |
| S5 | Dependency and authority boundaries remain mechanically true. | Metadata reports exactly `nimbus-network -> nimbus-core`; source scans find no speculative candidate trait, no capability consumer outside `nimbus-network`, and no provider registry. |
| S6 | Later implementation ownership is explicit. | NNC4.3 owns concrete capability registration/selection; NNC4.4 machine fact mapping; NNC4.5 PEP readiness composition; NNC5 attachment lifecycle; NNC7 service/system/TLS projections; NNC8 provider reconciliation; horizontal scaling alone owns future cluster transport. |

## Review Method And Fail-Before Rule

This item is a seam decision, so its fail-before proof is structural rather
than a deliberately uncompilable test. Before any source edit, a proposed
interface must name:

1. its exact portable behavioral contract;
2. two real production adapters or two materially different production
   consumers;
3. the dependency-safe contract owner; and
4. the existing concrete authority that the interface replaces without
   creating a second owner.

Every new candidate fails that admission rule. The correct fail-before outcome
is therefore non-promotion. Adding a fake or no-op implementation merely to
make a trait appear substitutable is explicitly forbidden.

This proof does not claim that Nimbus will never gain another network
interface. It records that the present source tree has not yet earned one.
Later items may promote a seam only with new production evidence and an
acceptance-bearing plan checkpoint.

## Existing Trait Census

### `nimbus-network`

Production source exposes exactly one public trait:

```text
crates/nimbus-network/src/segment.rs:
  NetworkSegmentAllocator
```

Production implementations are:

- `SingleNodeSegmentAllocator`;
- `ConfiguredSegmentAllocator`, the configuration-selecting delegating
  adapter; and
- `ClusterSegmentAllocator`, which requires a live fenced super-net lease for
  creation while retaining restricted cleanup authority.

Test-only implementations are `FixedAllocator` inside the network unit tests
and sandbox's `RecordingSegmentAllocator`.

The OCI specialization fixes associated adapter types:

```text
dyn NetworkSegmentAllocator<
  Segment = OciSegmentRealization,
  Error = SandboxError,
>
```

Both `ContainerSandboxBackend` and `KrunSandboxBackend` store an injected
`Arc` of that trait object. Placement, finality, reaping, startup
reconciliation, and cluster allocation consume the same contract. Provider
realization names and errors remain sandbox-owned, and the portable contract
uses `NetworkAttachmentId`, not `SandboxId`.

This is a genuine, already-reviewed ports-and-adapters seam. It is not evidence
for a broader `NetworkProvider`.

### Upper owners

The upper crates contain valid interfaces, but their contracts do not become
network provider interfaces:

- `nimbus-sandbox::SandboxBackend` has container, krun, and upper CLI
  forwarded-machine implementations. It owns complete sandbox lifecycle and
  provider effects, not a portable network attachment effect contract.
- `nimbus-services::RuntimeServiceRegistry` has
  `ServiceInstanceBindingRegistry` and `ServiceManager` implementations. It
  owns tenant-qualified logical service lookup, readiness, lazy activation,
  and runtime bindings.
- `nimbus-services::IngressResolver` resolves an HTTP upgrade into a logical
  service instance. It has zero production implementations and zero production
  `WsIngress` constructions; its only implementation is a test fixture. It is
  neither production substitution evidence nor a socket/provider interface.
- `nimbus-server::WireProtocolAdapter` has MongoDB, DynamoDB, and S3
  implementations; `HttpProtocolAdapter` has Convex, Firebase, Cloudflare, and
  Cloud Functions implementations. Both are earned server-local protocol
  composition seams. Their bind/guard/spawn and router-mount contracts own
  transport effects and cannot move below the server.
- `nimbus-machine` describes provider facts with concrete
  `MachineProviderCapabilities`; it has no provider trait. Krunkit and vfkit
  use host-managed gvproxy, while unavailable WSL2 is marked
  provider-managed and fails closed at its backend gates.
- CLI's crate-private `MachineVmmBackend` has real Krunkit and vfkit
  implementations and one launch composition root. It is an earned
  machine-local effect seam, not portable network capability evidence.

## Candidate Decision Table

| Candidate | Production substitution evidence | Decision | Ownership and pattern rationale |
| --- | --- | --- | --- |
| Capability matcher/provider registry interface | `NetworkProviderCapabilities::ensure_satisfied` evaluates one explicit value report; there are zero upper production consumers and no registry today. | Keep matching concrete; add no registry trait in NNC4.2. | NNC4.1 deliberately added value vocabulary only. NNC4.3 may register concrete capability reports and select exact satisfying registrations without inventing an effect interface. |
| `NetworkSegmentAllocator` | Three production implementations, two backend consumers, and two test substitutes already prove the same allocation/hold/release/reconcile contract. | Reuse unchanged. | This NNC2-earned seam separates portable allocation from OCI effects and cluster transport. Wrapping it would duplicate authority. |
| Sandbox-local `ClusterLeaseProvider` prototype | Zero production implementations and two in-module test fakes. Cluster admission is hard-blocked while its clock/fencing model is unproven. | Do not promote or generalize in NNC4.2; retain the explicitly deferred prototype under its existing owner. | NNC2.8 and `horizontal-scaling-plan.md` deliberately record this crate-private boundary as a prototype, not a sibling-crate contract. HS2/HS5 must promote the smallest real lease-source seam only when committed cluster state exists. It remains separate from `ClusterTransport`. |
| `NetworkAttachmentProvider` | Container and krun expose attachment work only as part of the broader `SandboxBackend`; no separate common production attachment adapter contract exists. | Do not promote. | NNC5 owns consolidation of the sandbox attachment lifecycle. Until that common behavior is real, effects remain concept-owned concrete modules. |
| Network lifecycle/inspection provider | Network has concrete desired/durable/observed state and lease transitions; upper adapters inspect different provider effects with different evidence. | Do not promote. | A generic lifecycle trait would erase effect-specific authentication and ambiguous-outcome semantics. NNC5/NNC8 must first establish common operations and exact evidence. |
| `IngressProvider` | Server has several protocol adapters, but they substitute only inside server's protocol composition roots. The main listener has one effect owner with Nimbus-owned and externally supplied ownership modes, not two interchangeable providers. `IngressResolver` has no production implementation and resolves logical identity only. | Do not promote. | Capability facts may identify server/local ingress in NNC4.3. Sockets, guards, TLS termination, framing, and protocol startup stay server-owned. |
| `NameProvider` | `RuntimeServiceRegistry` has two real implementations, but both implement the services-owned logical binding contract. Network has zero name-publication consumer or provider. | Do not promote in network. | Reuse the services seam. DNS/xDS/Consul may later consume an already resolved binding only after a separately approved concrete provider exists. |
| `CertificateProvider` | Server `TlsConfig` loads operator ingress identity; proxy `WorkloadPepTlsAuthority` mints a per-workload ephemeral interception CA and per-host leaves. | Reject unification. | These are intentionally non-substitutable trust authorities with different stores, handles, rotation, and threat boundaries. NNC7.6 proves separation. |
| `ForwardingProvider` | Egress has one production enforcement mode (`SupervisorProxy`) and proxy has one production PEP (`WorkloadPep`). HTTP forwarding, CONNECT splice, and HTTPS interception are branches inside that PEP, not provider substitutions. Sandbox gvproxy forwarding and service connection drain have still different policy and lifecycle contracts. | Reject unification. | Preserve PDP/PEP, provider-effect, transport, and drain owners. Network composes handles/readiness but never forwards. |
| Machine network provider trait | A concrete enum maps three machine providers to facts; only the networking-ownership boolean is currently consumed. WSL2 has no available backend. | Do not promote. | NNC4.4 maps machine-owned facts into network requirements and proves host-managed/provider-managed separation. An unavailable enum variant is not a real adapter. |
| Port lease/store provider interface | One concrete `LocalPortLeaseAuthority` and one network-owned `LocalNetworkStateStore` deliberately form the cross-process host authority. | Keep concrete. | Substitution here would risk a second lease/store authority. Testability comes from deterministic state machines, isolated roots, process harnesses, and fault injection rather than competing production authorities. |
| Observation/projection provider | `NetworkStatus` is portable observed vocabulary; `nimbus-system` stores rebuildable projections. | Do not promote. | Value/status conversion is sufficient. A provider trait would invite projections to become desired state or lease authority. |
| `ClusterTransport` | There is no crate or production implementation. The cluster allocator only consumes a fenced super-net lease. | Do not promote or stub. | Horizontal scaling exclusively owns future membership, node identity, routing, mesh, and consensus. Allocation remains routed-not-overlay and independent. |

## Production Consumer-Side Census

The interface rule has two independent admission paths, so implementation
counts alone are insufficient. This table records the materially different
production consumer groups for all thirteen candidate rows:

| Candidate | Production adapters/implementations | Production consumer groups | Why consumers do or do not demonstrate substitution |
| --- | ---: | ---: | --- |
| Capability matcher/registry | One concrete value matcher; zero registries. | Zero upper capability-report consumers. | There is no production selection or invocation contract to substitute. |
| `NetworkSegmentAllocator` | Three production implementations. | Two top-level backend consumers (container and krun), plus shared placement/reconciliation helpers. | Both backends inject the same trait object and substitute real/test implementations under one allocation contract; the seam is already earned. |
| `ClusterLeaseProvider` prototype | Zero production, two test fakes. | Zero production composition roots. | Test-only deterministic lease/clock proof does not earn promotion. |
| Attachment provider | One production effect engine (Netavark). | Two related OCI backend callers (container and krun). | The callers share one concrete realization engine; they do not substitute attachment implementations. NNC5 first consolidates their duplicated lifecycle. |
| Lifecycle/inspection provider | Zero portable effect implementations. | Zero upper consumers of `NetworkResourcePhase`, `NetworkTransitionEvidence`, `NetworkResourceVersion`, `DurableNetworkResourceState`, `NetworkStateTransition`, or `NetworkStateMutation`. | Network's concrete state machine is value/authority logic. Sandbox, server, KV, and machine inspect different effects without a shared effect contract. |
| Ingress provider | One main-listener effect owner; two ownership modes, not providers. | One server composition root; protocol adapters are additive children. | Nimbus-owned and externally supplied descriptors have intentionally different release/recovery semantics and cannot substitute as providers. |
| Name provider | Zero network name-publication adapters. | Zero network name-publication consumers; two `RuntimeServiceRegistry` implementations serve upper runtime callers. | The existing consumers substitute a services-owned logical-binding seam, so adding a network copy would duplicate ownership. |
| Certificate provider | Two mechanisms with different trust contracts. | Main server ingress and workload PEP interception. | These consumers require non-interchangeable identities, key stores, rotation, and threat boundaries. |
| Forwarding provider | One egress enforcement mode and one production PEP; one sandbox gvproxy effect. | Egress decisions/PEP, machine port realization, and service drain. | The consumers require different policy, visibility, transport, and lifecycle semantics; shared naming would erase boundaries rather than demonstrate substitution. |
| Machine network provider | One concrete provider-fact mapping; two real VMM adapters remain CLI-local. | Two production lifecycle modules (`readiness.rs` and `stop.rs`), four `uses_provider_networking()` call sites. | Both modules consume the same machine-owned mode decision; they do not swap provider implementations. NNC4.4 owns the portable fact mapping. |
| Port lease/store provider | One `LocalPortLeaseAuthority`; one `LocalNetworkStateStore`. | Four port-authority owners (sandbox, server, KV, CLI machine); raw store use outside network has one production owner (sandbox segment/IPAM). | All four must converge on the same node-global store/lock/state machine. Mere shared use is the safety invariant, not substitution. A pluggable per-consumer authority could create split-brain allocation; consumer-specific provider effects already remain in upper adapters. |
| Observation/projection provider | Zero portable effect implementations. | Zero upper consumers of `NetworkObservation`, `NetworkStatus`, `NetworkCondition`, or `NetworkStatusUpdate`; system retains one separate projection owner. | No current adapter pair shares a provider-observation contract. NNC7 must integrate portable values without turning projection into authority. |
| `ClusterTransport` | Zero production definitions. | Zero production transport consumers; two comment-only “shaped” references do not define a contract. | Horizontal scaling must first land real membership/routing consumers and its own dependency-safe seam. |

## Boundary Findings

### Capability values are not effect interfaces

`NetworkProviderCapabilities`, `NetworkCapabilityRequirements`,
`NetworkProviderId`, and `NetworkProviderHandle` are closed value/identity
types. A capability report describes one named registration; an opaque handle
lets only its owning upper adapter interpret provider state. Neither type
authorizes `nimbus-network` to call a socket, VMM, proxy, Netavark, DNS, TLS,
or cloud effect.

There are currently zero production references to
`NetworkProviderCapabilities` or `NetworkCapabilityRequirements` outside
`nimbus-network`. That is expected before NNC4.3 and prevents NNC4.2 from
mistaking planned consumers for existing substitution.

The sandbox-local `ClusterLeaseProvider` is a source-proven exception to the
current census, not an interface NNC4.2 may silently endorse or move. It is
crate-private, unreachable from production composition, has only test fakes,
and is guarded by `CLUSTER_LEASE_CLOCK_MODEL_PROVEN = false`.
`nnc2.8-horizontal-scaling-seam-truth-up.md` and the horizontal-scaling owner
already record why it exists: preserving deterministic allocator/fencing
proofs before the real raft-backed lease source is available. Removing or
promoting it here would fork that authority. Its next admissible transition is
HS2/HS5 with a real committed-state adapter and its own acceptance proof.

### Similar names do not imply substitutable semantics

- Server ingress TLS proves public server identity; proxy interception TLS
  creates a workload-local trust anchor. Sharing a certificate trait would
  weaken the type boundary.
- Sandbox gvproxy forwarding realizes machine port mappings; the proxy PEP
  enforces admitted egress; services drain logical held connections. Sharing a
  forwarding trait would collapse policy, transport, and lifecycle authority.
- Service resolution chooses a tenant-qualified logical instance; network
  status names reachable resource observations. Sharing a name interface would
  let observed addresses become identity.
- Machine provider capabilities choose VMM/provider behavior; segment
  allocation chooses portable tenant address space. Reusing one as the other
  would conflate provider ownership with allocation authority.

### Concrete does not mean untestable

The review applies “program to an interface” only where substitution is the
reason for variability. Single-authority state machines remain deterministic
through:

- pure value validation and transition tables;
- injected state roots and fault points;
- exact process-lifetime/generation/epoch fences;
- fake provider effects in the upper owning crate;
- fresh-process contention and crash-cut harnesses; and
- adapter contract suites once at least two real adapters share behavior.

This avoids a god interface while preserving robust testing.

## Structural Proof Commands

The candidate-frozen item must record real exit status for these checks:

```text
rg '^pub(?:\\([^)]*\\))? (?:unsafe )?trait ' crates/nimbus-network/src --glob '*.rs'
# exactly NetworkSegmentAllocator

rg 'impl NetworkSegmentAllocator for' crates --glob '*.rs'
# three production implementations plus two test-only implementations

rg 'NetworkProviderCapabilities|NetworkCapabilityRequirements' \
  crates --glob '*.rs' --glob '!crates/nimbus-network/**'
# no matches

rg 'NetworkAttachmentProvider|IngressProvider|NameProvider|CertificateProvider|ForwardingProvider' \
  crates --glob '*.rs'
# no matches

cargo metadata --format-version 1 --no-deps
# nimbus-network has exactly one workspace dependency: nimbus-core
```

The zero-match checks must be wrapped so “not found” is asserted deliberately,
not confused with a failed command or missing input.

## Accepted Scope

NNC4.2 owns only:

- this review proof;
- the canonical plan/recovery/checkpoint ledger; and
- the private plan routing status.

No Rust source, Cargo manifest, verifier, inventory, provider, test, or
production behavior change is admitted because no new interface passed the
seam-promotion rule.

## Verification Evidence

The candidate-frozen structural gate exited zero and reported:

```text
PASS public-network-traits=1 NetworkSegmentAllocator
PASS allocator-implementations=5 production=3 test-only=2
PASS upper-capability-references=0
PASS speculative-candidate-symbols=0
PASS capability-provider-registry-symbols=0
PASS nimbus-network-workspace-edges=1 edge=nimbus-core
PASS owned-staged-paths=2 docs-only
```

The upper-owner census also exited zero:

```text
PASS runtime-service-registry-production-impls=2
PASS ingress-resolver-production-impls=0 test-impls=1 external-constructions=0
PASS wire-protocol-adapter-production-impls=3 owner=nimbus-server
PASS http-protocol-adapter-production-impls=4 owner=nimbus-server
PASS machine-vmm-backend-production-impls=2 owner=nimbus-cli
PASS cluster-lease-provider-production-impls=0 test-impls=2 admission-clock-gate=false
```

The post-review consumer-side correction gate exited zero:

```text
PASS upper-generic-lifecycle-consumer-sites=0
PASS upper-network-observation-consumer-sites=0
PASS port-lease-production-consumer-owners=4 sandbox,server,kv,cli-machine
PASS raw-state-store-upper-production-consumer-owners=1 sandbox
PASS machine-networking-production-consumer-modules=2 call-sites=4
PASS cluster-transport-production-definitions=0
```

Three bounded read-only audit lanes independently inspected the sandbox/machine,
services/server/KV, and egress/proxy/system candidate groups. All returned the
same non-promotion result and changed zero paths. Their exact evidence informed
the tables above; they were source audits, not structured autoreviews.

Repository gates:

```text
timeout 900 bash scripts/verify-nimbus-network-control-plane.sh
# 15 passed, 0 failed

timeout 900 bash scripts/verify-nimbus-network-control-plane.sh --self-test
# 45 passed, 0 failed

timeout 900 bash scripts/check-docs.sh
# 108 pages link-clean; source map resolves; private fence intact; titles unique

timeout 900 bash scripts/verify-nimbus-docs-site.sh
# 17/17 conditions green

timeout 900 cargo fmt --all --check
git diff --check
git diff --cached --check
# exit 0
```

No Rust behavior test was rerun for NNC4.2 because its admitted diff contains
no Rust, manifest, verifier, or executable change. The exact NNC4.1 source
checkpoint remains the last behavior-tested code, and the live verifier
re-proves the dependency/effect/authority boundaries against the current
working tree.

## Acceptance Disposition

| Clause | Result | Evidence |
| --- | --- | --- |
| S1 | pass | The existing-trait census and thirteen-row candidate table cover all plan-implied capability families plus the deferred cluster lease/transport boundary. |
| S2 | pass | The thirteen-row consumer-side census records both independent admission paths and distinguishes real substitution from shared use, test fakes, ownership modes, and unrelated semantics. |
| S3 | pass | The staged diff is exactly this proof plus the canonical plan; no executable path or speculative trait is admitted. |
| S4 | pass | Existing network, services, server, sandbox, and machine seams remain unchanged and in their current dependency-safe owners. |
| S5 | pass | Sole public network trait, zero candidate symbols, zero upper capability consumers, zero registry symbols, exact core-only workspace edge, and verifier 15/15 all pass. |
| S6 | pass | The decision table routes every later concern to NNC4.3-NNC8 or HS2/HS5 without forking authority. |

## Structured Review

The one full structured item review ran against the acceptance-green frozen
bundle:

```text
AUTOREVIEW_ALLOW_NESTED_CODEX=1 \
  /Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local \
  --engine codex \
  --model gpt-5.6-sol \
  --thinking xhigh \
  --codex-speed fast \
  --prompt '<NNC4.2 S1-S6 and boundary-focused prompt>'
```

The helper confirmed the actual reviewer configuration:

```text
engine: codex
model: gpt-5.6-sol
thinking: xhigh
codex_speed: service_tier="fast"
bundle: 36874 bytes
review passes: 1
```

It reported two valid proof findings:

1. P2: S2 had exact implementation counts but omitted the independently
   sufficient production-consumer path for the port authority, lifecycle, and
   observation candidates. The thirteen-row consumer-side census and its
   exact zero/nonzero gate above correct the proof.
2. P3: S1 called the decision table eleven rows although it contains thirteen.
   S1 now names the exact thirteen rows.

Both corrections change documentation/proof only. They do not alter executable
code, interface admission, ownership, dependency direction, or the
non-promotion decision. Per the item-level review cadence, no second review is
warranted for documentation/ledger corrections. The two findings are accepted
and fully dispositioned; there are no rejected findings.
