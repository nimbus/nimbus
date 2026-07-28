# NNC3.9 Single Port Authority Deletion Proof

Date: 2026-07-28

Status: `complete`

Starting checkpoint:
`129a06ea2620e0c250de7e6ed496b143431a2511`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Written Acceptance

NNC3.9 is complete only when all of these clauses pass:

| ID | Required result | Direct proof |
| --- | --- | --- |
| D1 | No old production allocator, availability probe, `PortManager` type, or `port_manager` module remains. | NNCV005 passes, the exact legacy-name scan is empty, and no compatibility alias or forwarding module exists. |
| D2 | Sandbox composition continues to consume the one `LocalPortLeaseAuthority`; it does not become a second allocation authority. | The renamed adapter delegates exact/range reservation, quota, conflict, generation, and release transitions to `nimbus-network`; focused and full sandbox behavior remains green. |
| D3 | Every production bind/allocation/adoption occurrence is current and classified. | Regenerated inventory, source-derived census, AST tests, and fail-closed verifier self-tests agree exactly with zero unclassified sites. |
| D4 | Compiler-resolved and generated-code evidence finds no hidden production allocator or socket authority. | Affected all-target/all-feature compilation, resolved MIR/source boundary checks, the named conditional/include/alias/qself/adoption scans, and every current generated Firebase/Tonic output are recorded with exact results. NNC9.1 still owns making this evidence a permanent complete-verifier condition. |
| D5 | The architectural dependency and effect boundaries do not move. | `nimbus-network -> nimbus-core` remains its only workspace edge; sockets, Netavark, gvproxy, PEP, provider inspection, and sandbox orchestration remain above it. |

## Current Authority Audit

The expected-red closeout verifier is exact:

```text
bash scripts/verify-nimbus-network-control-plane.sh
```

Result: exit `1`; `14 passed, 1 failed`. The sole failure is:

```text
FAIL NNCV005 no-duplicate-port-allocation-authority
     crates/nimbus-sandbox/src/backends/oci/port_manager.rs:55:
     pub(crate) struct PortManager {
```

NNCV006 already passes. Its source census classifies `67/67` current
authority/ownership occurrences and `35/35` non-authority risks with zero
unclassified sites. One of the 67 occurrences is the deliberately retained
`legacy-port-manager-definition`; all legacy availability functions named by
NNCV005 are already absent. The verifier self-test passes `44/44`.

Source and caller inspection proves that `PortManager` is no longer a port
allocation authority:

- `reserve_launch_ports_for_sandbox` builds the sandbox request batch, then
  delegates the atomic exact/range choice, quota, conflict, and durable
  lifecycle to `LocalPortLeaseAuthority`;
- Netavark, MachinePortProxy, PEP, and socket effects remain in their sandbox
  adapters;
- the type stores only the network state root, desired range, admission limit,
  and the sandbox provider mode needed to compose those owners; and
- no method scans manifests, probes availability, binds a socket, or chooses a
  host-global port outside the canonical lease transaction.

The remaining defect is an obsolete authority-shaped generic name and module
that obscures this composition role. The clean replacement is:

```text
oci::port_lifecycle::OciPortLeaseCoordinator
```

There is no alias, deprecated re-export, forwarding module, or migration shim.
The internal callers move atomically to the new name and module.

## Owned Change Set

The source-derived audit finds 38 Rust paths containing the old type/module
name. NNC3.9 owns only their mechanical import/type/accessor rename plus these
direct verifier and evidence owners:

- move `crates/nimbus-sandbox/src/backends/oci/port_manager.rs` to
  `crates/nimbus-sandbox/src/backends/oci/port_lifecycle.rs`;
- move the seven source/test children under
  `crates/nimbus-sandbox/src/backends/oci/port_manager/` to the corresponding
  `port_lifecycle/` paths;
- update the remaining sandbox callers returned by
  `rg -l '\bPortManager\b|port_manager' crates/nimbus-sandbox --glob '*.rs'`;
- refresh
  `docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json`;
- strengthen `scripts/verify-nimbus-network-control-plane.sh` so reintroducing
  an old type/module/accessor fails NNCV005 in its self-test;
- update this proof, the canonical plan, and the plan routing index.

