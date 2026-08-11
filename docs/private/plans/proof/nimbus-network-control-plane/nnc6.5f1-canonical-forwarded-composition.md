# NNC6.5f1 canonical forwarded composition and retirement facade

Status: `in progress; implementation complete and acceptance convergence in progress`

This record owns NNC6.5f1. It extracts one canonical forwarded workload
composition. It installs the exact forwarded teardown capability realm. It
also exposes the compute-owned retirement facade to foreground callers. It
does not cut over a teardown caller.

## Objective

Both forwarded server start and forwarded Compose up currently activate the
same machine source and independently assemble the same provision and restart
providers. Both omit the teardown registrations already retained by
`ForwardedMachineApiSandboxBackend`. NNC6.5f1 replaces those two assembly
roots with one concept-owned path:

```text
prepared forwarded source + frozen network + catalog + build admission
  -> one canonical prepared forwarded profile
  -> exact network selection validation
  -> one source activation
  -> one retained forwarded backend
  -> its attachment + execution + ingress teardown registrations
  -> one WorkloadTeardownCapabilityRegistry
  -> one ServerWorkloadComposition exact realm
  -> server AppState, or foreground ComputeState
  -> fallible ComputeResourceRetirer facade over that same compute realm
```

Local Krun composition remains unchanged. The foreground facade delegates to
`ComputeState::resource_retirer`. It does not cache, rebuild, or expose raw
store, coordinator, registry, runtime, or provider authority.

## Non-goals

NNC6.5f1 does not own:

- Compose down behavior or caller cutover.
- delete the coarse guest stop route, client, wire type, or backend call.
- physical-machine stop policy, admission guards, or barriers.
- failed-provision or failed-restart compensation.
- tenant retirement or final teardown convergence.
- provider effects, provider journal semantics, or Machine API protocol work.
- local Krun provider composition changes.
- service naming, tenant policy, proxy forwarding, egress, certificates, or
  cluster transport.
- any change to `nimbus-network` or its dependency graph.

## Acceptance contract

| ID | Verifiable success criterion |
| --- | --- |
| K1 | `network_composition/forwarded/profile.rs` is the sole forwarded activation and provider-composition owner. |
| K2 | `compose_forwarded_server` and `compose_forwarded_foreground` directly return `prepare_forwarded_workload_profile`; neither reconstructs or discards the canonical result. |
| K3 | Local Krun composition source and behavior remain unchanged. |
| K4 | The canonical path constructs one retained `ForwardedMachineApiSandboxBackend`, consumes its exact attachment, execution, and ingress teardown registrations, constructs exactly one `WorkloadTeardownCapabilityRegistry`, and passes it through `with_teardown_capabilities`. |
| K5 | `ServerWorkloadComposition::new` authenticates that registry against the frozen attachment, ingress, and execution provider identities before runtime authority exists. |
| K6 | Server and foreground callers preserve their distinct `LocalBuildAdmission` values. |
| K7 | One completion opens one machine source, one forwarded backend, and one forwarded teardown provider-command journal. No caller opens a second client, network manager, registry, or provider-command journal. |
| K8 | `ServerForegroundWorkloadRuntime::resource_retirer` returns the existing `ComputeResourceRetirer` through `ComputeState::resource_retirer`. It adds no cached authority or second resolution contract. |
| K9 | The foreground runtime exposes no `ComputeState`, workload store, saga coordinator, teardown registry, teardown runtime, dispatcher, or provider adapter. |
| K10 | A foreground runtime without exact teardown composition returns the existing precise compute error before source, saga, provider, network, or filesystem work. |
| K11 | An exact foreground teardown realm resolves a retirement facade without source, saga, provider, network, or filesystem effects. Repeated resolution reuses the same underlying `Arc` authorities. |
| K12 | A crossed attachment, ingress, or execution provider realm is rejected before runtime construction or provider effects. |
| K13 | The real forwarded adapter execute-substitution test dispatches withdrawal, drain, stop, detach, and release through compute exactly once and without fallback. |
| K14 | The real forwarded inspect-substitution test preserves inspect-before-retry and exact phase fencing. |
| K15 | Existing real local Krun composition and exact local provider tests remain green; no local registry constructor is added. |
| K16 | No manifest or dependency edge changes. No socket, Axum, Pingora, Netavark, nftables, gvproxy, Iroh, cluster, cloud, policy, service-name, or provider effect enters `nimbus-network`. |
| K17 | NNCV035 changes only its forwarded-registry diagnostic from red to green: live result becomes exact `0/6`; the aggregate remains exact `35/36` with only NNCV035 red. Its `138/138` helper and `552/552` aggregate mutation gates remain green. |
| K18 | Focused and full affected crate tests, Rust format, Clippy, dependency/effect scans, strict proof lint, docs, and site gates pass with exact counts recorded below. |
| K19 | Exactly one GPT-5.6 Sol/xhigh/fast item review runs only after K1-K18 are green and the item is candidate-frozen. A narrow correction review runs only after an accepted executable finding. |
| K20 | Plan, routing, proof, recovery state, and the exact item diff close in one reviewed item commit. |

