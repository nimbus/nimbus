# NNC6.1 Compute Network Manager Injection

Status: `complete`

## Outcome owned by this item

NNC6.1 carries the one already-frozen `Arc<LocalNetworkManager>` from the
outer CLI composition through server construction into `ComputeState`. The
manager remains the only identity that pairs the node-local store, attachment
authority, port authority, and immutable capability registry. Compute does not
reopen a root, copy a registry, or construct provider effects. Compute does not
own policy, service names, proxies, projections, workload sagas, or cluster
authority.

This item only carries identity. NNC6.1a owns the sole cross-domain saga
coordinator. NNC6.1b-e own workload vocabulary, durable saga state,
service-store replacement, and recovery. NNC6.2-NNC6.6 own plan compilation
and lifecycle choreography.

## Read-only constructor and caller census

The audit ran at exact parent
`994fe584a4a40fabfec008b2abfa984f4c755cc4`.

| Seam | Current state | NNC6.1 target |
| --- | --- | --- |
| `nimbus-compute/Cargo.toml` | No `nimbus-network` dependency. | Add the one upper-to-lower workspace edge. Keep `nimbus-network -> nimbus-core` unchanged. |
| `ComputeStateConfig` / `ComputeState` | Carry engine, deployment, control-plane, node-service, and runtime state only. | Carry an optional manager solely to represent two explicit construction profiles: managed workload-capable and protocol-only. Expose the retained Arc without copying its registry. |
| `PreparedLocalNetworkComposition` | Retains `FrozenLocalNetworkComposition`, whose manager accessor is test-only. Production exposes only `LocalNetworkAuthority`. | Expose a production Arc clone from both frozen and prepared composition. |
| CLI start | Calls `ServeOptions::new(engine, prepared_network.authority())`. | Pass `prepared_network.manager()`. The manager is the already-frozen source of the authority and registry. |
| `ServeOptions::new` | Accepts `LocalNetworkAuthority`. Compute never sees the manager. | Accept `Arc<LocalNetworkManager>`, derive listener authority from it, and put that same Arc into `RouterOptions`. |
| `RouterOptions` / `RouterBuildConfig` | Have no network-manager field. | Distinguish `new(engine, manager)` from explicitly named `protocol_only(engine)` and preserve the exact Arc through build. |
| `AppStateConfig` | Has no network-manager field. | Carry the same optional Arc into `ComputeStateConfig`. |
| Direct reconstruction | Reconstructs primitive listener authority for protocol/embedder tests and small local control servers. | Remain explicitly protocol-only. It does not construct a manager and cannot admit service or machine workload managers. |
| Manager construction census | Outer CLI/network composition and explicit tests own `LocalNetworkManager::{bootstrap,open}`. Compute and production server state construct none. | Preserve that ownership. Static proof rejects a new compute/server manager constructor. |

Only `crates/nimbus-compute/src/state.rs` and
`crates/nimbus-server/src/state.rs` construct `ComputeStateConfig`.
`crates/nimbus-server/src/router.rs` and its focused test fixture construct
`AppStateConfig`. Production `ServeOptions::new` callers are CLI start/dev and
the manager-composition integration tests. Direct `RouterOptions::new` callers
are server protocol tests plus one ignored krun workload test. Protocol-only
tests become explicit. The workload test must supply the composition's
manager.

## Frozen architecture decisions

1. The injected value is the concrete `Arc<LocalNetworkManager>`. There is one
   real implementation, so NNC6.1 does not invent a god `NetworkProvider` or a
   speculative substitution trait.
2. `LocalNetworkManager` retains the immutable `NetworkCapabilityRegistry`.
   Compute stores the manager Arc, not a cloned registry or independently
   opened store/authority.
3. `RouterOptions::new` is the managed constructor and requires the Arc.
   `RouterOptions::protocol_only` is the explicit negative profile used by
   direct protocol/embedder fixtures. The negative profile cannot install a
   `ServiceManager` or `MachineLifecycleManager`.
4. `ServeOptions::new` is manager-derived. It uses `manager.authority()` for
   server listener leases and injects the same manager into router/compute
   state. It cannot accept a separately selected authority.
5. `ServeOptions::reconstruct_direct{,_at}` remains the explicitly named
   protocol-only reconstruction admitted by NNC4.6d/f. It reconstructs only
   listener authority, injects no manager, and cannot become a workload-capable
   path through builder order.