No `nimbus-network`, server, KV, CLI, compute, tenant, service, proxy,
projection, or cluster implementation path is admitted. Any behavior change
outside the sandbox composition name/move requires a new owner decision.

## Modularity Decision

The moved `port_lifecycle.rs` is an explicit 1,500-1,999-line deep-module
exception for this deletion item. It owns one sandbox-side lifecycle facade
over a single state root and one exact portable authority; its state-machine
methods must remain co-located so callers cannot compose partial claim,
activation, cleanup, and release sequences. Existing concept-owned children
already isolate batch classification, Netavark lifetime recovery, and tests.
NNC3.9 adds no behavior or switchboard branch. Any later production logic must
land in a concept-owned child rather than increasing the facade.

The moved private `port_lifecycle/tests.rs` is a separate 1,500-1,999-line
test-module exception at 1,921 lines. It is the concept-owned root for this
facade's shared fixtures, cross-process characterization harness, and primary
end-to-end lease-lifecycle tests; its four existing child modules isolate
batch classification, machine cleanup, Netavark lifetime cleanup, and
teardown progress. NNC3.9 moves this root intact and adds no test logic.
Splitting shared fixtures from the tests that consume their private protocol
would obscure the one lifecycle test seam without reducing production
complexity. Any new independent test family must land in a concept-owned
child rather than increasing this root.

## Implementation Result

The clean replacement is implemented atomically:

- the eight `oci/port_manager*` paths moved to the corresponding
  `oci/port_lifecycle*` paths;
- `PortManager` became `OciPortLeaseCoordinator`;
- caller fields, parameters, and accessors use `port_lease_coordinator`;
- module paths use `port_lifecycle`; and
- there is no old module, type alias, deprecated re-export, forwarding
  function, or compatibility shim.

Before staging, the 46 sandbox Rust status endpoints were exactly the 30
modified callers plus the eight deleted old paths and eight replacement
paths. The other four admitted implementation/evidence owners are the
inventory, verifier shell, this proof, and canonical plan. After Git detects
the eight moves, the frozen candidate is exactly 42 staged paths:
30 modified sandbox callers, eight renamed sandbox paths, and those four
evidence owners. The final item checkpoint adds only the plan routing index,
so its commit scope is 43 paths. No `nimbus-network` or later-band
implementation source changed.

`OciPortLeaseCoordinator::reserve_launch_ports_for_sandbox` still builds the
provider-specific request batch and then delegates the one atomic decision:

- bounded tenant admission uses `reserve_batch_with_tenant_limit`;
- unbounded admission uses `reserve_batch`; and
- inspect, adoption, activation, cleanup, and release open the same
  `LocalPortLeaseAuthority`.

The coordinator owns sandbox provider composition and lifecycle ordering. It
does not choose a port, implement quota/conflict/fencing, or bind a socket.

## Authority And Behavior Evidence

The exact old-authority scan is empty:

```text
rg -n \
  'PortManager|port_manager|fn resolve_listener_port|fn ephemeral_port|fn allocate_machine_ssh_port|fn machine_port_is_available' \
  crates/nimbus-sandbox/src crates/nimbus-cli/src

rg --files crates/nimbus-sandbox/src/backends/oci | rg 'port_manager'
```

Both searches exit `1` with zero matches. The live verifier now exits `0`:

```text
PASS NNCV005 no-duplicate-port-allocation-authority
PASS NNCV006 unclassified-production-bind
...
Summary: 15 passed, 0 failed
```

The regenerated source-derived inventory retains the same 26 logical sites
and classifies:

- `66/66` authority/ownership occurrences;
- `35/35` non-authority risks; and
- zero unclassified production sites.

The one removed occurrence was the obsolete
`legacy-port-manager-definition`. No real effect site disappeared.

Behavior is unchanged:

| Gate | Result |
| --- | --- |
| `cargo test -p nimbus-sandbox --lib port_lifecycle` | `47 passed; 2 ignored`; the ignores are the existing allocation-scale characterization and child-only process role |
| `cargo test -p nimbus-sandbox --lib` | `683 passed; 24 ignored` |
| `cargo test --manifest-path scripts/nimbus-network-bind-census-ast/Cargo.toml --locked --offline` | `8 passed; 0 failed` |
| `cargo check -p nimbus-sandbox --all-targets --all-features` | pass; only pre-existing vendored Brotli warnings |