## Current ownership and duplicated path

| Concern | Current source | Problem |
| --- | --- | --- |
| Server forwarded preparation | `network_composition/forwarded.rs` | Owns delayed source activation and constructs provision/restart providers, but omits teardown. |
| Compose foreground preparation | `compose/provision.rs` | Duplicates the server activation and provider construction and also omits teardown. |
| Retained teardown provider | `machine/backend.rs` and `machine/backend/teardown.rs` | Already owns one adapter and all five exact capability roles; callers do not consume them. |
| Exact realm authentication | `nimbus-server/workload_composition.rs` | Already accepts and validates an optional exact teardown registry. No new server registry seam is needed. |
| Foreground lifetime owner | `nimbus-server/workload_composition.rs` | Retains the exact `ComputeState`, but exposes only provisioning and services. |
| Retirement facade | `nimbus-compute/resource_retirement.rs` | Already resolves all five exact runtime authorities from `ComputeState` and fails closed if one is absent. |

Current duplicated flow:

```text
forwarded server root             forwarded Compose root
  -> source.activate                -> source.activate
  -> backend                        -> backend
  -> ServiceManager                 -> ServiceManager
  -> provision + restart only       -> provision + restart only
  -> server composition             -> server composition
```

Target flow:

```text
forwarded server root -----------\
                                  -> canonical forwarded profile composition
forwarded Compose root ----------/     -> one retained backend
                                         -> exact teardown registrations
                                         -> one exact registry
                                         -> server composition
```

## Canonical seam and ordering

`PreparedForwardedWorkloadProfile` owns only already-prepared inputs:

- frozen network composition.
- prepared default machine source.
- exact service catalog.
- the caller-specific local build admission.

`prepare_forwarded_workload_profile` runs these steps in order:

1. clone the source-owned selection, requirements, sovereignty, node, and
   execution-provider identity.
2. reselect the exact frozen network bundle before activation.
3. activate the prepared machine source once.
4. construct one concrete `ForwardedMachineApiSandboxBackend` from the exact
   returned client and provision adapter.
5. request teardown registrations from that concrete retained backend before
   type erasure.
6. consume the three registration groups into one exact registry.
7. construct the service manager from the same backend.
8. install provision, restart, and teardown capability sets into one server
   provider bundle.
9. let `ServerWorkloadComposition::new` authenticate exact network,
   sovereignty, and teardown realm identity.

The server and foreground consumer functions are thin tail calls. The local
profile keeps its current independent concept-owned composition.

## Foreground facade decision

The foreground runtime will add this narrow contract:

```rust
pub fn resource_retirer(&self) -> Result<ComputeResourceRetirer, ComputeError>
```

It delegates to the retained `ComputeState`. It does not add a field and does
not make `into_foreground_runtime` fallible. This preserves valid provision-
only generic server compositions while making retirement fail closed at the
existing compute boundary. NNC6.5f2 will consume the method and own caller
error handling.

Multiple facade values are not multiple authorities: they clone the same
services, provisioner, coordinator, restart runtime, and teardown runtime
`Arc`s. The one-authority invariant applies to those owners and their durable
stores and journals.

## Failure and no-effect matrix