6. CLI start/dev owns the production local-node composition. It freezes
   capabilities once and passes that manager to server/compute. It retains the
   manager, sandbox process, provider owners, and listeners for process life.
7. Compute may read portable manager handles and capability facts. Socket,
   Netavark, nft, namespace, gvproxy, PEP, certificate, proxy, and provider
   effects remain in their existing owners.
8. No workload saga, network plan compilation, restart command, lazy
   activation, desired-state persistence, or lifecycle ordering enters this
   item.

## Exact owned paths

The acceptance-frozen item may edit only these paths before a recorded scope
amendment:

```text
Cargo.lock
crates/nimbus-cli/src/dev/wire.rs
crates/nimbus-cli/src/machine/local_server.rs
crates/nimbus-cli/src/network_composition.rs
crates/nimbus-cli/src/start/boot.rs
crates/nimbus-cli/src/start/tests/krun.rs
crates/nimbus-compute/Cargo.toml
crates/nimbus-compute/src/state.rs
crates/nimbus-server/src/adapters/http_mount.rs
crates/nimbus-server/src/adapters/cloudflare/kv/mod.rs
crates/nimbus-server/src/construction.rs
crates/nimbus-server/src/router.rs
crates/nimbus-server/src/state.rs
crates/nimbus-server/src/tests.rs
crates/nimbus-server/src/tests/machine_lifecycle.rs
crates/nimbus-server/tests/network_manager_composition.rs
crates/nimbus-server/tests/reactive_loop.rs
docs/private/plans/README.md
docs/private/plans/nimbus-network-control-plane-plan.md
docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json
docs/private/plans/proof/nimbus-network-control-plane/nnc4.6f-production-network-authority-census.json
docs/private/plans/proof/nimbus-network-control-plane/nnc6.1-compute-network-manager-injection.md
scripts/nimbus-network-control-plane/compute-network-manager-injection-contract.sh
scripts/verify-nimbus-network-control-plane.sh
scripts/verify-nimbus-network-source-contract.mjs
```

`Cargo.lock` records the new direct dependency in the existing
`nimbus-compute` package entry. No package version or checksum changes. If
compilation requires another constructor or focused test path, amend this list
and the recovery ledger before editing that path.

## Acceptance criteria

| ID | Verifiable criterion |
| --- | --- |
| R1 | Cargo metadata proves `nimbus-compute -> nimbus-network`; `nimbus-network` still has exactly one workspace dependency, `nimbus-core`, in normal/dev/all-feature profiles, and the graph is acyclic. |
| R2 | `ComputeStateConfig` and `ComputeState` retain the injected manager; `ComputeState::network_manager()` returns an Arc pointer-identical to the input and exposes the exact immutable registry. Protocol-only state returns `None`. |
| R3 | `FrozenLocalNetworkComposition` and `PreparedLocalNetworkComposition` return the exact frozen manager Arc in production code. Repeated access is pointer-identical and does not mutate durable bytes or provider state. |
| R4 | `ServeOptions::new` accepts the manager, derives its listener authority, and preserves the exact Arc into router/compute state. No caller can pair manager A's compute registry with authority B's listeners. |
| R5 | Managed `RouterOptions` preserves one pointer through `RouterBuildConfig -> AppStateConfig -> ComputeStateConfig -> ComputeState`. No adapter constructs, opens, bootstraps, or freezes a second manager. |
| R6 | CLI start/dev and the ignored real-krun workload path pass the already-frozen manager. Service, sandbox, and machine workload-capable production builders cannot begin from `protocol_only` or direct reconstruction. |
| R7 | Explicit protocol-only/direct paths remain usable for transport-only tests and local control servers, retain their existing primitive listener authority, report no compute manager, and fail before installing a service or machine workload manager. |
| R8 | Static NNCV025 rejects a missing compute edge/field/accessor, copied registry, authority-only start/serve handoff, manager-less managed constructor, workload-enabled protocol-only builder, or any compute/server `LocalNetworkManager::{open,bootstrap}` call. |
| R9 | Focused Rust tests cover managed happy path, protocol-only edge path, pointer/registry identity, repeated access, and both service/machine negative builder paths. Existing listener, service, sandbox, machine, router, and CLI composition tests remain green. |
| R10 | Affected all-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, script syntax/ShellCheck, live verifier/self-test, docs/site gates, one candidate-frozen GPT-5.6 Sol/xhigh/fast review, exact ledger closeout, and one item commit pass. |

