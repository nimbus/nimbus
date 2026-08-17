# NNC1.6 Static Verifier Deepening Proof

Date: 2026-07-23

Status: `passed`

Source commit before the item:
`6c5b1d767`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Result

The network control-plane verifier now has three distinct, named regression
conditions:

| Condition | Contract | Current result |
| --- | --- | --- |
| `NNCV012 forbidden-network-dependencies-effects` | `nimbus-network` has no upper workspace, transport/provider, or cloud-SDK dependency; implements no socket/process/provider effect; portable segment source contains no OCI realization. | `PASS` |
| `NNCV013 single-network-definition-owner` | Every public network contract has one definition, stable IDs have one domain macro owner, and former core/sandbox owners expose no compatibility alias. | `PASS` |
| `NNCV014 address-is-not-network-identity` | Network IDs cannot be IP/socket/CIDR/port/index-backed or constructed through address conversions; segment identity remains distinct from CIDR location. | `PASS` |

Before NNC1.6 these named conditions did not exist. Injecting these regression
families therefore could not produce a condition-specific failure. The item
adds the conditions, direct positive scans, and fail-closed meta-tests rather
than relabeling an existing failure.

## Modular Verifier Seam

The existing shell verifier is the composition root for plan, metadata,
inventory, source, and ledger checks. It was already roughly 800 lines and
contained a large embedded JavaScript scanner for bind-census classification.
NNC1.6 does not add another embedded parser.

The new
`scripts/verify-nimbus-network-source-contract.mjs` is a focused, 499-line
source-contract engine with three explicit modes:

```text
forbidden-dependencies-effects
single-definition-owner
address-is-not-identity
```

`scripts/verify-nimbus-network-control-plane.sh` invokes each mode separately
and maps its result to a stable condition ID. A missing helper, missing source
root, invalid/missing Cargo metadata, missing identity/segment owner, or empty
definition set is a named failure rather than an empty-scan pass.

The helper has no package dependency. It walks production Rust sources,
excludes integration-test/benchmark locations, masks comments and Rust
string/raw-string/byte-string/C-string/character literals while preserving
line offsets, and removes exact `#[cfg(test)]` items before matching. A
positive-control fixture proves words in comments and strings do not create a
provider-effect false positive.

Filesystem APIs are deliberately not forbidden: NNC2.1 must implement the
network-owned crash-safe local store. The effect condition rejects transport,
provider command execution, network namespaces, and explicit provider/transport
dependencies without preventing the later durable-store implementation.

## NNCV012 — Dependency And Effect Locality

The dependency half consumes `cargo metadata --no-deps --format-version 1`
and rejects:

- every workspace dependency except `nimbus-core`, including dev/build edges;
- Axum, Pingora, Netavark, Iroh, openraft, socket/HTTP/RPC/TLS/DNS transport
  crates;
- common cloud SDK package prefixes; and
- a missing `nimbus-network` package or metadata failure.

Cargo metadata dependency names are package names, so a manifest rename does
not bypass the classifier.

The production source half rejects:

- TCP/UDP/Unix listener or socket binds;
- TCP/Unix stream connects;
- direct standard/Tokio transport types;
- process-command execution;
- Axum, Pingora, Netavark, Iroh, openraft, namespace/mount/network, and raw
  libc socket effects; and
- bridge/interface/network-name/Netavark realization fields in portable
  `segment.rs`.

`SocketAddr`, `IpAddr`, and `Cidr` remain permitted as provider-neutral value
types. Provider capability vocabulary may be added in its owning NNC4 item;
this condition rejects implementation paths and dependencies, not words in
documentation.

## NNCV013 — One Definition Owner

The verifier discovers every public `struct`, `enum`, `trait`, and type alias
under `nimbus-network`, then requires exactly one production definition across
the workspace.

The eight macro-generated stable ID domains receive stronger checks:

- each has exactly one `define_stable_resource_id!` invocation in
  network-owned `identity.rs`;
- none has a second concrete definition in any production crate; and
- the shared macro has exactly one opaque `String` backing field.

Former owners `nimbus-core` and `nimbus-sandbox` are scanned for public
compatibility aliases/re-exports of endpoint, attachment, or segment
vocabulary. The intended top-level `nimbus` façade remains allowed; it
re-exports the canonical owner rather than claiming a second definition.

## NNCV014 — Location Is Not Identity

Production network source fails if an `*Id`:

- is a tuple/newtype or type alias over an IP address, socket address, CIDR, or
  integer;
- is stored in an `_id`/`id` field using one of those location types;
- implements address-to-ID or ID-to-address conversion;
- is returned from an ID constructor accepting address/CIDR/port input; or
- assigns an address/CIDR/port expression to a stable network-ID field.

The condition also positively requires `AllocatedSegment` to carry both
`NetworkSegmentId` identity and `Cidr` location as distinct fields. It does
not reject `PublishedEndpoint.host: SocketAddr` or other observed locations
merely for existing.

## Fail-Closed Meta-Test Matrix

The shell verifier self-test expanded from 7 to 15 cases:

