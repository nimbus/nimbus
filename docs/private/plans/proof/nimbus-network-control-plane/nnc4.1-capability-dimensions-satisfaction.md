# NNC4.1 Capability Dimensions And Satisfaction Proof

Date: 2026-07-28

Status: `complete`

Starting checkpoint:
`c642433b230bd175b4a4a5fe968a3996d611664c`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Written Acceptance

NNC4.1 is complete only when all of these clauses pass:

| ID | Required result | Direct proof |
| --- | --- | --- |
| C1 | Every binding capability dimension has explicit provider facts and plan requirements. | The 14-dimension matrix below is represented by closed enums/sets with no implicit provider default. |
| C2 | Satisfaction is deterministic and fail closed. | A fixed-order mismatch vector is byte-stable for the same inputs; missing support returns an error and never changes the requested plan or selects another provider. |
| C3 | Diagnostics name the rejected provider, exact requirement, dimension, and safe alternatives. | Typed error assertions and pinned display text cover each dimension; alternatives are sorted and deduplicated. |
| C4 | Every dimension has positive and named-negative behavior tests. | At least one isolated positive and one exact negative assertion per row below, plus a complete multi-mismatch ordering test. |
| C5 | `NetworkPlan` carries requirements as desired state. | Requirements round-trip in the plan wire, participate in equal-generation divergence checks, and remain covered by the caller-supplied canonical plan digest. |
| C6 | NNC4.1 creates no speculative provider seam or effect owner. | Only transport-free value types, validation, matching, errors, and tests land in `nimbus-network`; registry, machine mapping, provider traits, sockets, and effects remain later-owned. |

## Current-State Audit

`nimbus-network` currently has no capability or requirement type. Its
`NetworkPlan` contains only:

```text
{ plan_id, generation, digest }
```

All five `NetworkPlan::new` call sites are inside `nimbus-network` source or
tests (`plan.rs`, `state.rs`, and `status.rs`); no upper crate currently
constructs a plan. This is a clean pre-launch point to add requirements
directly, without a compatibility constructor or defaulted field.

`nimbus-machine::MachineProviderCapabilities` contains five facts, but the
networking axis is one boolean:

```text
uses_provider_networking: bool
```

The current production consumers are exact:

- machine post-start skips host API forwarding when the provider owns
  networking;
- machine stop skips host listener withdrawal, API-forwarder stop, and
  gvproxy stop when the provider owns networking;
- krunkit and vfkit report host-managed networking; and
- WSL2 reports provider-managed networking but both start and stop reject it
  with the same named `InvalidInput` error before provider or port-authority
  effects.

CLI provider precedence explicitly chooses command input, then a non-empty
environment value, then Krunkit. That is configuration defaulting, not
capability satisfaction. NNC4.1 does not change it. NNC4.4 later maps
machine-owned facts into the portable requirement vocabulary and proves the
two networking modes cannot masquerade as one another.

There is no current network provider registry, automatic safe alternative, or
fallback path. NNC4.1 must preserve that absence: matching one explicitly
named capability report either succeeds or returns complete typed mismatch
evidence.

## Target Value Objects

NNC4.1 adds one concept-owned `nimbus-network::capability` module and exports
only value types:

- `NetworkCapabilityRequirements`;
- `NetworkProviderCapabilities`;
- `NetworkCapabilityDimension`;
- `NetworkCapabilityMismatch`; and
- `NetworkCapabilitySatisfactionError`.

The module does not add `NetworkProvider`, a registry, a trait, an async
operation, an effect callback, or a provider-specific enum. NNC4.2 still owns
the substitution test for interfaces; NNC4.3 owns provider registration and
selection; NNC4.4 owns machine fact mapping.

Requirements and provider facts are composed from six concept groups so the
top-level values do not become boolean bags:

```text
attachment  = management + attachment modes + isolation modes
endpoint    = address families + bind realms + exposure + protocol + port modes
ingress     = ingress features
forwarding  = forwarding/drain features
lifecycle   = durable inspect/reconcile/delete features
sovereignty = control-plane locality + required external dependencies
              + offline-restart support
```