## Fail-before packet

Before product edits, the structural contract must fail only at NNCV025.
Compute has no manager edge or fields. CLI exposes only an authority. Server
constructors accept authority-only composition.

| Packet | Fail-before observation | Corrected proof |
| --- | --- | --- |
| F1 compute seam absent | Cargo and source checks report the missing dependency, config/state field, and accessor. | R1-R2. |
| F2 manager identity discarded | Production prepared composition has no manager accessor and start passes only authority. | R3 and R6. |
| F3 server split seam | `ServeOptions::new` accepts authority and router/state carry no manager. | R4-R5. |
| F4 negative profile unsealed | Direct/protocol-only construction can install service or machine workload managers. | R6-R7. |
| F5 future regression mutations | Each named mutation must fail exclusively as NNCV025 after correction. | R8 and aggregate self-test. |

Accept compile-fail evidence only when the missing NNC6.1 field or constructor
is the direct cause. Unrelated imports, private-path mistakes, or fixture
failures do not count.

## Verification ledger

| Checkpoint | Status | Evidence |
| --- | --- | --- |
| Read-only audit | `done` | Constructor/caller/dependency census above; zero product path changed. |
| Acceptance freeze | `done` | R1-R10, F1-F5, exact owned paths, decisions, and non-goals are recorded before product edits. |
| Fail-before | `done` | Before product edits the live aggregate verifier reported 25 passes and NNCV025 as the sole failure (`Summary: 25 passed, 1 failed`). Its eight diagnostics named the missing compute dependency/fields/accessor, frozen/prepared manager accessor, CLI start handoff, serve composition, router profiles/fences, and app-state handoff. |
| Implementation | `done` | The shared Arc is carried through compute, CLI composition, serve/router build state, and explicit managed/protocol-only construction. Focused behavior is `10/10`: compute identity `1`, router identity/fences `2`, server listener composition `4`, and CLI composition/dev/machine entrypoints `3`. |
| Candidate convergence | `done` | Compute `72/72`; CLI `937/937` with one declared ignore; server `509` passed with `26` declared ignores and only the two inherited parent-lineage trust-boundary failures below; focused listener integration `4/4`; live verifier `26/26`; corrected mutations `173/173`; metadata, check, Clippy, rustdoc, format, diff, script, docs `108`, and site `17/17` gates pass. |
| Candidate review | `done` | The sole full GPT-5.6 Sol/xhigh/fast review reported three findings at confidence `0.96`. Two P2 findings are accepted. One P1 finding is rejected with source and live-verifier evidence below. |
| Review corrections | `done` | `ComputeState::from_config` fences both direct workload-manager profiles without the shared manager. `RouterBuildConfig::into_state` is the production handoff used by both `build` and the identity test. Each correction has an exclusive NNCV025 fail-before mutation; the final aggregate passes `173/173`, including all 15 NNCV025 mutations. |
| Narrow correction review | `done` | One GPT-5.6 Sol/xhigh/fast pass reviewed only the two accepted corrections. It reported no accepted or actionable findings and judged the patch correct at confidence `0.98`. No further NNC6.1 review is warranted. |
| Exact checkpoint | `done` | The narrow-reviewed staged tree is `5005eb5e29498c2c34667d1a5cdc74f3b3c20ff2`. Its executable/script SHA-256 is `c731906ea89fcc16ad764ec42738ab89630e8289aa956dc4ff7d6d5b53ea2c15`; its full staged-patch SHA-256 is `cca4c767b781624a390783f744d801a6a406eec986cd9134c9b2289f0d52a349`. The final ledger-only edits do not change executable code and require no review rerun. |

## Full review disposition

| Finding | Disposition | Evidence and action |
| --- | --- | --- |
| P1 test fixtures pollute the parallel-manager census | `rejected` | `walkRust` applies `withoutCfgTestItems` before every NNCV025 scan. The candidate live verifier passed `26/26` with both inline fixture constructors present. The `parallel-compute-manager` and `parallel-server-manager` mutations also failed exclusively at NNCV025. |
| P2 direct compute construction can pair workload managers with `None` | `accepted` | `ComputeState::from_config` now checks the profile before it resolves node services. Direct service and machine state-construction tests must fail before either effect-forbidden stub can run. The `missing-compute-profile-fence` mutation owns the fail-before regression proof. |
| P2 Arc identity test bypasses the production router-state handoff | `accepted` | `RouterBuildConfig::into_state` now owns the production handoff used by `build` and the identity test. The `missing-router-build-handoff` mutation owns the fail-before regression proof. |