The verifier gains a fail-closed child injection that reintroduces synthetic
`PortManager` authority vocabulary and proves an exclusive NNCV005 failure.
The first run executed all 45 branches successfully and exposed only a stale
hard-coded final summary string (`44`). After correcting that reporting
literal, the frozen verifier rerun passes exactly:

```text
self-test: 45 passed, 0 failed
```

## Compiler-Resolved And Generated-Code Closure

The affected all-feature sandbox library was lowered to resolved MIR:

```text
cargo rustc -p nimbus-sandbox --lib --all-features \
  -- --emit=mir=/tmp/nnc39-sandbox.mir
```

The result is `19,608,127` bytes. It contains:

- zero `PortManager`, `port_manager`, `resolve_listener_port`,
  `ephemeral_port`, `allocate_machine_ssh_port`, or
  `machine_port_is_available` symbols;
- zero `reserve_port` or `allocate_port` symbols;
- exactly one resolved `std::net::TcpListener::bind`, at the source-mapped
  `MachinePortProxy::prepare` call already classified as the
  `machine-port-proxy-bind` provider-effect site; and
- only `File::from_raw_fd` for the two unrelated raw-FD MIR matches.

The current-source boundary scans find:

| Shape that can escape a bounded source AST census | Current result |
| --- | --- |
| `cfg_attr(..., path = ...)` | zero |
| qself `<... as ...>::bind` | zero |
| `std::net` / `tokio::net` / `socket2` glob imports | zero |
| a socket-type impl with a `Self` associated alias | zero |
| a directly port/listener/socket-named `#[cfg(test)]` provider field | zero |
| `from_std` / `from_raw_fd` / `from_owned_fd` syntax | 15 total: eight production occurrences classified in the inventory and seven tests in `nimbus-server::listener_lease` |

The only computed production `include!` boundary is
`crates/nimbus-firebase/src/grpc.rs`. The current build has three generated
output directories with the same five files each: 15 Rust outputs totaling
`405,012` bytes. All 15 have zero occurrences of `TcpListener`, `UdpSocket`,
`UnixListener`, `UnixDatagram`, `Socket::bind`, `from_std`, `from_raw_fd`,
`host_port`, `port_mappings`, `reserve_port`, or `allocate_port`. Their five
unique SHA-256 hashes, repeated identically in all three directories, are:

| Generated file | SHA-256 |
| --- | --- |
| `firebase_grpc.rs` | `b7d837be7005640b841cd7e3d7bf231a43629a5bab2901f52c59fd00481dd888` |
| `google.api.rs` | `70462b1371c306c2c1e0316c0b4cb643b0a5dceec687989833dd0183f80f9730` |
| `google.firestore.v1.rs` | `e203487d079c81506001b5ec87e853d0ed68012858ab8779cd983f5bcec36505` |
| `google.r#type.rs` | `9726883f3f1c4511dbcabd8576f1071480c0bc00fc6bca6db778adb6620b8191` |
| `google.rpc.rs` | `0070e667eb36e003873ac8ed207c467999217ac7edb59e978c68b2585d41d022` |

This closes D4 for the current NNC3.9 source/build state without claiming that
the standalone AST scanner supplies general Rust name resolution. NNC9.1
still owns turning the compiler/generated-code evidence into a permanent
complete-verifier condition.

## Dependency And Effect Boundary

`cargo metadata --format-version 1 --no-deps` reports exactly one normal
workspace dependency for `nimbus-network`: `nimbus-core`. The manifest still
contains no sandbox, server, tenant, service, proxy, transport, Netavark,
gvproxy, cloud SDK, or cluster edge. Resolved sockets and provider effects
remain in the sandbox and other upper-layer adapters.

## Acceptance Ledger