All collections use ordered sets. Constructors require every group
explicitly; there is no `Default` implementation. Empty feature sets are an
explicit “no requirement” or “unsupported” statement, not an omitted value.
Unknown port exposure and unknown bind-realm evidence are not capability
facts and cannot satisfy a requirement.

## Capability Matrix

| Order | Dimension | Portable facts / requirements | Satisfaction rule | Named negative |
| --- | --- | --- | --- | --- |
| 1 | management | `NimbusHostManaged`, `ProviderManaged` | Exact equality. | `ManagementMode` names required and offered mode. |
| 2 | attachment mode | `HostNetwork`, `IsolatedNamespace`, `VirtualMachineGuest`, `ProviderVirtualNetwork` | Provider set is a superset of required modes. | `AttachmentMode` names the missing mode. |
| 3 | isolation mode | `WorkloadNamespace`, `TenantSegment`, `ProviderBoundary` | Provider set is a superset; no ordering or approximation is inferred. | `IsolationMode` names the missing proof. |
| 4 | address family | IPv4, IPv6 | Provider set is a superset. | `AddressFamily` names the missing family. |
| 5 | bind realm | host, proven-isolated | Provider set is a superset; unknown evidence never satisfies. | `BindRealm` names the missing realm kind. |
| 6 | exposure | loopback, private, public | Provider set is a superset; `PortExposure::Unknown` is rejected from both inputs. | `Exposure` names the missing reachability. |
| 7 | transport protocol | `PortProtocol::{Tcp,Udp}` | Provider set is a superset. | `Protocol` names TCP or UDP. |
| 8 | port assignment | exact, Nimbus-allocated range, provider-assigned | Provider set is a superset; one mode never substitutes for another. | `PortAssignment` names the missing mode. |
| 9 | ingress feature | host routing, path routing, TLS termination, WebSocket, streaming | Provider set is a superset; TLS support does not imply a hosted-certificate dependency. | `IngressFeature` names the missing feature. |
| 10 | forwarding feature | port forwarding, connection drain | Provider set is a superset. | `ForwardingFeature` names the missing feature. |
| 11 | lifecycle feature | durable inspect, reconcile, delete | Provider set is a superset. | `LifecycleFeature` names the missing durable operation. |
| 12 | control-plane locality | local-only, operator-local, third-party | Provider dependency scope must be no broader than the plan's maximum allowed scope. | `ControlPlaneLocality` names required maximum and offered scope. |
| 13 | external dependency | public network, DNS, hosted certificate, relay, external control plane | Every provider-required dependency must be explicitly allowed by the plan. | `ExternalDependency` names the disallowed dependency. |
| 14 | offline restart | required / not required versus supported / unsupported | Required implies provider support; unsupported never downgrades the plan. | `OfflineRestart` names the missing support. |

The fixed mismatch order is the table order. Within a set-valued dimension,
enum declaration order is the diagnostic order. This makes both the typed
vector and display string deterministic.

## Satisfaction Contract

The matching API evaluates one explicitly named `NetworkProviderId`:

```text
capabilities.ensure_satisfied(requirements, safe_alternatives)
  -> Ok(())
  -> Err(NetworkCapabilitySatisfactionError {
       provider_id,
       mismatches_in_fixed_order,
       sorted_deduplicated_safe_alternatives,
     })
```

The method does not consult environment variables, global registries, cloud
APIs, machine configuration, or projections. It never mutates either input
and never chooses an alternative. “Safe alternatives” are diagnostic facts
provided by the future registry/caller; when none are proven, display text
must say `safe alternatives: none`.

`NetworkPlan` gains a required `NetworkCapabilityRequirements` field and
accessor. Its constructor changes directly. Equal plan ID/generation accepts
only equal digest and equal requirements; either difference is conflicting
desired content. Serialization denies unknown fields and does not default
missing requirements.

## Fail-Before And Test Matrix

The fail-before change will add tests that reference the missing capability
types and four-argument `NetworkPlan::new` before the implementation exists.
The expected compile failure must name those missing types/constructor shape;
an unrelated failure is not evidence.