## Narrow correction review

The one permitted correction review used GPT-5.6 Sol with xhigh reasoning and
fast service. It ran one pass over the two accepted defects. The review
confirmed four facts:

- `ComputeState::from_config` validates both workload-capable profiles before
  node-service resolution.
- `RouterBuildConfig::build` delegates through the same `into_state` handoff
  used by the identity test.
- Both named mutations protect those boundaries.
- The source scanner removes the shared test fixture from the production
  census.

The review reported no finding and judged the patch correct at confidence
`0.98`. The review cadence therefore forbids another NNC6.1 review.

## Candidate acceptance evidence

| Requirement | Evidence |
| --- | --- |
| R1 dependency boundary | Cargo metadata reports `nimbus-compute -> nimbus-network`. It reports `nimbus-core` as the only `nimbus-network` workspace edge. NNCV004 and NNCV007 also pass. |
| R2-R5 identity handoff | Compute identity passes `1/1`. Router identity and protocol-only state pass `2/2`. Server manager-derived listener composition passes `4/4`. Every assertion uses Arc pointer identity or exact registry-reference identity. |
| R3 access and lifetime | The CLI composition test passes `1/1`. Repeated access returns pointer-identical Arcs without materializing durable authority. Dropping the wrapper while clones exist retains the process claim. Dropping all clones releases it. |
| R6-R7 entrypoint profiles | CLI dev and machine entrypoint tests pass `2/2`. The real-krun smoke path compiles under all targets and remains deliberately ignored on hosts without its KVM fixture. Both public workload-manager builders reject protocol-only construction before their effect-forbidden stubs can run. |
| R8 structural protection | Live verifier NNCV025 passes inside `26/26`. Its 15 mutations each fail exclusively at NNCV025, including `missing-compute-profile-fence` and `missing-router-build-handoff`. The complete aggregate reports `173 passed, 0 failed`. |
| R9 affected behavior | Full compute reports `72` passed. Full CLI reports `937` passed and one declared ignore. Full server reports `509` passed and `26` declared ignores before the two inherited failures below. The separately run listener composition integration suite reports `4/4`. |
| R10 quality and review | The three affected crates pass all-target/all-feature check, strict Clippy with `-D warnings`, and no-dependency all-feature rustdoc with `RUSTDOCFLAGS=-D warnings`. Cargo format, diff check, Node syntax, Prettier, Bash syntax, and ShellCheck pass. Docs report `108` link-clean pages. The site reports `17/17`. The sole full review and one permitted narrow correction review are dispositioned. |

The full server suite reproduces the same two unrelated trust-boundary
failures documented at the clean NNC5.6 parent lineage:

- `deploy_admin_requires_local_admin_header_even_with_deploy_bearer` returns
  `400` instead of `200`.
- `cloud_functions_passes_runtime_owner_lifecycle_conformance` returns `409`
  instead of `200` in its isolated subprocess.

NNC6.1 changes manager composition and does not change either trust-boundary
implementation or test. The focused router and listener suites pass `6/6`.
The candidate does not count the two inherited failures as NNC6.1 acceptance.

## Candidate diff census

The candidate uses 24 of the 25 acceptance-frozen paths. It does not use
`crates/nimbus-server/src/tests/machine_lifecycle.rs`. The lockfile changes one
dependency row. The two authority inventories change source-derived line and
symbol fields only. No provider effect, policy, service-name, forwarding,
certificate, system projection, cluster transport, or workload-saga owner
moves into `nimbus-network` or compute.

The source verifier is 1,598 lines and is an explicit deep structural-scanner
exception in the repository's 1,500-1,999-line review band. It keeps one
coherent ownership story: shared Rust test-item masking, source-graph walking,
and mutually exclusive contract modes. Splitting the NNCV025 mode would either
duplicate that masking and graph machinery or create a second source-contract
authority. The new mutation driver remains a concept-owned child script.
`crates/nimbus-server/src/router.rs` is 1,175 lines and remains below the
1,500-line threshold.