| Condition | Required result | Forbidden result |
| --- | --- | --- |
| Frozen selection missing or changed | typed selection/composition error before source activation | machine activation, journal, listener, lease, or provider effect |
| Activated source differs from prepared source | fail closed through existing source authentication | fallback provider or reconstructed source |
| Backend lacks exact provision authority | `teardown_capabilities` precondition error | empty registry or provision-only retirement runtime |
| Duplicate or conflicting registration | exact registry construction error | last-writer-wins capability replacement |
| Crossed attachment, ingress, or execution identity | `IncompleteExactRealm` or exact server mismatch before runtime | store read, source read, provider call, or listener effect |
| Foreground composition lacks teardown | `ComputeState::resource_retirer` error | raw coordinator access or fabricated facade |
| Repeated facade resolution | new handle over the same retained `Arc` authorities | second store, coordinator, runtime, manager, registry, or journal |
| Guest response is ambiguous | retained existing inspect-before-retry behavior | blind repeated effect |

## Fail-before evidence

Before product edits:

- `cargo test -p nimbus-server workload_composition --lib -- --nocapture`
  passes `10/10`.
- `cargo test -p nimbus-cli compose_local_and_forwarded_restart_use_compute
  --lib -- --nocapture` passes `1/1`.
- `cargo test -p nimbus-cli
  forwarded_server_profile_defers_machine_activation_until_after_engine_construction
  --lib -- --nocapture` passes `1/1`.
- live NNCV035 is exact `0/7`. The sole f1-owned red diagnostic is
  `teardown-contract/forwarded-machine-registry`.
- the canonical `forwarded/profile.rs`, both required direct consumer
  functions, and `ServerForegroundWorkloadRuntime::resource_retirer` do not
  exist. The focused compile/source tests added by this item must fail before
  implementation and pass only through the canonical seam.

## Behavioral proof matrix

| Proof | Owner | Required assertion |
| --- | --- | --- |
| Canonical consumer structure | CLI composition test plus NNCV035 | two direct returned canonical calls; one registry constructor across the three files |
| Exact foreground facade | server composition test | exact realm resolves without store/provider/filesystem effects |
| Missing teardown facade | server composition test | precise error and zero effects |
| Crossed teardown realm | server/compute composition tests | rejection before runtime/store/source/provider work |
| Real forwarded execute substitution | existing forwarded adapter harness | five ordered exact phase dispatches and no fallback |
| Real forwarded inspect substitution | existing forwarded adapter harness | inspect-before-retry and exact command fences |
| Real local composition | existing local Krun composition test | one exact selectable local bundle; effect-free completion |
| No duplicate journal | source census plus backend tests | one teardown adapter construction per backend and one provider-command namespace open |

## Frozen changed-path roster

Product and focused test paths:

- `crates/nimbus-cli/src/compose/mod.rs`
- `crates/nimbus-cli/src/network_composition.rs`
- `crates/nimbus-cli/src/network_composition/forwarded.rs`
- `crates/nimbus-cli/src/network_composition/forwarded/profile.rs` (new)
- `crates/nimbus-cli/src/compose/provision.rs`
- `crates/nimbus-cli/src/compose/tests/lifecycle.rs`
- `crates/nimbus-cli/src/machine/backend.rs`
- `crates/nimbus-cli/src/machine/backend/teardown.rs`
- `crates/nimbus-server/src/workload_composition.rs`
- `crates/nimbus-server/src/workload_composition/tests.rs`
- `crates/nimbus-node/src/systemd_transient.rs`
- `crates/nimbus-sandbox/src/backends/oci/command.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/egress_pin.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/netns.rs`

The five additions outside the original nine-path seam are direct K15 compile
prerequisites found by the real Linux build. They delete one uncalled
Linux-test helper and gate one test-only systemd constructor. They also qualify
one netns file creation and route finite nft stdin through the existing bounded
command owner. They do not change provider ownership, policy, or network
effects.

Control-plane evidence paths:

- this proof.
- `docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json`
  (source-derived coordinates only).
- `docs/private/plans/nimbus-network-control-plane-plan.md`.
- `docs/private/plans/README.md`.

