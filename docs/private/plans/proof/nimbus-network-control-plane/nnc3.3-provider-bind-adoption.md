# NNC3.3 Provider Bind And Adoption Proof

Date: 2026-07-24

Status: `passed`

Source commit:
`deb1df57f1e51c155bcee24ef8ce0aaeb30e70bc`

Upstream merge base:
`9c2d4f150c60f43dfdc0a3f1ec6550942e26ab8f`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

`nimbus-network` now owns the portable evidence exchanged across the
provider-effect boundary without owning the effect itself:

- `PortBoundEndpoint` is a concrete, non-zero endpoint proven after a
  successful bind or inherited-socket inspection;
- `PortBindAttempt` records the concrete operation that failed and preserves
  port zero when the durable request delegated numeric assignment;
- `PortBindingProvenance` distinguishes Nimbus-owned, provider-assigned, and
  externally owned sockets;
- `PortLeaseBinding` carries the exact endpoint, provenance, and a redacted
  provider-scoped handle; and
- `PortBindFailure` carries a stable failure category, exact attempted
  operation, and redacted provider-attempt handle.

The durable authority validates protocol, realm, target, requested/selected
port, and provenance before mutation. Binding adoption enters `Binding`;
failed-bind evidence enters `Failed`. Neither transition publishes the
listener. Activation is possible only from a matching adopted binding.

No provider trait was invented. The evidence contract is the earned seam:
effect-owning adapters retain real sockets and translate their native results
into these portable values. `nimbus-network` still performs no socket,
transport, proxy, firewall, DNS, or provider operation.

## Fail-Before

The integration contract was written before the production API:

```text
timeout 300 cargo test -p nimbus-testing \
  --test network_port_binding --no-run
```

It exited `101`. The crate had no `PortBindFailure`,
`PortBindFailureKind`, `PortBindingMismatch`,
`PortBindingProvenance`, or `PortBoundEndpoint`; it also had no
`LocalPortLeaseAuthority::record_bind_failure_without_effect` or durable
failure accessor.
The test therefore could not compile against the NNC3.2 lease authority.

An initial shell wrapper used zsh's read-only `status` name and was discarded
as invalid evidence. The command above was rerun directly and supplied the
recorded expected-red result.

## Real External-Binder Collision

`external_addr_in_use_is_durable_and_cannot_publish` uses two OS processes:

1. a child binds IPv4 loopback port zero, retains the listener, and reports
   the kernel-selected address through a semantic acknowledgement;
2. the parent constructs and reserves one exact stable lease for that
   already-owned port;
3. the parent performs the faithful provider-equivalent `TcpListener::bind`;
4. the kernel returns exactly `std::io::ErrorKind::AddrInUse`;
5. the adapter-side test maps that native error to portable evidence; and
6. the authority commits `Failed` without a binding or activation path.

The child owns the port before the lease request exists, so there is no
probe/drop selector window. The failure category, concrete attempted
operation, numeric port, and opaque
provider-attempt identity survive an authority restart. Activation and late
adoption both reject the terminal failed record. The child handshake and
cleanup are deadline-bounded and diagnostic; `wait-timeout` supplies a
blocking semantic process wait, and no sleep or busy-yield loop is used.

Confirmed pre-bind failure carries no provider effect. The separate NNC3.8
item remains the owner for ambiguous effects, cleanup-pending state, and the
final abandoned-reservation reuse rule.

## Pre-Bound And Systemd-Style Adoption

`externally_owned_prebound_listener_adopts_exact_identity_and_address` creates
a real loopback listener before opening lease authority. The authority:

- reserves the exact port under a stable `PortLeaseId` and owner ID;
- adopts the listener's actual protocol, realm, address, and port;
- records `ExternallyOwned` provenance and the provider-scoped
  `systemd:nimbus.socket:fd0` handle;
- activates only after durable adoption;
- preserves the evidence across restart; and
- withdraws publication without closing the externally owned socket.

After withdrawal, a different stable lease cannot reserve the same address
and port because the `Withdrawing` record retains the fence. A real client
still connects and the original listener still accepts, proving that authority
mutation did not seize or close the external effect.

Negative tests prove a wildcard address cannot satisfy a specific request and
provider-assigned provenance cannot satisfy an exact request. Both rejections
leave the durable lease unchanged in `Reserved`.

## Validation And Trust Properties

- Bound endpoints and failed attempts reject unknown realm or target evidence.
- Successful bound endpoints require a non-zero port.
- Failed provider-assigned attempts preserve and validate port zero.
- Exact and range requests require Nimbus-owned bindings, provider-assigned
  requests require provider-assigned bindings, and inherited sockets require
  exact requests.