| ID | Status | Evidence |
| --- | --- | --- |
| D1 | pass | Legacy type/module/probe and filesystem scans are empty; NNCV005 passes; no alias/shim exists. |
| D2 | pass | The lifecycle coordinator delegates the one atomic decision to `LocalPortLeaseAuthority`; focused 47/2 and full sandbox 683/24 pass. |
| D3 | pass | Inventory is exact at 66/66 + 35/35 across 26 sites; AST is 8/8; verifier self-test is 45/45; NNCV006 passes. |
| D4 | pass | All-target/all-feature compile, 19.6 MB resolved MIR, named source-boundary scans, and 15 generated outputs find no hidden authority. |
| D5 | pass | `nimbus-network` retains exactly the `nimbus-core` workspace edge and zero provider effects. |

## Quality And Documentation Gates

| Gate | Result |
| --- | --- |
| `cargo fmt --all --check` | pass |
| `git diff --check` | pass |
| `bash -n scripts/verify-nimbus-network-control-plane.sh` | pass |
| `shellcheck scripts/verify-nimbus-network-control-plane.sh` | pass |
| `jq empty .../nnc0.1-bind-owner-inventory.json` | pass |
| `make clippy` | pass, workspace/all-targets with `-D warnings`; only pre-existing vendored Brotli diagnostics are emitted outside Nimbus source |
| `RUSTDOCFLAGS='-D warnings' cargo doc -p nimbus-sandbox --no-deps --all-features` | pass |
| `bash scripts/check-docs.sh` | `108` pages link-clean; source map, private fence, and titles pass |
| `bash scripts/verify-nimbus-docs-site.sh` | `17/17` conditions green |

## Structured Review

The one full-candidate structured review invocation used GPT-5.6 Sol, xhigh
reasoning, and fast mode against a `582,286`-byte bundle. The helper split that
single invocation into two bundle chunks; this is not two review invocations.
Chunk 1 (`019fa883-f7ad-7b20-a528-d16559205cf6`) reported zero findings,
`patch is correct`, confidence `0.94`. Chunk 2
(`019fa884-9bbe-7211-9bbe-4bc664d06952`) reported three P2 closeout findings:

| Finding | Disposition |
| --- | --- |
| The synthetic legacy-authority self-test did not prove NNCV005 was the only failure. | Accepted. It now requires exactly one `FAIL` line and the exact `Summary: 14 passed, 1 failed` output in addition to the named NNCV005 failure and absent NNCV005 pass. |
| The 1,921-line moved private test root lacked its own modularity exception. | Accepted. The separate test-root exception above records its shared-fixture/process-harness/primary-lifecycle ownership and requires new independent families to land in concept-owned children. |
| The recovery header's pre-staging path count did not describe the frozen candidate. | Accepted with an evidence correction. `git diff --cached --name-only` proves exactly 42 staged paths after Git detects eight renames; 50 counts the old and new endpoints separately before rename detection and is not the staged candidate path count. The header and proof now use 42. |

The executable correction is limited to the verifier self-test. Its direct
child proof exits `1` with exactly one NNCV005 failure and `14/1`; the normal
verifier remains `15/15`; Bash syntax, ShellCheck, and diff checks pass; and
the complete corrected verifier self-test passes `45/45`. Product source did
not change, so the already-green sandbox/MIR/generated-code gates remain the
applicable product evidence.

Because the accepted exclusivity finding changed executable verifier behavior,
one narrow correction review was required. It reviewed only the three-file,
`18,190`-byte correction bundle in one pass using GPT-5.6 Sol, xhigh reasoning,
and fast mode. Thread `019fa893-eecb-7810-a881-e61f228dc23e` reports zero
findings, `patch is correct`, confidence `0.98`, and independently confirms:

- the child requires a nonzero exit, named NNCV005 failure, absent NNCV005
  pass, exactly one `FAIL`, and exact `14/1` summary;
- the separate 1,921-line test-root exception is credible and explicit; and
- the reviewed candidate is exactly 42 staged paths after eight rename
  detections.

No further review is required because the remaining changes are this result,
the ledger transition, and routing-index status only.

## Final Result

NNC3.9 passes D1-D5. The obsolete authority-shaped sandbox type/module and all
old probe/drop allocator names are deleted without a compatibility layer. One
host-global `LocalPortLeaseAuthority` remains, provider effects remain above
`nimbus-network`, the complete current source/compiler/generated-output census
is classified, and every behavior, quality, documentation, and review gate
listed above is green. NNC3's band gate is closed.