| Fixture/control | Required result |
| --- | --- |
| Missing canonical plan | exclusive `FAIL NNCV001` |
| Missing bind inventory | exclusive `FAIL NNCV002` |
| Missing dependency baseline | exclusive `FAIL NNCV002` |
| Injected production bind | exclusive `FAIL NNCV006` |
| Production bind after a test-only item | exclusive `FAIL NNCV006` |
| Pure `#[cfg(test)]` bind | `PASS NNCV006` |
| Missing core source | exclusive `FAIL NNCV010` |
| Injected `nimbus-tenant` dependency | exclusive `FAIL NNCV012` |
| Injected `axum` dependency | exclusive `FAIL NNCV012` |
| Injected `aws-sdk-ec2` dependency | exclusive `FAIL NNCV012` |
| Injected `TcpListener::bind` effect | exclusive `FAIL NNCV012` |
| Injected duplicate `NetworkPlan` definition | exclusive `FAIL NNCV013` |
| Injected CIDR-backed `NetworkSegmentId` | exclusive `FAIL NNCV014` |
| Provider-effect terms only in comment/string | `PASS NNCV012` |
| Missing network source root | `FAIL NNCV012`, `NNCV013`, and `NNCV014` |

Each injected failure requires its named `FAIL` line and forbids the matching
`PASS` line. The aggregate nonzero exit caused by later expected-red
conditions cannot satisfy these assertions.

The three dependency fixtures enter the same list produced from Cargo
metadata before the unchanged production predicate. They are not
special-cased into an error:

- `nimbus-tenant` exercises the forbidden workspace-edge branch;
- `axum` exercises the explicit transport/provider blocklist; and
- `aws-sdk-ec2` exercises the cloud-SDK prefix classifier.

The comment/string positive control proves the real source/dependency baseline
can pass `NNCV012`; therefore an always-failing condition cannot make the
negative fixtures look valid.

```text
timeout 900 bash scripts/verify-nimbus-network-control-plane.sh --self-test

self-test: 15 passed, 0 failed
```

## Current Aggregate Result

```text
PASS NNCV000-NNCV004
FAIL NNCV005 no-duplicate-port-allocation-authority
PASS NNCV006-NNCV010
FAIL NNCV011 single-portable-vocabulary-owner
PASS NNCV012 forbidden-network-dependencies-effects
PASS NNCV013 single-network-definition-owner
PASS NNCV014 address-is-not-network-identity

Summary: 13 passed, 2 failed
```

The remaining failures are deliberate, named later-band obligations:

- `NNCV005` identifies pre-NNC3 CLI/machine/sandbox port allocation
  authorities; and
- `NNCV011` identifies the private sandbox allocator trait that NNC2.2 moves
  and generalizes.

No later-band authority was hidden, allow-listed, or weakened to complete
NNC1.

## Verification

```text
bash -n scripts/verify-nimbus-network-control-plane.sh
# exit 0

shellcheck scripts/verify-nimbus-network-control-plane.sh
# exit 0

node --check scripts/verify-nimbus-network-source-contract.mjs
# exit 0

node scripts/verify-nimbus-network-source-contract.mjs \
  forbidden-dependencies-effects
node scripts/verify-nimbus-network-source-contract.mjs \
  single-definition-owner
node scripts/verify-nimbus-network-source-contract.mjs \
  address-is-not-identity
# each exit 0

npm exec --yes prettier -- --check \
  scripts/verify-nimbus-network-source-contract.mjs
# all matched files use Prettier code style

git diff --check
# exit 0
```

The normal aggregate verifier exits `1` exactly because `NNCV005` and
`NNCV011` remain red. Its `13 passed, 2 failed` result is the expected success
state for this extraction point.

## Independent Review

The first Claude Opus 4.8 maximum-reasoning review found one accepted gap:
`NNCV012` correctly rejected forbidden dependencies, but the then-12-case
meta-suite only injected its source-effect branch. The reviewer found the
production logic otherwise correct, including renamed-dependency behavior,
lexical masking, cfg-test handling, definition/macro/alias ownership,
address-versus-location semantics, missing-input behavior, Bash status
capture, and NNC2 filesystem compatibility.

The accepted gap was fixed by adding the three dependency-path fixtures above,
raising the suite to 15 cases. A maximum-reasoning Opus rerun reached its
explicit 30-minute timeout without returning a verdict; it is recorded as a
reviewer timeout, not approval. A configured GPT-5.6 Sol rerun correctly
refused nested execution inside this Codex-managed session and likewise
produced no verdict.

A narrow Claude Opus 4.8 high-reasoning rerun then reviewed the correction and
reported no accepted or actionable findings (`0.75` correctness confidence).
It independently confirmed that all three fixtures flow through the real
classifier, each shell assertion requires the named failure and forbids a
pass, the positive control prevents vacuity, the 7-to-15 count matches the
eight added cases, and no self-test hook affects non-child production runs.

## Scope Guard

NNC1.6 does not:

- make the expected-red port or allocator conditions green before their owner
  items;
- parse Rust semantically or claim a substitute for compile/test/model proof;
- forbid the filesystem operations required by NNC2.1;
- prohibit provider capability vocabulary that NNC4 deliberately surfaces;
- move a provider effect, transport, policy, naming, projection, or cluster
  implementation; or
- claim the final NNC9 verifier conditions are already complete.
