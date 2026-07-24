# NNC1.3 Endpoint Vocabulary Migration Proof

Date: 2026-07-23

Status: `passed`

Source commit before the item: `61f8c59a36dd2f71a8fdb4bae780b55a063ce14d`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

`nimbus-network` is now the only definition owner for portable endpoint
vocabulary:

- `EndpointProtocol` generalizes the former sandbox-named protocol enum;
- `PublishedEndpoint` remains the actual reachable location reported by an
  effect provider; and
- the `nimbus` facade re-exports both directly from `nimbus-network`.

The old `nimbus-sandbox::endpoint` module, its public re-export, and the
`PublishedEndpointProtocol` name are deleted. This is a direct pre-launch
replacement: there is no alias, deprecated name, compatibility module, or
transitive sandbox re-export.

The endpoint type remains deliberately transport-free. `EndpointProtocol`
describes application semantics but parses no bytes, owns no listener, performs
no bind, terminates no TLS, and forwards no traffic.

## Wire and API Parity

Four network-owned tests pin the existing behavior exactly:

1. protocol values remain `"tcp"`, `"http"`, and `"https"`, while unknown
   values fail;
2. an IPv4 endpoint without `guest_port` retains the exact field order and
   omission behavior;
3. an IPv6 HTTPS endpoint with `guest_port` retains the exact address and
   numeric wire forms; and
4. omitted and explicit-null guest ports both deserialize as `None`.

Pinned examples:

```json
{"name":"api","protocol":"http","address":"127.0.0.1:8080"}
```

```json
{"name":"secure-api","protocol":"https","address":"[::1]:443","guest_port":8443}
```

The constructor and `with_guest_port` behavior are unchanged except for the
generalized protocol type name.

NNC1.3 does not invent endpoint identity from an IP/port or generate a new ID
during inspection. `PublishedEndpoint` is the observed location value; NNC1.5
will compose it into the distinct desired/durable/observed status model with
the already-landed `PublishedEndpointId` and `NetworkResourceGeneration`.
That sequencing preserves wire parity here without weakening the binding that
address is never identity.

## Ownership and Dependency Proof

Static source checks prove:

- `crates/nimbus-sandbox/src/endpoint.rs` does not exist;
- zero Rust source files contain `PublishedEndpointProtocol`;
- zero Rust source files import either endpoint type through
  `nimbus_sandbox`;
- zero sandbox files reference the deleted `crate::endpoint`;
- `nimbus-network` is the sole definition owner; and
- the only public re-exports are the owner crate and the public `nimbus`
  facade, whose source is explicitly `nimbus_network`.

The affected direct consumers now declare the downward edge appropriate to
their source context:

| Consumer | Edge kind |
| --- | --- |
| `nimbus-sandbox` | normal |
| `nimbus-tenant` | normal |
| `nimbus-services` | normal |
| `nimbus-machine` | normal |
| `nimbus-system` | normal |
| `nimbus` facade | normal |
| `nimbus-node` | dev/test |
| `nimbus-server` | dev/test |

The CLI already consumes the public `nimbus` facade; after the facade rewrite,
that path resolves directly to the network owner rather than a sandbox
re-export.

A freshly generated dependency graph proves all six normal/dev/all-feature
host and target profiles remain acyclic. The network crate still has exactly
one outgoing workspace edge:

```text
nimbus-network -> nimbus-core
```

## Behavioral Verification

Affected library suites:

| Crate | Passed | Ignored | Notes |
| --- | ---: | ---: | --- |
| `nimbus-network` | 15 | 0 | Includes four endpoint wire/API tests. |
| `nimbus-machine` | 16 | 0 | Machine API shapes remain valid. |
| `nimbus-node` | 46 | 0 | Test-only protocol consumer migrated. |
| `nimbus-sandbox` | 243 | 16 | Both container/krun endpoint and manifest paths pass; existing expected-red/baseline ignores remain. |
| `nimbus-services` | 93 | 1 | Endpoint ranking/resolution pass; existing NNC0.6 expected red remains ignored. |
| `nimbus-system` | 72 | 0 | Ordinary local lane used the required fixture-disable guard. |
| `nimbus-tenant` | 93 | 0 | Admission/policy serialization remains green. |

The first `nimbus-system` run intentionally omitted the fixture-disable guard
and failed only the three environment contracts for absent LibSQL, MySQL, and
Postgres URLs. The required rerun used
`NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1` and passed 72/72. Those
three guarded cases did not exercise live providers and are not claimed as
provider evidence; the other 69 system tests executed normally.

Focused public/API consumers:

| Surface | Passed | Filtered |
| --- | ---: | ---: |
| CLI compose | 113 | 745 |
| CLI machine/API | 22 | 836 |
| server service-manager routes | 26 | 487 |
| server tenant-isolation drift | 3 | 510 |
| server tenant-isolation harness | 1 | 512 |

The affected ten-crate all-target `cargo check` and all-target Clippy with
`-D warnings` pass. Only pre-existing warnings from the vendored Brotli sources
appear outside the workspace targets.

Additional gates:

```text
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

## Static Verifier State

The expected-red verifier remains at ten passes and two deliberately later
failures:

- `NNCV005` still names the old port-allocation authorities; and
- `NNCV011` now names only `NetworkSegment` and
  `NetworkSegmentAllocator`.

The endpoint definition failures have disappeared, proving NNC1.3 advanced the
intended condition without hiding the NNC1.4 work.

## Independent Review

The repository `autoreview` workflow reviewed the complete staged NNC1.3 diff
with Claude Opus 4.8 at maximum reasoning. It reported no accepted or
actionable findings and assessed the patch as correct with 0.83 confidence.
The review independently confirmed:

- exactly one canonical endpoint-vocabulary owner in `nimbus-network`, with no
  sandbox alias, re-export, or remaining `PublishedEndpointProtocol` name;
- exact serialized and API parity, including the pinned IPv4, IPv6,
  `guest_port`, and unknown-protocol cases;
- complete direct-consumer rewiring without a dependency cycle or an added
  `nimbus-network` workspace dependency beyond `nimbus-core`;
- no transport, binding, policy, service-name, or provider effect moved into
  the control-plane crate;
- the NNC1.5 identity/generation-envelope deferral is contract-consistent and
  avoids inventing workload identity from an address; and
- the external-provider caveat and the single-active-item ledger state are
  truthful.

## Scope Guard

NNC1.3 does not:

- move logical service names, selection, readiness, or binding out of
  `nimbus-services`;
- move sandbox manifests, probes, binds, TLS, or provider effects;
- create an optional DNS/name provider;
- fabricate stable identity from an address; or
- define the desired/durable/observed state envelope ahead of NNC1.5.