The NNCV035 scanner already names the canonical module, consumer functions,
single-constructor rule, and real forwarded execute/inspect tests. Change it
only if implementation exposes a proven semantic blind spot. Do not update it
only to make the candidate pass.

## Complexity disposition

- `network_composition.rs` is in the 1,500-1,999 line exception band because
  it owns the local composition root and inline concept tests. NNC6.5f1 makes
  only type routing changes there and moves forwarded composition out.
- NNC6.5f1 removes duplicate assembly and makes `forwarded.rs` and
  `compose/provision.rs` smaller.
- the new `forwarded/profile.rs` owns one concept and should remain small.
- server composition and its tests remain below the 1,500-line threshold.
- compute state and retirement code do not change.
- NNC6.5f1 permits no compatibility shim, god provider, generic helper, or
  speculative trait.

## Verification and review gates

Focused implementation gates:

- fail-before tests for canonical callers and the foreground facade.
- full `nimbus-server` workload-composition tests.
- focused canonical CLI composition, real local composition, real forwarded
  execute/inspect substitution, missing-authority, and source-activation tests.
- full `nimbus-server`, `nimbus-cli`, and directly affected dependency suites
  as required by the item risk.
- live and helper NNCV035, then the aggregate verifier.
- `cargo fmt --all --check`, affected Clippy, dependency/effect scans, strict
  proof lint, docs, and site verification.

Only after all K1-K18 gates are green and the diff is candidate-frozen, run
one full GPT-5.6 Sol/xhigh/fast item review. Run one narrow correction review
only if an accepted finding materially changes executable code.

## Acceptance ledger

| Criterion | Status | Evidence |
| --- | --- | --- |
| K1-K14 and K16 architecture and behavior | `pass` | The frozen seam is implemented in the exact path roster. Server composition passes `13/13`; canonical CLI source, real forwarded execute/inspect substitution, missing-authority, and delayed-activation gates pass `1/1`, `2/2`, `1/1`, and `1/1`. Full CLI passes `996` with `3` ignored after the accepted review correction. Full server passes serially with `722` and `33` ignored; a prior parallel run produced unrelated contention failures, and an isolated representative failure passed `1/1`. |
| K15 local regression | `pass` | The full macOS CLI suite passes. The real local Krun composition test is `cfg(target_os = "linux")`. A byte-matched candidate uses Rust `1.96.1` on `nimbus@192.168.4.29`. Its first executable run proved that proof-only manager, backend, service-manager, and prepared-composition handles remained live at the reopen assertion. The corrected test scopes its registry borrow and explicitly releases every retained owner. The exact rerun passes `1/1`, with `1004` filtered. It proves that live composition blocks a duplicate claim and final release permits reopen without durable mutation. |
| K17 static contract | `pass` | Live NNCV035 is exact `0/6`, so only the f1-owned forwarded-registry diagnostic changed to green. Its helper passes `138/138`. NNCV008, NNCV015, NNCV027, and NNCV034 pass after their exact f1 corrections. The live aggregate is exact `35/36` with only NNCV035 red. The complete aggregate mutation gate passes `552/552`. |
| K18 quality | `pass` | Format and diff pass. Full CLI passes `996 + 3 ignored`; full server passes. Full sandbox passes `1,155 + 32 ignored`, plus `11` integration/binary passes and `16` environment-gated ignores; node passes `121/121`. Strict all-target/all-feature CLI/server/sandbox/node Clippy passes. The Linux-only K15 gate passes `1/1`, with `1004` filtered. NNCV035 helper is `138/138`, and the complete aggregate mutation gate is `552/552`. The live aggregate is exact `35/36` with only NNCV035 red. Strict proof lint, docs `108`, and site `17/17` pass. |
| K19 item review | `pass` | The one full GPT-5.6 Sol/xhigh/fast review accepted one P2 error-classification regression. The correction preserves operational `nimbus::Error` variants and maps only composition validation to `InvalidInput`. Its focused test passes `1/1`; full CLI passes `996 + 3 ignored`; strict CLI Clippy passes. The one narrow Sol/xhigh/fast review is clean at confidence `0.99`. Review cadence is exhausted. |
| K20 durable closeout | `pass` | This proof, the canonical plan, routing index, source-derived census, and exact 14-path product/test diff close in the item commit that contains this row. NNC6.5f2 becomes the sole active item and must reconcile current main before source edits. |