The fail-before gate ran before any production source changed:

```text
timeout 600 cargo test -p nimbus-network --test capability_satisfaction
exit: 101
E0432: unresolved capability value-object imports
E0061: NetworkPlan::new takes 3 arguments but 4 were supplied
E0599: NetworkPlan has no requirements accessor
```

Those are exactly the three missing contract surfaces exercised by the test.
The compiler reached the intended crate and integration-test target; no
dependency, fixture, or unrelated source failure obscured the result.

After implementation:

1. Each of the 14 rows gets an isolated positive test.
2. Each row gets an isolated named-negative test that asserts:
   provider ID, mismatch variant, dimension, required/offered fact, safe
   alternatives, and display text.
3. One full-capability provider satisfies a full-requirement plan.
4. One zero-support provider produces the complete 14-dimension mismatch
   vector in fixed order without mutating requirements.
5. Reordered/duplicated input facts and safe alternatives produce identical
   typed errors and text.
6. Unknown exposure/bind evidence and unknown wire fields fail at
   construction/deserialization.
7. Local-only plus an empty external-dependency allowance rejects each of the
   five external dependencies independently.
8. TLS support without a hosted-certificate dependency satisfies local TLS;
   the dependency remains a distinct dimension.
9. Exact/Nimbus-allocated/provider-assigned port modes cannot substitute for
   one another.
10. `NetworkPlan` wire round-trips requirements, rejects a missing field, and
    rejects equal-generation requirement divergence even when a caller
    incorrectly reuses the digest.

## Owned Scope

Admitted implementation paths are:

- `crates/nimbus-network/src/capability.rs`;
- `crates/nimbus-network/src/lib.rs`;
- `crates/nimbus-network/src/plan.rs`;
- current `NetworkPlan` fixture callers in `state.rs` and `status.rs`;
- this proof; and
- the canonical plan/recovery ledger.

No machine, sandbox, server, KV, CLI, compute, tenant, service, proxy, system,
cluster, dependency-baseline, bind-inventory, or verifier path is admitted
unless the fail-before audit proves a direct NNC4.1 acceptance need.

## Implementation And Acceptance Evidence

The implementation remains split by concept ownership:

- `capability.rs` contains 1,055 lines of production value objects,
  canonicalization, matching, and typed diagnostics;
- `capability/tests.rs` contains 724 lines of private behavioral proofs, so
  the production module remains below the repository's 1,500-line
  justification threshold;
- the public integration test pins construction, named-provider matching,
  fail-closed mismatch behavior, sorted/deduplicated alternatives, and the
  required plan field; and
- existing `plan.rs`, `state.rs`, and `status.rs` fixtures were changed
  directly, without a compatibility constructor or a default.

No error type accepts a deserialized authority-shaped wire. Requirements and
provider facts deserialize through closed enums and ordered sets; satisfaction
errors are deterministic outputs only.

| Clause | Result | Evidence |
| --- | --- | --- |
| C1 | pass | Six concept groups expose all 14 closed dimensions; 14 named tests exercise every row and no capability type implements `Default`. |
| C2 | pass | One provider is evaluated explicitly; missing support returns one fixed-order mismatch vector. Reordered/duplicated requirement facts, provider facts, and alternatives produce equal typed errors, display text, and serialized bytes. |
| C3 | pass | Every negative asserts provider identity, exact mismatch variant/dimension, safe alternatives, and requirement-bearing text. Empty alternatives render as `none`; non-empty alternatives sort and deduplicate by stable provider ID. |
| C4 | pass | Fourteen `*_has_positive_and_named_negative_proof` tests isolate every dimension. The complete case asserts the exact 14-dimension order. |
| C5 | pass | `NetworkPlan::new` requires a distinct `NetworkPlanContentDigest` plus requirements and derives the final domain-separated `NetworkPlanDigest` from both. The plan wire stores only the content digest and requirements, so an inconsistent final digest cannot be supplied or deserialized. Requirement-only changes alter the pinned final digest, and equal-generation content divergence returns `EqualGenerationContentConflict`. |
| C6 | pass | Source scans find no provider trait/registry, `Default`, socket, transport, cloud, machine, cluster, async, or provider-effect symbol in the admitted capability paths. |

