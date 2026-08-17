# NNC1.5 Network State Model Proof

Date: 2026-07-23

Status: `passed`

Source commit before the item:
`032068ecf724d2acac742201a171fc0d548518ed`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

`nimbus-network` now has three structurally distinct state domains:

```text
NetworkPlan
  { stable plan ID, desired generation, canonical content digest }
        |
        v
DurableNetworkResourceState
  { typed resource ID, exact generation/digest/lease epoch,
    authoritative phase, optional redacted provider handle }
        |
        v
NetworkStatus / NetworkObservation
  { generation-scoped provider evidence and bounded conditions }
```

None of these types performs I/O or a provider effect. Desired intent cannot
write durable phase, durable state cannot claim observed readiness, and
observed evidence cannot allocate, publish, release, or mutate authority.

This item establishes the provider-neutral desired-generation envelope and
fencing contract. NNC4 remains the owner of capability requirement vocabulary,
and NNC6.2 remains the owner of compiling admitted tenant/workload intent into
the complete plan content above this low-dependency crate. The canonical
content digest is stored now so equal-generation divergence fails closed while
those later resource shapes are added; NNC1.5 does not claim their
implementation.

## Desired Plan Contract

`NetworkPlan` contains:

- a stable `NetworkPlanId`;
- a monotonic `NetworkResourceGeneration`; and
- a `NetworkPlanDigest`, pinned to canonical lowercase SHA-256.

The canonical encoding is deliberately compiled above `nimbus-network`.
`classify_update` accepts an exact identity/generation/digest replay as
idempotent, accepts only a strictly newer generation as an advance, and
rejects:

- another stable plan identity;
- an older generation; and
- the same generation with different desired content.

Digest parsing requires exactly 64 lowercase hexadecimal characters. The
SHA-256 algorithm, wire encoding, and one known vector are pinned by tests.

## Durable Authority Contract

`NetworkResourceId` is a tagged union over attachment, segment, published
endpoint, listener, ingress-route, and port-lease identities. It does not
flatten those domains into strings and never uses an address as identity.

`NetworkResourceVersion` binds each durable resource to the exact:

- plan identity;
- typed resource identity;
- desired generation;
- desired plan digest; and
- issuing lease epoch.

Every mutation validates all five fields before changing authority. Older and
newer generations, older and newer epochs, another identity, and a conflicting
digest return named errors without mutation.

The authoritative phase model is:

```text
Reserved -> Provisioning -> Ready -> Publishing -> Active
                  |           |          |           |
                  +-----------+----------+-----------+
                                      |
                                      v
                                Withdrawing
                                  /      \
                            Draining    Deleting
                                \        /
                                 v      v
                             CleanupPending
                                  |    |
                                  +----+
                                    |
                           DeletionConfirmed
                                    |
                                 Released
```

Additional explicitly guarded paths are:

- `Reserved -> Released|Failed` only with `ConfirmedNoEffect`;
- `Provisioning -> Failed` only with `ConfirmedNoEffect` and no durable
  provider handle;
- any effect-bearing nonterminal phase may enter `CleanupPending` on
  `AmbiguousEffect`;
- `Deleting|CleanupPending -> Released` only with `DeletionConfirmed`; and
- replaying the same phase is idempotent after the full version fence passes.

There is no ordinal shortcut. The implementation enumerates each allowed edge.
`Released` and `Failed` are terminal. A known provider handle prevents a
no-effect terminal transition; ambiguity quarantines it for cleanup. A
historical opaque handle may remain in a released record only after deletion
proof, preserving audit/reconciliation history without implying a live effect.

Provider handles are bounded, nonempty, control-character-free, scoped to a
stable provider ID, and redacted from `Debug` and `Display`. Durable
serialization necessarily retains the opaque value for reconciliation; only
the explicit provider accessor reveals it.

## Observed Status Contract

`NetworkObservation` contains only:

- the exact resource version fence;
- a provider-reported phase;
- an optional stable provider registration ID; and
- at most one value for each bounded condition kind (`Ready`, `Published`,
  `Degraded`, `CleanupPending`).

Conditions are canonically sorted and tri-state. Exact replay is idempotent;
same-generation evidence may refresh. Wrong-plan, wrong-resource, stale,
future, conflicting-digest, and wrong-epoch observations are rejected without
changing the latest projection.

When desired state advances, older evidence may remain available through
`latest_evidence` for reconciliation diagnostics, but `current` mechanically
hides it. A newer desired generation cannot regress the lease epoch.
Deserialized retained evidence must share the plan/resource identity, cannot
be from the future, and cannot carry an epoch newer than desired authority.

`NetworkStatus` remains a rebuildable evidence record. It has no provider
handle, allocation mutation, phase-transition, publish, or release method.

## Exhaustive State-Machine Proof

The phase/evidence test independently enumerates:

```text
11 source phases × 11 target phases × 4 evidence classes = 484 cases
```

For every case it:

1. compares `allows_transition` against an independent 24-edge truth table;
2. invokes `DurableNetworkResourceState::apply_transition`;
3. requires exact replay to be idempotent;
4. requires every named edge to reach only its target; and
5. requires every illegal edge to return its named error with byte-equivalent
   authority state.