- Unknown desired realm/target evidence may be refined by a concrete provider
  observation, but positive IPv6 disjointness evidence cannot be weakened.
- Provider handles serialize for reconciliation but remain redacted in
  `Debug`, `Display`, and errors.
- Checksum-valid records with missing, simultaneous, or request-incompatible
  binding/failure evidence fail closed during startup.
- Checksum-valid failed range records cannot disagree with their atomically
  selected port, and provider-assigned failures cannot smuggle in a reserved
  non-zero port.
- Replaying identical failure evidence is idempotent; divergent evidence is a
  conflict and cannot rewrite the durable record.

`port_lease.rs` is 1,610 lines, within the repository's
1,500–1,999 explicit-justification band. It remains one concept-owned lifecycle
authority with its private invariant tests. Request/overlap and bind-evidence
vocabulary are already split into concept-owned children; provider effects and
listener orchestration are forbidden from extending the parent. The owning
plan requires moving the private test module intact before 2,000 lines.

## Deferred Production Migrations

This item does not migrate a production listener or choose a concrete provider.
Those ownership transfers remain sequenced:

- NNC3.4: sandbox endpoint, PEP listener, and OCI `MachinePortProxy`;
- NNC3.5: server and sibling wire listeners;
- NNC3.6: standalone `nimbus-kv`;
- NNC3.7/3.7a: machine and CLI listeners;
- NNC3.7b: close the complete bind census;
- NNC3.8: crash/cleanup-pending and abandoned-reservation rules; and
- NNC3.9: delete old production scanners and allocators.

NNCV005 therefore remains the one expected aggregate failure.

## Verification

```text
cargo fmt --all --check
# exit 0

cargo test -p nimbus-network --all-features -- --test-threads=1
# unit: 80 passed; 0 failed; 0 ignored
# port_conflict_model: 6 passed; 0 failed; 0 ignored
# doc tests: 0 failed

cargo test -p nimbus-testing --test network_port_binding \
  -- --test-threads=1
# 3 passed; 0 failed; 1 ignored child entrypoint

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
[{"name":"nimbus-core","kind":null,"optional":false,"target":null}]

rg production socket/process effects in crates/nimbus-network/src
# zero matches

rg forbidden upper/transport/provider dependencies in nimbus-network
# zero matches

bash -n scripts/verify-nimbus-network-control-plane.sh
shellcheck -s bash scripts/verify-nimbus-network-control-plane.sh
git diff --check
# all exit 0

bash scripts/verify-nimbus-network-control-plane.sh --self-test
# 16 passed; 0 failed

bash scripts/verify-nimbus-network-control-plane.sh
# 14 passed; 1 failed; exit 1 exactly as expected
# only NNCV005, owned by NNC3.4-NNC3.9

bash scripts/check-docs.sh
# 108 pages link-clean; source map resolves; private fence intact; titles unique

bash scripts/verify-nimbus-docs-site.sh
# 17/17 conditions green
```

## Worktree Isolation

The implementation was performed only in the dedicated owner worktree and
branch. The original checkout still has exactly its four pre-existing
user-owned paths:

```text
 M docs/private/plans/README.md
A  docs/private/plans/nimbus-runtime-tenant-isolation-plan.md
 M docs/private/plans/research/concurrent-write-throughput-benchmark.md
?? demos/convex/vendor/browser.bundle.js
```

No original-checkout file was modified, staged, discarded, or overwritten.
No push or pull request was performed.

## Independent Review

The first owner-directed Sol pass found three accepted P2 issues:

- failed range/provider-assigned records lacked a complete selected-port
  corruption check;
- the external-binder test closed a selector socket before the child bind; and
- child exit used a busy-yield polling loop.

All three were corrected with two new checksum-valid corruption cases, a
child-owned kernel-selected port established before lease construction, and
the bounded `wait-timeout` process-wait primitive. The same complete-bundle
review was then rerun:

```text
/Users/jack/src/github.com/nimbus/agent-skills/skills/autoreview/scripts/autoreview \
  --mode local --engine codex --model gpt-5.6-sol \
  --thinking xhigh --codex-speed fast --stream-engine-output
```

Claude Opus 4.8 is explicitly excluded from this and future reviews.

The 109,628-byte rerun reported zero findings:

```text
autoreview clean: no accepted/actionable findings reported
overall: patch is correct (0.88)
```

The reviewer concluded that binding/failure validation is consistent, lease
state remains atomic, provider effects remain outside `nimbus-network`, and
the process/durability tests exercise the intended behavior without a concrete
correctness, security, architecture, or release-blocking defect.
