# NNC1.2 Stable Network Identities Proof

Date: 2026-07-23

Status: `passed`

Source commit before the item: `6f81754cbf28bb842dbc16f9fe380244b5bc7ac7`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

`nimbus-network` now owns stable, domain-separated identities for every
resource named by the target model:

| Type | Canonical prefix | Identity is independent of |
| --- | --- | --- |
| `NetworkPlanId` | `netplan_` | plan generation and provider |
| `NetworkAttachmentId` | `netattachment_` | workload ID, address, and netns |
| `NetworkSegmentId` | `netsegment_` | CIDR, local index, and bridge |
| `PublishedEndpointId` | `netendpoint_` | IP address and port |
| `ListenerId` | `netlistener_` | bind address |
| `IngressRouteId` | `netroute_` | observed route target |
| `PortLeaseId` | `netportlease_` | leased port number |
| `NetworkProviderId` | `netprovider_` | copied provider enum or handle |

Each ID is an opaque newtype around one canonical
`<domain>_<26-character uppercase ULID>` representation. Generation is
explicit through `generate()`; parsing, `Display`, string conversion, ordering,
hashing, and JSON serialization preserve that representation. Strict parsing
rejects another domain, malformed ULIDs, lowercase/noncanonical encodings, and
does not copy the rejected value into its error.

The new numeric fencing types are:

- `NetworkResourceGeneration`, a monotonic desired generation within a plan;
  and
- `NetworkLeaseEpoch`, a monotonic authority epoch that fences stale create
  and publish actors without erasing the later cleanup right.

Both are ordered `u64` newtypes with pinned numeric JSON representation and
`checked_next()`. Overflow returns `None`; it can never wrap to apparently
fresh authority.

## Why This Shape

The implementation follows the strongest useful pieces of existing Nimbus
conventions while closing their known gaps:

- newtypes preserve compile-time domain separation;
- borrowed `as_str()`, `Display`, `FromStr`, and strict serde match the
  established core-ID ergonomics;
- ULIDs provide portable, address-independent identity and sortable canonical
  text without adding a workspace dependency;
- full resource prefixes make logs and durable records self-identifying and
  make cross-domain parsing fail closed; and
- no IP, port, local allocation index, provider enum, bridge name, or sandbox
  ID participates in identity.

ULID ordering is a deterministic representation property, not a claim that
wall-clock generation order is a fencing token. Only
`NetworkResourceGeneration` and `NetworkLeaseEpoch` carry monotonic authority.

## Property and Wire Proof

The property suite fixes each invariant at 512 generated cases:

| Property | Work per generated case | Result |
| --- | ---: | --- |
| text + JSON round trip | all 8 ID types | pass |
| domain-separation matrix | 8 accepted + 56 rejected parses | pass |
| numeric/lexical ordering | all 8 ID types | pass |
| generation/epoch serde + ordering | both fencing types | pass |

A pinned golden vector fixes all eight textual prefixes, exact uppercase ULID
encoding, string-valued ID JSON, and numeric generation/epoch JSON. Focused
unit tests additionally prove:

- generated attachment IDs use the canonical prefix and parse back exactly;
- another resource domain fails both direct parsing and serde;
- malformed and noncanonical inputs report stable reason codes;
- generation and epoch remain distinct Rust types; and
- both counters return `None` at `u64::MAX`.

Command and result:

```text
timeout 240 cargo test -p nimbus-network -- --test-threads=1
# 11 passed, 0 failed, 0 ignored; 0 doctests
```

Proptest prints a reproducible seed and persists a regression case if a
generated invariant fails.

## Dependency and Static Boundary

Production adds only external `serde` and `ulid`; test code adds external
`proptest` and `serde_json`. A freshly generated normal/dev/all-feature graph
for the host and four target profiles proves that every profile remains
acyclic and contains exactly one workspace edge:

```text
nimbus-network -> nimbus-core
```

The expected-red verifier remains honestly red with ten passes and two
deliberately later failures. `NNCV011` no longer reports a missing
`NetworkAttachmentId`; it continues to report the endpoint, segment, and
allocator owners that NNC1.3 and NNC1.4 have not migrated.

## Quality Gates

```text
timeout 240 cargo clippy -p nimbus-network --all-targets -- -D warnings
# exit 0

timeout 180 cargo check -p nimbus-network --all-targets
# exit 0

timeout 180 cargo doc -p nimbus-network --no-deps
# exit 0

cargo fmt --all --check
# exit 0

git diff --check
# exit 0

bash scripts/check-docs.sh
# 108 pages link-clean, source map resolves, private fence intact, titles unique

bash scripts/verify-nimbus-docs-site.sh
# 17/17 conditions green
```

## Scope Guard

NNC1.2 adds identity and fencing vocabulary only. It does not:

- define `NetworkPlan`, endpoint, segment-allocation, or state-machine types;
- move or re-export sandbox endpoint vocabulary;
- create a provider interface, registry, durable store, or effect;
- assign a segment from a node super-net; or
- change any current consumer.

Those responsibilities remain sequenced under NNC1.3 and later items.

## Independent Review

Claude Opus 4.8 at maximum reasoning reported no accepted or actionable
findings and rated the patch correct (`0.85`). The review independently
verified all eight required ID domains, the 8-by-8 cross-parse matrix, strict
canonical/confusable ULID handling, fixed-length lexical ordering, pinned
serde forms, generation/epoch type separation and no-wrap behavior, parse-error
redaction, reproducible property failures, the exact eleven-test accounting,
the single workspace edge, and the plan/index/recovery-ledger transition.