Separate tests reject every mismatched identity/generation/digest/epoch token,
prove terminal phases cannot reactivate, prove provider-handle adoption is
idempotent and conflict-safe, and pin the tagged resource-ID wire.

## Validated Persistence Boundary

The initial independent review found two accepted wire-boundary defects:

1. directly derived `Deserialize` could reconstruct
   `Reserved|Failed + provider_handle`, including an unsafe
   `Reserved -> Released` no-effect claim; and
2. directly derived observation/status deserialization could bypass condition
   uniqueness/canonicalization.

Both were fixed before completion:

- `DurableNetworkResourceState` now deserializes through a private validating
  wire type and rejects every API-unreachable phase/handle pair;
- `apply_transition` also defensively refuses a handle-bearing release unless
  evidence is `DeletionConfirmed`;
- `NetworkObservation` deserializes through its validating constructor;
- `NetworkStatus` deserializes through an identity, generation, digest, and
  epoch validator; and
- malformed-wire tests cover unreachable durable states, duplicate
  conditions, canonical ordering, cross-resource smuggling, future evidence,
  and inconsistent retained epochs.

Valid provisioning and released-with-deletion-proof states round-trip exactly.
Aggregate debug output remains redacted. A second Opus review confirmed the two
findings were fully closed and found no new accepted or actionable issue.

## Dependency And Effect Boundary

A fresh six-profile metadata capture at source HEAD `032068ecf` reports:

| Profile | Workspace edges | Cycles |
| --- | ---: | ---: |
| normal default host | 222 | 0 |
| dev/test/build default host | 247 | 0 |
| all-feature macOS arm64 | 253 | 0 |
| all-feature Linux x86_64 | 253 | 0 |
| all-feature Linux arm64 | 253 | 0 |
| all-feature Windows x86_64 | 253 | 0 |

The only outgoing workspace edge remains:

```text
nimbus-network -> nimbus-core
```

`sha2` is a third-party pure-computation dependency; it adds no workspace edge
and no I/O. Source scans found no tenant, sandbox, server, system, service,
compute, cluster, Iroh, Axum, Pingora, Netavark, nftables, gvproxy, filesystem,
process, socket-binding, DNS, or provider-effect implementation in the new
modules. The only `SocketAddr` in the crate remains the previously migrated
published endpoint value.

Each new public state/plan/provider type has exactly one definition under
`nimbus-network`.

## Behavioral Verification

```text
timeout 600 cargo test -p nimbus-network --all-targets \
  -- --test-threads=1
# 40 passed; 0 failed; 0 ignored

timeout 600 cargo clippy -p nimbus-network --all-targets -- -D warnings
# exit 0

timeout 600 cargo doc -p nimbus-network --no-deps
# exit 0

cargo fmt --all --check
git diff --check
# exit 0
```

The 40 tests include the 484-case exhaustive transition matrix, desired
generation/digest rules, exact version fencing, provider-handle lifecycle and
redaction, validated durable/status wire reconstruction, stale/future
observations, projection fencing, and all previously landed network identity,
endpoint, and segment behavior.

## Static Verifier State

The pre-NNC1.6 verifier remains honestly expected-red:

```text
Summary: 10 passed, 2 failed
```

The two failures are the already sequenced later authorities:

- `NNCV005` names pre-NNC3 port scanners/allocators; and
- `NNCV011` names the private sandbox allocator trait that NNC2.2 moves and
  generalizes.

No condition was weakened or suppressed to complete NNC1.5. NNC1.6 now owns
deepening the verifier with named forbidden dependency/effect, duplicate
definition, and address-as-identity conditions.

## Independent Review

The repository `autoreview` workflow ran Claude Opus 4.8 at maximum reasoning
twice over the staged NNC1.5 diff.

The first pass reported the two accepted deserialization findings recorded
above and otherwise confirmed:

- explicit rather than ordinal phase transitions;
- exact plan/resource/generation/digest/epoch fencing;
- non-vacuous exhaustive mutation tests;
- distinct desired, durable, and observed types;
- redacted provider handles;
- the documented desired digest-envelope seam; and
- preserved dependency and provider-effect locality.

After the fixes and four additional tests, the second pass reported no
accepted or actionable findings and assessed the patch as correct with `0.77`
confidence. It independently verified the complete runtime-unreachable
phase/handle set, deletion-confirmed historical-handle semantics, validated
condition/status reconstruction, cross-resource projection fencing, all 24
legal edges, non-mutating error paths, and the absence of another serde bypass.

## Scope Guard

NNC1.5 does not:

- implement the NNC2 crash-safe store or claim these serde records are an
  atomic persistence mechanism;
- define speculative capability-provider interfaces ahead of NNC4;
- compile tenant/workload policy into complete plan content ahead of NNC6.2;
- execute sockets, forwarding, namespaces, bridges, firewalls, DNS, TLS,
  proxy, or cluster transport;
- treat observations as desired state or lease authority;
- permit an ambiguous provider effect to become reusable; or
- suppress the known expected-red port and allocator obligations.