## Item review disposition

The one full item review used GPT-5.6 Sol with xhigh reasoning and fast mode.
It reviewed staged tree `370234323d32c1db4b55aa4bc2b69a911d546161`,
patch SHA-256
`327efde2bddb495f6a06f1e76137435deec3a65d65579f51053dd3d8a32fb4b7`,
and thread `019ff308-5e98-7011-b26d-8730dbcedb92`.

The review reported one P2 at confidence `0.97`. The canonical foreground
consumer converted every `LocalNetworkCompositionError` to `InvalidInput`,
although the replaced path preserved operational activation and backend
errors. We accept the finding. `forwarded_compose_activation_error` now
returns the inner `LocalNetworkCompositionError::Compose` error unchanged. It
converts other composition-validation errors to `InvalidInput`. The focused
regression test passes `1/1`, full CLI passes `996 + 3 ignored`, and strict CLI
Clippy passes. This executable correction authorizes one narrow review.

The one narrow correction review used GPT-5.6 Sol with xhigh reasoning and
fast mode. It reviewed staged tree
`1360ee6a5cb64a85293dbf90037726f1390dca03`, patch SHA-256
`8bb3860fb5d758b34d9a72fee40f30fcdf4d8027cc5996965db50c0e6919b241`,
and thread `019ff314-31d9-7460-b19f-c8a583dcfebc`. It reported no findings
and rated the correction correct at confidence `0.99`. The cadence permits no
further NNC6.5f1 review.

## Recovery state

- owner worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`.
- owner branch: `codex/nimbus-network-architecture-audit`.
- dependency checkpoint: NNC6.5f commit
  `53e0b36ce94d6676875d51eb720a37373050311c`.
- completed item: NNC6.5f1. The commit containing this proof is its durable
  checkpoint.
- next item: NNC6.5f2. Reconcile current main from a clean checkpoint before
  source edits.
- completed phase: K1-K20 pass. The correction resolves and proves the full
  review's one P2, and the narrow review is clean.
- dirty-state owner: NNC6.5f1 owns fourteen product/test paths. It also
  owns the source-derived census coordinates, proof, plan, and routing index.
  No manifest, dependency, `nimbus-network`, Compose-down, coarse-stop,
  physical-stop, compensation, tenant-retirement, or provider-effect path
  changed.
- product source: one canonical 110-line forwarded profile and two thin
  consumers. The item also changes type routing and retained teardown
  accessors. It adds the delegated server retirement facade and focused tests.
- last green behavior: server workload composition `13/13`. Full CLI is `996`
  plus `3` ignored. Full server serial is `722` plus `33` ignored. Canonical
  source is `1/1`. Real forwarded execute and inspect are `2/2`.
- last green failures: missing authority `1/1` and delayed activation `1/1`.
- last green static gates: live NNCV035 `0/6`, helper `138/138`, aggregate
  mutations `552/552`, NNCV015, and the exact live aggregate `35/36` with only
  NNCV035 red.
- last green quality gates: format, full sandbox, full node, strict
  CLI/server/sandbox/node Clippy, strict proof lint, docs `108`, and site
  `17/17` pass.
- last failed gate: the aggregate mutation run found NNCV008 missing recovery
  tokens and the NNCV027 Compose handoff source mismatch. The run stopped once
  those failures were decisive. The focused corrections now pass. No product
  effect ran.
- host evidence: the K15 real local Krun composition test is Linux-only and is
  absent from the macOS test binary. On the byte-matched minicloud candidate,
  `cargo +1.96.1 test --locked -p nimbus-cli
  real_local_krun_and_server_reports_freeze_one_exact_selectable_bundle --lib
  -- --nocapture` passes `1/1`, with `1004` filtered.
- item review: one full Sol/xhigh/fast review accepted one P2. The correction
  passes focused `1/1`, full CLI `996 + 3 ignored`, and strict CLI Clippy. The
  narrow review reports no findings at confidence `0.99`. The item exhausted
  its review cadence.
- blocker: none.