Additional behavioral obligations pass:

- a full-capability provider satisfies a full requirements set;
- every external dependency fails independently unless explicitly admitted;
- local TLS does not imply a hosted-certificate dependency;
- exact, Nimbus-range, and provider-assigned port modes never substitute;
- unknown bind-realm/exposure evidence fails conversion and has no accepted
  capability wire value; and
- unknown fields fail deserialization.

## Verification

| Gate | Result |
| --- | --- |
| Fail-before integration test | intended exit 101: `E0432`, `E0061`, `E0599` only |
| Capability matrix | 21 passed, 0 failed, 0 ignored |
| Plan behavior/wire | 7 passed, 0 failed, 0 ignored |
| Public capability integration | 3 passed, 0 failed, 0 ignored |
| Full `nimbus-network --all-features` | 169 passed across 160 unit + 3 capability integration + 6 conflict integration; 0 failed, 0 ignored |
| All-target/all-feature check | pass |
| Strict affected Clippy | pass with `-D warnings` |
| Workspace `make clippy` | pass |
| Warning-denied `nimbus-network` rustdoc | pass |
| Workspace dependency metadata | exactly one workspace edge: normal, non-optional `nimbus-network -> nimbus-core` |
| Live network-control-plane verifier | 15 passed, 0 failed |
| Verifier fail-closed self-test | 45 passed, 0 failed |
| Format and diff checks | pass |
| Private/public docs gates | 108 pages link-clean; site 17/17 |

## Structured Review And Finding Disposition

The one candidate-complete review ran with:

```text
engine: codex
model: gpt-5.6-sol
thinking: xhigh
codex_speed: service_tier="fast"
bundle: 110119 bytes; review passes: 1
```

It reported one P2 at `plan.rs:144`: the original constructor accepted the
final digest and requirements independently, so C5's complete-content digest
claim was not enforced by the type. The finding is accepted as an in-scope
correctness defect.

Before correction, the focused regression failed exactly because a
requirement-only change retained the same digest:

```text
timeout 600 cargo test -p nimbus-network \
  plan::tests::capability_requirements_are_bound_into_the_plan_digest -- --exact
exit: 101
left:  NetworkPlanDigest("b60b935389f7cf68e7877a80a4ded0dfc93e248b8807932536e1de0f771d259b")
right: NetworkPlanDigest("b60b935389f7cf68e7877a80a4ded0dfc93e248b8807932536e1de0f771d259b")
```

The correction introduces a distinct `NetworkPlanContentDigest`. `NetworkPlan`
stores that value and the canonical requirements, while `digest()` derives a
domain-separated SHA-256 over:

```text
"nimbus.network.plan.digest.v1\0"
|| 32-byte content digest
|| big-endian u64 requirements length
|| canonical requirements JSON
```

Requirements contain only fixed-order structs, closed enums, booleans, and
ordered sets. The complete digest is pinned to
`dd1314e61f5027ead64e890f99fd4c421defb60ae4edda124924e0b522f4fe60`
for the named fixture. The plan wire stores `content_digest` plus
`requirements` and rejects a supplied `digest` field. The focused regression
now passes 1/1; plan tests pass 7/7; full network tests pass 169/169; strict
affected Clippy, warning-denied rustdoc, format, and diff checks pass.

Because this accepted finding materially changed executable plan identity
semantics, one narrow correction review was required by the review contract.
It ran as one 125,242-byte Sol/xhigh/fast pass and returned:

```text
autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.98)
```

The reviewer confirmed that the original P2 is resolved across constructors,
serde wire, requirement-only changes, canonicalization, equal-generation
fencing, state/status consumers, and the no-effect ownership boundary. No
additional broad or second-opinion review is warranted.

## Next Action

Commit this completed item with its plan/proof checkpoint, then begin NNC4.2
with a read-only substitution inventory. Do not promote a provider interface
without two real adapters or consumers and a behavioral substitution proof.
