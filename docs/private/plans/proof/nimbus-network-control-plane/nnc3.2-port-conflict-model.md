# NNC3.2 Portable Port-Conflict Model Proof

Date: 2026-07-24

Status: `passed`

Source commit:
`003806f008bf648de2482a4b0b722420486a6d4a`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

`nimbus-network` now owns a portable, serialized conflict domain for
host-global port leases. One immutable `PortBindingSpec` records:

- TCP or UDP;
- host, unknown, or stable proven-isolated bind realm;
- unknown, IPv4, or IPv6 bind target;
- wildcard or specific address;
- conservative unknown, known dual-stack, or proven-disjoint IPv6 behavior;
- loopback, private, public, or unknown exposure metadata; and
- exact, inclusive range, or provider-assigned port selection.

Exposure describes policy intent but never weakens kernel overlap. Unknown
realm, target, or IPv6 behavior fails closed by overlapping every domain it
could occupy. Different protocols and genuinely disjoint address/realm
domains may reuse the same number.

The stable lease and owner IDs, generation, and epoch remain the authority
identity. An IP address remains location data only.

## Fail-Before

The complete integration contract was written before the request model:

```text
timeout 300 cargo test -p nimbus-network --test port_conflict_model --no-run
```

It exited `101` with 17 compiler errors. The new request types,
`PortLeaseRequest::new`, `PortLeaseRecord::reserved_port`, and
`PortLeaseError::PortRangeExhausted` did not exist, so the conflict matrix
could not compile against the NNC3.1 exact-only authority.

## Conflict And Allocation Semantics

The implemented relation is deliberately small and explicit:

```text
same protocol
AND overlapping bind realm
AND overlapping address/family target
AND same selected numeric port
=> conflict
```

Realm rules:

- `Unknown` overlaps host and every isolated realm;
- host overlaps host;
- the same proven-isolated identity overlaps itself; and
- two distinct proven-isolated identities, or host and a proven-isolated
  identity, are disjoint.

Address rules:

- unknown overlaps every target;
- wildcard overlaps every target in its family;
- equal specific addresses overlap and unequal specifics are disjoint;
- IPv4 and IPv6 overlap unless the IPv6 target carries
  `ProvenDisjoint`; and
- IPv4-mapped IPv6 input is rejected so it cannot bypass the canonical
  cross-family rule.

Exact allocation atomically fences its number. Range allocation atomically
selects the lowest free slot in the requested overlap domain; it does not
reserve an entire range. Provider-assigned reservation first persists only
the stable lease identity, then adoption atomically checks and fences the
provider's actual non-zero port. Failed exhaustion or adoption publishes no
partial numeric authority.

All wire types reject unknown fields where structure permits it. Realm IDs
are non-empty, at most 128 bytes, and restricted to a portable ASCII
alphabet. Inclusive ranges cannot be reversed, including through
deserialization.

## Durable Validation And Concurrency

Authority startup rejects checksum-valid records when:

- an exact/range request lacks a selected port;
- a selected port falls outside the exact/range request;
- adopted provider evidence disagrees with the selected port; or
- two non-terminal records overlap on protocol, realm, address, and number.

The generated/unit relation proofs cover:

- the complete 4-by-4 realm truth table in both orders;
- complete wildcard/specific truth tables for each address family;
- every IPv4/IPv6 target pair for all three IPv6 evidence values in both
  orders; and
- 48 binding specifications across 2,304 ordered pairs, proving reflexivity,
  symmetry, and TCP/UDP separation.

The integration suite adds 18 named positive/negative conflict cases and
separate exact/range/provider-assigned lifecycle tests. The real-process
harness proves:

1. two exact contenders produce one active winner;
2. two overlapping range contenders atomically select distinct slots; and
3. two provider-assigned contenders adopting one number produce one binding
   winner while the loser remains reserved without a numeric fence.

No test relies on sleeps.

## Trust Boundary And Deferred Effects

`ProvenIsolated` and `ProvenDisjoint` are positive evidence claims, not
fallbacks. Production callers must use `Unknown` until an effect adapter can
establish the claim. NNC3.3 owns provider bind/adoption evidence; NNC4 owns
capability reporting and the production proof source. The conflict model
never invokes a socket, provider, firewall, proxy, DNS, or cluster transport.

This item does not migrate any production listener. NNC3.3 through NNC3.7b
own provider/adopted sockets and individual listener owners; NNC3.9 deletes
the old scan/probe authorities. Therefore NNCV005 remains the sole expected
aggregate verifier failure.

## Verification

```text
cargo fmt --all --check
# exit 0

cargo test -p nimbus-network --all-features -- --test-threads=1
# unit: 73 passed; 0 failed; 0 ignored
# port_conflict_model: 6 passed; 0 failed; 0 ignored
# doc tests: 0 failed

cargo test -p nimbus-testing --test network_port_lease \
  -- --test-threads=1
# 3 passed; 0 failed; 1 ignored child entrypoint

cargo check -p nimbus-network -p nimbus-testing \
  --all-targets --all-features
# exit 0

cargo clippy -p nimbus-network -p nimbus-testing \
  --all-targets --all-features -- -D warnings
# exit 0; only existing vendored Brotli warnings

cargo doc -p nimbus-network --all-features --no-deps
# exit 0

cargo metadata --format-version 1 --no-deps
# nimbus-network workspace edges:
[{"name":"nimbus-core","kind":null}]

bash -n scripts/verify-nimbus-network-control-plane.sh
shellcheck -s bash scripts/verify-nimbus-network-control-plane.sh
git diff --check
# all exit 0

bash scripts/verify-nimbus-network-control-plane.sh --self-test
# 16 passed; 0 failed

bash scripts/verify-nimbus-network-control-plane.sh
# 14 passed; 1 failed; exit 1 exactly as expected
# only NNCV005, whose production-owner migration remains NNC3.4-NNC3.9

bash scripts/check-docs.sh
# 108 pages link-clean; source map resolves; private fence intact; titles unique

bash scripts/verify-nimbus-docs-site.sh
# 17/17 conditions green
```

## Worktree Isolation

The implementation was performed only in the dedicated owner worktree and
branch. The original checkout retained its four pre-existing user-owned
paths. No push or pull request was performed.

## Independent Review

The repository autoreview workflow reviewed the complete 100,777-byte local
bundle with Claude Opus 4.8 at maximum reasoning:

```text
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local --engine claude --model claude-opus-4-8 \
  --thinking max --stream-engine-output

autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.8)
```

The reviewer independently traced the overlap relation, exact/range reserve,
provider-assigned adoption, corruption validation, real-process serialization,
serde validation, dependency direction, and plan/test evidence. It reported
no correctness, security, atomicity, wire-contract, or dependency-layering
defect requiring a fix.
