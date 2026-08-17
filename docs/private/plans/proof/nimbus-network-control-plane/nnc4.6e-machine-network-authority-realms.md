# NNC4.6e Machine Network-Authority Realms

Status: `complete; Bands A-D and E1-E17 green; the sole full item review and
one permitted narrow correction review are dispositioned; all eleven accepted
findings are corrected and reproved`

Owner: NNC4.6e in
`docs/private/plans/nimbus-network-control-plane-plan.md`.

Protocol supersession note: NNC5.3a verified the official gvproxy client
contract and corrected an inherited modeling error in this proof. Native
expose/unexpose responses are status-only and native current observation is
one `GET /services/forwarder/all`; gvproxy does not echo Nimbus provider
identity/generation and has no `/inspect`. The sandbox adapter scopes native
routes under lifecycle-issued authority and mints Nimbus evidence only after
exact post-effect observation. See
`nnc5.3a-machine-forwarded-readiness.md`.

This proof is the implementation and closeout contract for host-machine and
guest-machine network composition. It closes NNCF24 and NNCF25. It does not
change the product ownership model:

- `nimbus-network` owns portable identity, durable leases, generations,
  fencing, and reconciliation transitions;
- `nimbus-machine` owns pure machine-provider facts and serialized machine
  intent/evidence;
- `nimbus-cli` owns host and guest composition, Machine API choreography, and
  gvproxy lifecycle translation;
- `nimbus-sandbox` owns guest Netavark, namespace, wildcard proxy, and gvproxy
  forwarding effects; and
- `nimbus-services` and later `nimbus-compute` remain the service/workload
  lifecycle owners.

No socket, gvproxy, Netavark, Machine API, Axum, tenant policy, service naming,
or provider effect enters `nimbus-network`.

## Recovery Ledger

| Field | Current value |
| --- | --- |
| Canonical item | `NNC4.6e` |
| Dependency checkpoint | NNC4.6g commit `8468ddce3b5afc001356ea2a3ee099d29781957c`, tree `0964dc7343bddb745dd7b356233b99c582a524c1` |
| Current phase | Acceptance complete. The sole full item review and one permitted narrow correction review are dispositioned; all eleven accepted findings and every E1-E17 gate are green. |
| Last completed action | Reproved the final candidate: `nimbus-network` 221/0/0, `nimbus-machine` 27/0/0, `nimbus-sandbox` 736/0/24, `nimbus-cli` 932/0/1, `nimbus-server` 601/0/28 under the recorded inherited exclusions, and `nimbus-assets` 9/0/0. Affected all-target/all-feature check, workspace strict Clippy, warning-denied rustdoc, format/diff, six `machine-os` lanes, source-derived 62-authority/36-risk/five-local-IPC census, live verifier 16/16, and self-test 51/51 pass. |
| Next action | Commit this exact Nimbus item checkpoint, then start NNC4.6f with its read-only constructor/root/primitive-handle substitution audit. |
| Executable dirty paths | The exact NNC4.6e Nimbus source/test/template set is frozen for the containing item commit. Cross-repo recipe/docs are clean at companion commit `f0cf9eca2878eb07bd24eec6562fcc58b40f0b5e` in `/Users/jack/src/github.com/nimbus/machine-os-network-authority-audit`. The original source checkouts remain untouched. |
| Review state | Full item review threads `019faf82-4d02-7ee2-8de0-b87403298857` and `019faf88-5d10-7431-aedc-7a325879d7bc` ran once through the structured helper using GPT-5.6 Sol/xhigh/fast; all nine findings were accepted, corrected, and reverified. The one permitted narrow correction review ran through `019fafcb-aa4e-7840-9b96-b3cf086dc4bb` and `019fafcf-65a6-7242-8f3c-e6e9a12ca084`; its two P1 findings were accepted, corrected, and reverified. No further NNC4.6e review is warranted. |
| Blocker | None |

Update this ledger after every extraction band and before any likely context
loss. The canonical plan recovery header remains the authoritative pointer.

## Reviewed and Final Candidate Identity

| Field | Value |
| --- | --- |
| Scope baseline | NNC4.6e only: separate parent-host and guest-node machine network-authority realms while preserving the transport-free `nimbus-network -> nimbus-core` seam and current provider-effect owners. |
| Initially reviewed identity | Nimbus tree `b40fdc96f6aa304102b763d35b4b2a731c8c828d`, executable SHA-256 `2d591155c62da488b22a144d0b70516313f8af1c85ab55d129e20fa6803acf16`; `machine-os` tree `4ac1660e90e6c7ce77c3333b4fd37d55dcbec52f`, executable SHA-256 `70072693094567b2b9df2c5d76497ec1063b3be389150954dc114e8f194b13f4`; domain-separated combined SHA-256 `97cfb2c16d509c4986ded83f36653dea0bb237b59aaa6f539e7349a7ee6caf8d`. |
| Narrow-reviewed correction identity | Nimbus: 98 staged item paths, pre-review-ledger tree `e0be42a385b87d3ec7f44f020cd1a827cd366d6a`, `crates/` executable SHA-256 `849d79ad9f9be6c71a2a840d15adb520da2374c360131e607a4ff184437d3d91`. `machine-os`: eight staged paths, tree `b32015ba67b7802784ada0b308b4c28fbdbaedad`, recipe/verifier executable SHA-256 `9e391aa6985c343f43d9c22029747d7fcca2582f2ca04c644d5eb330a7210533`. Domain-separated combined SHA-256 `f2ce2650b7e31b696b981ba5dbdeddb937afed29adc2dbbdd8742ee3994b310f`. This is the exact input reviewed by the one permitted narrow correction review. |
| Final post-review-correction identity | Nimbus `crates/` executable SHA-256 `44099ca802550b3587b934b835a6372b901ccdaa3990e8b4b4c76a13edde7a47`. `machine-os` recipe/verifier executable SHA-256 `9e391aa6985c343f43d9c22029747d7fcca2582f2ca04c644d5eb330a7210533`, commit `f0cf9eca2878eb07bd24eec6562fcc58b40f0b5e`, tree `b32015ba67b7802784ada0b308b4c28fbdbaedad`. Domain-separated combined SHA-256 `748f7013671251cbb4a5b8878b8bdc12d6ff47a83fe4d1fcbf4c7706b9800c57`, computed over `nimbus-executable-sha256\0<hex>\0machine-os-executable-sha256\0<hex>\0`. |
| Candidate integrity | The Nimbus item has zero unstaged source paths and passes cached/working-tree diff checks; the companion worktree is clean at its exact commit. The final Nimbus executable diff spans 97 crate paths. Documentation/ledger truth-up does not alter either executable digest. |

## Source-Proven Current Ownership

```text
parent host process
  MachineRootLayout::resolve()
    -> config/state/data/cache/runtime artifact roots
    -> raw network_state_root (implicit, independently resolved)
  machine/manager/ports.rs
    -> LocalPortLeaseAuthority::open(raw root)
    -> SSH gvproxy port lease
  ForwardedMachineApiSandboxBackend
    -> Machine API start/stop first
    -> no parent-host publication lease

guest machine process
  run_machine_api_command
    -> create control directory
    -> bind/adopt Unix listener
    -> ContainerSandboxBackend::new(plan_only config)
       network_state_root == workload_state_root
    -> guest-derived boot-id gvproxy handle/generation
  OCI machine-port lifecycle
    -> guest Host-realm wildcard proxy lease + bind
    -> /expose asks parent gvproxy to publish the same numeric port
```

Source anchors:

| Current owner/site | Proven behavior or gap |
| --- | --- |
| `crates/nimbus-machine/src/roots.rs` | `MachineRootLayout` mixes artifact roots with a raw `network_state_root`; `resolve`, `new`, and `with_network_state_root` can independently select authority. |
| `crates/nimbus-machine/src/image_source.rs` | `MachineConfigRecord` persists the mixed root layout but no authenticated manager provenance. |
| `crates/nimbus-cli/src/machine/handlers.rs` | Direct commands, default-client helpers, and fallback lifecycle paths repeatedly resolve roots. |
| `crates/nimbus-cli/src/machine/server_control.rs` | Embedded lifecycle resolves machine roots and overwrites the raw network path instead of retaining the start manager. |
| `crates/nimbus-cli/src/machine/manager/ports.rs` | Prepare, withdraw, and delete reopen `LocalPortLeaseAuthority` from raw roots. |
| `crates/nimbus-cli/src/machine/files.rs` | Loading refreshes and writes state before authenticating the persisted artifact or network authority. |
| `crates/nimbus-cli/src/machine/handlers.rs` delete path | Artifacts are loaded through caller roots and the SSH lease is released through those same independently resolved roots. |
| `crates/nimbus-cli/src/machine/api.rs` | Control-directory and listener effects precede a guest manager; the container network and workload roots are identical; provider identity is minted from guest node/boot ID. |
| `crates/nimbus-sandbox/src/backends/container/runtime/network_composition.rs` | `with_network_process` already provides the correct injected guest composition seam; `new` is an explicit direct reconstruction seam. |
| `crates/nimbus-sandbox/src/backends/oci/network/proxy.rs` | The guest wildcard proxy correctly owns a `Host`-realm lease relative to the guest OS node. |
| `crates/nimbus-sandbox/src/backends/oci/network/forwarding.rs` | Exact `/expose` and `/unexpose` receipts already authenticate provider handle, generation, local/remote publication, protocol, and typed outcome, but the parent does not consume them as its own lease evidence. |
| `crates/nimbus-cli/src/machine/backend.rs` | A forwarded start sends Machine API I/O before any parent publication reservation; stop has no parent withdrawal/release lifecycle. |
| `crates/nimbus-machine/src/networking.rs` and `provider.rs` | krunkit/vfkit are Nimbus-host-managed; WSL2 is provider-managed and deliberately unavailable. Those facts are already correctly separate from the general capability registry. |

## Target Ownership and Dependency Shape

```text
one parent OS-node manager
  LocalNetworkAuthority
    +-- managed-machine SSH lease
    +-- per-workload gvproxy publication leases
    +-- retained by direct CLI or injected by start/server
    |
    +-- persisted machine provenance
          authenticates authority path + provider instance
          before state write, provider I/O, or artifact deletion

parent-issued machine provider incarnation
  NetworkProviderHandle + NetworkResourceGeneration
    +-- persisted in machine config/runtime evidence
    +-- supplied to guest boot/config
    +-- authenticated by every Machine API mutation
    +-- scopes adapter-translated native route observations and Nimbus receipts

one guest OS-node manager
  guest control root (network authority)
    +-- injected OciNetworkProcess
    +-- guest wildcard proxy leases/binds
  guest control root/service-sandboxes/... (workload artifacts)
    +-- manifests, bundles, runner state, provider artifacts

same numeric port, separate OS-node conflict domains
  guest Host realm  !=  parent Host realm
```

The dependency direction stays:

```text
nimbus-cli
  -> nimbus-machine
  -> nimbus-sandbox
  -> nimbus-network
       -> nimbus-core
```

`nimbus-machine` may serialize portable `nimbus-network` identities and
generations. It does not construct managers or perform effects.

## Binding Decisions

1. `MachineRootLayout` owns artifacts only. It does not select a network
   authority, default one from the state root, or expose a replacement setter.
2. A serialized machine-authority record stores the canonical parent authority
   provenance and the parent-issued gvproxy provider instance. The live
   `LocalNetworkAuthority` authenticates that record before any state mutation,
   machine provider effect, lease transition, or artifact deletion.
3. Direct host commands create one outer parent manager only after live-server
   delegation is ruled out. Embedded lifecycle receives and retains the exact
   already-frozen start authority. Neither path reopens a manager or primitive
   from serialized roots.
4. SSH prepare, withdraw, exact-stop retention, restart, and deletion receive a
   manager-derived port handle. Raw `LocalPortLeaseAuthority::open` is removed
   from production machine lifecycle.
5. The parent issues one stable opaque gvproxy provider instance per machine
   configuration and a monotonic provider generation per launched machine
   incarnation. The guest never derives either from its IP, boot ID, node ID,
   PID, socket path, or port.
6. Provider generation is durable before gvproxy or VMM spawn. Guest
   provisioning receives that exact identity; Machine API start/stop requests
   must match the boot-authenticated identity before guest workload effects.
7. The guest manager is claimed before control-directory creation, listener
   bind/adoption, backend reconciliation, or provider effect. Its network root
   is the guest control root. Workload artifacts remain under a named
   `service-sandboxes` child.
8. The guest container backend is constructed with one injected
   `OciNetworkProcess`; the separate runner's serialized reconstruction remains
   the admitted same-guest-node cross-process seam.
9. The parent allocates the sandbox incarnation ID before Machine API I/O. The
   same tenant-qualified stable ID owns parent publication leases and is
   supplied to the guest, so stop/restart recovery never guesses identity from
   an IP address or performs an unfenced lookup.
10. Parent publication prepare atomically reserves and claims the entire fixed
    port batch under provider-managed lifetimes before Machine API I/O. A
    conflict returns both owner identities and makes zero Machine API or
    gvproxy request.
11. A Machine API start response is activation evidence only when it
    authenticates the exact requested sandbox ID, provider instance,
    generation, protocol, and complete publication set after the guest
    forwarding adapter observed the exact native route list and minted
    generation-scoped `Exposed` receipts. Native gvproxy status alone, a handle
    alone, a partial set, mismatched identity, EOF, timeout, or connection
    refusal is ambiguous and retains the parent fence.
12. Per-workload teardown is parent withdraw, guest stop/unexpose, exact
    `Withdrawn` or `ExactAlreadyAbsent` evidence, then parent release. Any
    missing, partial, stale, untyped, or transport evidence leaves
    `CleanupPending`.
13. Whole-machine stop first withdraws every parent publication for the exact
    machine provider incarnation. The parent persists the spawned gvproxy PID
    plus its operating-system birth token under the exact forwarder authority.
    Stop signals only a matching process incarnation. Exact absence or a birth
    mismatch proves only the old incarnation absent and never signals a
    replacement; missing, corrupt, or crossed identity evidence retains every
    fence.
14. Parent publication desired request, durable lease/provider handle, and
    observed receipt remain distinct. Machine/system projections do not become
    desired state or allocation authority.
15. Machine connectivity capability facts remain source-owned and separate.
    krunkit/vfkit use this host-managed composition; WSL2 remains unavailable
    before manager mutation or provider effect and gains no fabricated
    attachment/ingress capability.
16. No compatibility shim, fallback root, dual writer, global singleton
    lookup, hidden default manager, or path/IP/port-derived workload identity is
    introduced. The repository is pre-launch, so serialized schemas change
    directly.
17. Structured review treats NNC4.6e as one item. It runs once, with GPT-5.6
    Sol/xhigh/fast, only after every frozen criterion and pre-review gate is
    green. A material accepted executable finding permits one narrow correction
    review; docs, formatting, ledger wording, elapsed time, or internal diff
    chunks do not.

## Frozen NNC4.6e Acceptance

| ID | Verifiable success criterion |
| --- | --- |
| E1 | `MachineRootLayout` contains only config/state/data/cache/runtime artifact roots. No constructor, resolver, serialized field, setter, or fallback chooses network authority. Static scans find zero production machine uses of the deleted mixed-root vocabulary. |
| E2 | Every newly initialized config persists one canonical parent authority provenance and opaque provider instance. Same and existing-alias roots authenticate; a divergent or retargeted alias returns typed active/attempted evidence before state write, provider effect, lease mutation, or artifact deletion. |
| E3 | Direct mutation commands retain one outer parent manager; live-server delegation performs no local manager construction. Start injects its existing authority into both the embedded lifecycle manager and forwarded service backend. Final-drop reopen and alias-retarget retention are deterministic. |
| E4 | Managed-machine SSH prepare/start/withdraw/restart/delete use only the injected manager-derived port authority. Deletion with substituted caller roots or provenance cannot release a foreign lease or delete either artifact tree. |
| E5 | Provider instance is parent-issued and stable for one machine config; provider generation is monotonic across launches and durable before gvproxy/VMM spawn. Guest config and every Machine API mutation authenticate the exact pair; guest boot/node/IP/PID/port data cannot mint or replace it. |
| E6 | Guest manager claim precedes control-root creation, Unix-listener bind/adoption, backend reconciliation, and provider effects. One injected `OciNetworkProcess` owns guest segment/IPAM/port state; workload manifests/bundles remain below the separate `service-sandboxes` artifact root. |
| E7 | Parent selects one tenant-qualified sandbox incarnation and reserves/claims its complete fixed publication batch before the first Machine API byte. Same-parent conflicts fail with durable owner evidence and zero API calls; the same numeric guest proxy port can coexist in the separate guest root. |
| E8 | Exact, complete, same-generation `Exposed` evidence activates the parent batch. Partial/stale/crossed/untyped success, timeout, EOF, refusal, or response loss never publishes and leaves a recoverable fenced batch. |
| E9 | Per-workload stop executes withdraw -> guest stop/unexpose -> authenticated exact absence -> release. Live-owner and fresh-process dead-owner paths both converge. Ambiguous absence retains provider handle, generation, binding, and port conflict. |
| E10 | Whole-machine stop withdraws all exact-incarnation parent publications before provider stop. Parent-authenticated process-birth evidence permits signaling only the spawned gvproxy incarnation. Confirmed exact absence or replacement releases the old-incarnation publications and retains SSH under its explicit restart/delete contract; missing/corrupt/crossed evidence or ambiguous stop leaves both authorities fenced. |
| E11 | Restart increments provider generation, rejects stale guest receipts, and cannot reuse a port fenced by an ambiguous prior incarnation. Exact prior absence permits deterministic new-generation publication without changing stable workload identity. |
| E12 | Machine API inspect/read paths do not allocate or mutate parent authority. Exact start/stop DTOs deny unknown fields and carry no policy, service-name authority, socket handle, raw IP identity, or guest-minted provider identity. |
| E13 | krunkit and vfkit pass the host-managed contract suite. WSL2 remains provider-managed and fails with its existing named unavailable error before manager mutation, guest composition, or provider effect. |
| E14 | Source/effect scans show no second manager, raw production primitive reopen, mixed artifact/network root, guest-issued parent handle, unleased parent publication, or unclassified Machine API mutation path in the owned census. |
| E15 | `nimbus-network -> nimbus-core` remains the only initial workspace edge. Sockets, Axum, gvproxy, Netavark, machine types, sandbox effects, policy, service naming, proxy forwarding, cluster transport, and cloud SDKs remain absent. |
| E16 | Happy, edge, error, contention, alias, substitution, crash/restart, stale-generation, partial-batch, and exact/ambiguous lifecycle tests assert concrete phases, identities, order, untouched paths, and zero forbidden effects. No test passes by compilation or non-panic alone. |
| E17 | Focused suites, full affected-crate suites with exact counts/skips, all-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, module-size disposition, dependency/effect scans, live composition verifier and self-test, docs, and site gates pass. Then exactly one full Sol/xhigh/fast item review is dispositioned under binding decision 17. |

## Failure and Reconciliation Matrix

| Failure cut | Required durable/observed state | Retry or rollback obligation |
| --- | --- | --- |
| Parent manager/provenance mismatch | No attempted-root creation; no machine state write; no lease/provider/artifact mutation | Inject the active authority; never overwrite provenance |
| Parent batch conflict | Existing owner remains unchanged; attempted request has no provider effect | Return typed conflict before Machine API I/O |
| Failure after reserve/claim, before API write | Claimed provider-managed lifetimes identify a never-called attempt | Exact no-I/O proof may terminally settle; otherwise retain for recovery |
| API write/read timeout, EOF, refusal, malformed or partial response | Parent batch remains nonterminal and unpublished; provider attempt/generation retained | Inspect exact guest/provider generation before activate, compensate, or retry |
| Exact guest `Exposed` receipt, lost parent activation commit | Provider effect may exist; parent batch remains fenced | Inspect exact guest publication and idempotently adopt/activate |
| Parent activation fails after exact guest effect | Parent fence remains; guest effect is not reported ready | Withdraw/inspect the same generation; never start a replacement |
| Stop begins with live parent lifetime | Parent phase is `Withdrawing` before guest stop request | Exact response releases with live guards |
| Stop begins after parent crash | Dead lifetime recovery is authenticated and phase becomes `CleanupPending` before guest stop request | Exact response releases with recovery guards |
| Guest stop timeout/EOF/refusal/untyped response | Parent remains `Withdrawing` or `CleanupPending` with exact binding and generation | Retry/inspect; never infer absence |
| Guest returns stale/crossed/partial absence | Parent remains fenced | Reject and inspect exact requested publication set |
| Whole-machine gvproxy process receipt missing, corrupt, or crossed | SSH and every parent publication remain fenced | Reconcile the exact forwarder/process incarnation; numeric PID evidence alone cannot signal or release |
| Whole-machine exact gvproxy absence or birth-token mismatch | The exact old gvproxy incarnation is absent; a replacement process remains untouched | Release old-incarnation publications and retain/release SSH per its explicit restart/delete contract, then record stopped |
| Restart sees prior ambiguous publication | Old generation still conflicts | Refuse new generation until exact absence |
| Guest manager/root substitution | No listener, backend, Netavark, proxy, or control artifact in attempted root | Inject the boot-authenticated guest manager |
| WSL2 selected | No host/guest manager mutation and no provider process | Return existing named unavailable error |

## Dependency-Ordered Extraction Bands

### Band A — parent composition and persisted provenance

Owned concepts:

- `crates/nimbus-cli/src/machine/network_composition.rs`;
- machine config/state provenance types in `crates/nimbus-machine`;
- direct/embedded lifecycle injection in `machine/handlers.rs`,
  `machine/server_control.rs`, and `start/boot.rs`; and
- manager-derived SSH lifecycle in `machine/manager/ports.rs`.

Gate: E1-E5 and E13 pass; NNCF25 is behaviorally closed before publication
work begins.

### Band B — guest composition and parent-issued provider incarnation

Owned concepts:

- `crates/nimbus-cli/src/machine/api/network_composition.rs`;
- guest boot/config authority evidence;
- one injected guest `OciNetworkProcess`; and
- exact Machine API provider-authority authentication.

Gate: E5-E6, E12-E13 pass. Guest effects remain owned by sandbox/machine
adapters.

### Band C — parent publication authority

Owned concepts:

- `crates/nimbus-cli/src/machine/publication_authority.rs`;
- exact Machine API start/stop DTO evidence in `nimbus-machine`;
- parent-chosen sandbox incarnation handoff; and
- the forwarded backend's reserve/activate/withdraw/release state machine.

Gate: E7-E9 and E11-E12 pass, including real cross-process/root contention and
ambiguous response tests. NNCF24 is not closed until this band and Band D pass.

### Band D — whole-machine convergence and census

Owned concepts:

- whole-machine publication withdrawal/release ordering;
- restart and deletion reconciliation;
- exact constructor/effect census updates; and
- final proof/ledger/gates.

Gate: E10-E17 pass. Only then may the item be candidate-frozen for its one full
structured review.

## Exact Fail-Before Packet

Add tests before their production contracts. Each command must select at least
one test and fail for the named missing behavior, not because zero tests ran or
an unrelated fixture failed.

| Criterion | Named expected-red proof |
| --- | --- |
| E1-E4 | `machine_config_rejects_foreign_network_authority_before_state_or_artifact_mutation` |
| E3-E4 | `embedded_machine_lifecycle_retains_parent_authority_across_alias_retarget` |
| E4 | `machine_delete_uses_persisted_manager_provenance_not_substituted_caller_roots` |
| E5 | `machine_provider_generation_is_parent_issued_persisted_and_monotonic` |
| E5-E6 | `guest_rejects_guest_minted_or_stale_parent_provider_authority_before_effects` |
| E6 | `guest_machine_api_claims_manager_before_listener_and_splits_workload_artifacts` |
| E7 | `parent_publication_conflict_fails_before_machine_api_io` |
| E7 | `same_numeric_guest_and_parent_publications_use_distinct_authority_roots` |
| E8 | `forwarded_start_activates_parent_publication_only_from_exact_complete_evidence` |
| E8 | `ambiguous_forwarded_start_retains_unpublished_parent_fence` |
| E9 | `exact_forwarded_stop_withdraws_before_io_and_releases_after_receipt` |
| E9 | `ambiguous_forwarded_stop_retains_parent_publication_fence_after_fresh_process_recovery` |
| E10 | `machine_stop_withdraws_all_publications_before_provider_and_releases_only_after_exact_absence` |
| E11 | `stale_machine_generation_cannot_activate_or_release_current_publication` |
| E13 | `wsl2_refuses_machine_network_composition_before_authority_mutation` |

Record for every expected-red command:

- exact command and exit status;
- selected/running/failed test counts;
- first load-bearing assertion;
- durable phase and effect-observer state at failure; and
- the production change that makes only that assertion green.

### Executed fail-before and convergence evidence

| Proof | Expected-red evidence | Current green evidence |
| --- | --- | --- |
| Parent provenance, retained alias, substituted deletion | `cargo test -p nimbus-cli machine::tests::network_authority -- --test-threads=1 --nocapture` exited 101 with 3 selected, 2 failed, 1 passed. Foreign provenance was accepted through state refresh/write; substituted deletion removed persisted authority. The retained-alias control already passed. | Same command: 3 passed, 0 failed, 0 ignored, 903 filtered out. Authentication runs before attempted lock/root creation and repeats under the held record lock; existing path aliases normalize through the nearest existing ancestor. |
| Parent-issued monotonic provider generation | Exact named command exited 101 with 1 selected/failed: restart returned generation 1 instead of 2 after persisted generation-1 runtime evidence. | Exact named command: 1 passed, 0 failed, 0 ignored, 906 filtered out. Initial generation is 1; persisted matching authority advances with `checked_next` before lease/provider preparation. |
| Provider-managed WSL2 preflight | Exact named command exited 101 with 1 selected/failed: the configured network authority root existed after the named WSL2 unavailable error. | Exact named command: 1 passed, 0 failed, 0 ignored, 907 filtered out. Provider-mode preflight now precedes parent-manager claim, machine artifacts, guest composition, and provider effects. |
| Authenticated list/info state refresh | No separate expected-red command: the source audit proved list/info used raw record locks while refreshing and writing machine state. | Focused list-order test: 1 passed, 0 failed, 0 ignored, 905 filtered out. List/info now receive the retained parent authority and authenticate before and under each record lock. |
| Parent boot evidence and guest provider authentication | `guest_rejects_guest_minted_or_stale_parent_provider_authority_before_effects` exited 101 with 1 selected/failed because the old command ignored the fixture, began serving, and reached an unrelated `/proc` dependency; the load-bearing timeout proved the forbidden manager/listener path was entered. | Exact named command: 1 passed, 0 failed, 0 ignored, 906 filtered out. Strict evidence is loaded and adapter-authenticated before manager, filesystem, listener, workload, or provider effects; bootc mutation stale-authority proof is also 1/0/0 with 906 filtered out. |
| Guest manager ordering and split roots | `guest_machine_api_claims_manager_before_listener_and_splits_workload_artifacts` exited 101 with 1 selected/failed because the old command skipped the held manager and created the listener before reaching an unrelated `/proc` dependency. | Exact named command: 1 passed, 0 failed, 0 ignored, 906 filtered out. The guest claims the manager first, injects one OCI network process, keeps network state at `control` and workload state below `control/service-sandboxes`, and permits deterministic final-drop reopen. |
| Direct Machine API activation and image recipe | The source audit found independently enabled `nimbus.socket` units in both Nimbus templates and `machine-os`, plus a host start script that enabled `nimbus.service` across boots. | The direct-listener test is 1/0/0 with 906 filtered out and proves bounded socket observation, owner-only mode, SELinux relabel ordering, active-service proof, and no enable/socket activation. Nimbus template proof is 1/0/0 with 1 filtered out. The dedicated `machine-os` worktree removes the socket unit and default enablement; recipe, build-helper, OCI-layout, provider-artifact, publish-helper, SELinux-gate, and diff checks all pass. |
| Parent publication reservation and OS-node realm split | The frozen Band C packet exited 101 with 8 selected/running/failed, 0 ignored, and 910 filtered. Parent conflicts emitted Machine API I/O, no parent batch activated or fenced, live/fresh stop ordering had no parent phases, stale generations had no authority to reject, and inspect had no parent bytes to preserve. | `cargo test -p nimbus-cli machine::backend::tests::publication_authority -- --test-threads=1 --nocapture`: 8 passed, 0 failed, 0 ignored, 914 filtered. The packet covers conflict-before-I/O, separate guest/parent roots, exact complete activation, partial/stale/crossed/untyped/EOF/lost/timeout/refused start ambiguity, live withdraw ordering, real-subprocess fresh recovery, exact retry convergence, stale-generation retention, and byte-unchanged inspect. |
| Durable parent publication intent | The source/fail-before packet had no parent-owned durable desired-attempt barrier, so a crash could not distinguish pre-I/O staged work from a request that may have crossed the Machine API boundary. | `cargo test -p nimbus-cli machine::publication_authority::tests -- --test-threads=1 --nocapture`: 4 passed, 0 failed, 0 ignored, 918 filtered. Staged/committed/terminal replay, checksum corruption, same-process concurrent opens, and unknown-entry fail-closed behavior are exact. |
| Guest observed evidence and zero-binding truth | The initial guest adapter could derive success from desired bindings, and zero desired bindings failed because no provider-effect artifact existed. | `cargo test -p nimbus-cli publication_evidence -- --nocapture`: 5 passed, 0 failed. The sandbox evidence packet passes 6/0, exact unexpose retry passes 1/0, and `zero_binding_preselected_workload_has_exact_empty_publication_evidence` passes 1/0 with 750 filtered. Desired bindings are never upgraded into observed receipts; even an exact empty set requires a durable authenticated observed-absence header bound to tenant, sandbox, manifest authority, and exact receipt set. Four directly affected backend/client transport and projection tests also pass individually after their fixtures adopted parent-plan IDs and explicit observed zero-binding evidence. |
| Whole-machine stop, restart, and deletion | `machine_stop_withdraws_all_publications_before_provider_and_releases_only_after_exact_absence` initially exited 101 with 1 selected/failed and 922 filtered at the first provider observer: no parent publication was yet fenced before provider stop I/O. | The exact proof and the final eight-test `machine::manager::tests::stop_cleanup` suite pass. Two independent plans are `CleanupPending` before the first provider request; exact process-birth observation permits old-incarnation publication release and a confirmed-stopped SSH rebind claim. Missing, corrupt, or crossed process evidence retains both fences; a recycled PID is never signaled. Start rejects any nonterminal prior provider-instance plan before image/provider/state effects, while delete rejects nonterminal publications before SSH release or artifact deletion. |
| Full machine authority and caller isolation | The first full CLI convergence run exposed 39 test-only process-global manager collisions; a later run exposed 20 remaining fixture/root assumptions. Those failures proved the tests were constructing production singleton authority for isolated cases and guessing guest artifact paths. | Production retains one process-global manager. Tests now receive scoped primitive authorities through `#[cfg(test)]` handles, canonicalize aliases, and serialize only the few tests that genuinely assert process-global composition. `machine::tests::network_authority` passes 5/0/0, `machine::tests::records_state` passes 16/0/0, `kv::tests` passes 8/0/0, the same-numeric parent/guest proof passes 1/0/0, and the final full CLI suite passes 932/0/1. |
| Exact bind/composition census | The first live verifier failed NNCV006 because the source-derived inventory still described four removed inherited Machine API listener occurrences, stale shifted lines, and only one of two new withdrawal diagnostics; NNCV015 consequently could not consume a valid census. | The canonical inventory now classifies 62 authority occurrences, 36 non-authority risks, five local-IPC occurrences, and retires inherited Machine API listener authority under NNC4.6e. Direct bind and composition helpers pass, and the aggregate verifier passes 16/16. The repository-wide constructor/root/primitive-handle policy closure remains owned by NNC4.6f; this item records and classifies every machine-realm occurrence it changed rather than forking that owner. |

Integration checkpoint: `nimbus-network` passes 221/0/0,
`nimbus-machine` passes 27/0/0, `nimbus-sandbox` passes 736/0/24 across
its library, integration, and binary targets, and `nimbus-cli` passes
932/0/1. `nimbus-server` passes 601/0/28 with the recorded filtered names
covered by two exact inherited execution-base failures; both fail unchanged in a
clean detached `8468ddce3b5afc001356ea2a3ee099d29781957c` worktree and touch
no NNC4.6e source. `nimbus-assets` passes 9/0/0. The six affected crates pass all-target/all-feature
compilation, workspace `make clippy` passes with `-D warnings`, and
warning-denied all-feature rustdoc passes for all six crates.

## Full Review Finding Disposition

The sole full item review ran through the structured helper with
GPT-5.6 Sol/xhigh/fast. The helper used two internal passes to review the one
NNC4.6e unit of value; those passes are not separate item reviews. Every
finding was accepted because it identified an executable reliability or
single-authority defect:

| Finding | Acceptance criterion | Disposition and correction proof |
| --- | --- | --- |
| Bootc mutation clients did not retain the running parent forwarder authority. | E3, E5, E12 | Bootc now requires the retained runtime and injects its exact `MachineForwarderAuthority`. The source fail-before exited 1; `bootc_mutation_client_retains_running_parent_forwarder_authority` passes 1/0. |
| A lost successful stop followed by a missing-sandbox 404 could not converge. | E9, E16 | Stop retry reads durable authenticated absence for the exact tenant/sandbox/manifest authority; an unrelated sandbox remains 404. Sandbox evidence passes 6/0, CLI publication evidence 5/0, and exact unexpose retry 1/0. |
| Exact gvproxy absence was not persisted when an unrelated stop step also failed. | E10, E11 | Whole-machine stop now settles publication authority and retains an exact confirmed-stopped SSH rebind claim whenever exact gvproxy absence is established, while preserving unrelated teardown diagnostics. The exact regression and final eight-test stop-cleanup packet pass. |
| A staged attempt whose complete lease set was already `Released` permanently wedged recovery. | E8, E9, E11 | Recovery authenticates the immutable exact membership, terminalizes the abandoned attempt, and permits a new generation. The exact fail-before failed 1/0; the corrected exact case and nine-test parent publication packet pass. |
| A subprocess proof depended on external GNU `timeout`. | E16, E17 | The harness now uses portable bounded child polling and reports child status/output/checkpoint evidence itself; the nine-test packet passes without an external timeout binary. |
| Guest cleanup still referenced the removed `nimbus.socket` activation path. | E6, E10, E14 | The direct guest stop script now owns only `nimbus.service`; the legacy socket/disable path is absent. The source fail-before found the old path; two direct guest lifecycle tests pass. |
| Scalar lease mutations could partially mutate one member of a multi-member exact plan. | E7-E9, E11 | Scalar planned transitions require an exact durable singleton; all multi-member mutations authenticate and lock the complete batch in stable order, rolling back partial lock acquisition. Four scalar regressions, one contention case, and all 221 network tests pass. |
| `Restart=on-failure` could recreate an unlabeled Machine API socket outside host choreography. | E6, E10, E14 | Nimbus and `machine-os` templates now use `Restart=no`; the host remains the sole retry authority and must repeat authority, readiness, and relabel choreography. Asset tests pass 9/0 and all six deterministic `machine-os` lanes pass. |
| Absence evidence was authenticated against the caller-supplied forwarder instead of manifest authority, allowing crossed providers and I/O before authentication. | E5, E8-E9, E12 | Evidence is tenant- and sandbox-qualified, duplicate receipts are rejected, detached evidence is found across state roots only after manifest-authority preflight, and crossed provider evidence cannot overwrite or trigger forwarder I/O. Sandbox evidence passes 6/0 and CLI publication evidence 5/0. |

Because these findings materially changed executable behavior, binding decision
17 permits exactly one narrow correction review over these nine corrections.
No second full review is warranted.

## Narrow Correction Review Finding Disposition

The one permitted narrow correction review ran through structured helper
threads `019fafcb-aa4e-7840-9b96-b3cf086dc4bb` and
`019fafcf-65a6-7242-8f3c-e6e9a12ca084` using GPT-5.6 Sol/xhigh/fast over
combined executable SHA-256
`f2ce2650b7e31b696b981ba5dbdeddb937afed29adc2dbbdd8742ee3994b310f`.
It found two P1 defects. Both are accepted:

| Finding | Acceptance criterion | Disposition and correction proof |
| --- | --- | --- |
| A numeric live gvproxy PID could be reused, causing stop to signal an unrelated process and promote that result to provider absence. | E5, E10-E11, E16 | The parent now atomically persists a strict receipt containing the exact forwarder authority, PID, and OS process-birth token immediately after spawn. Stop authenticates the receipt, re-observes the birth token before every signal, and treats a replacement only as old-incarnation absence. The initial missing-contract run exited 101 before the new evidence could become signaling authority. The recycled-PID regression, process-identity parser, and final eight-test stop packet pass; the unrelated process remains alive, old publications release, and SSH remains an exact confirmed-stopped claim. |
| Start/stop response deserializers authenticated members individually but allowed duplicate bindings to replace an omitted exact-set member. | E8-E9, E12, E16 | The exact DTO test failed 0/1 at the duplicate-start assertion before correction. Both deserializers now reject repeated bindings after authenticating tenant, sandbox, provider instance, generation, and outcome. The same test passes 1/0, and full `nimbus-machine` passes 27/0/0. |

The accepted changes were reproved with the affected and final item gates. They
do not authorize another review: binding decision 17 allows one narrow
correction review, and that review is complete.

## Behavioral Proof Strategy

Use deterministic observers rather than sleeps:

- a recording Machine API transport counts and orders requests;
- a parent publication observer records reserve, claim, activate, withdraw,
  cleanup-pending, and release transitions;
- a provider observer records gvproxy/VMM start and stop;
- temporary roots prove attempted paths stay absent;
- cross-process helpers contend through the real durable authority;
- crash/restart tests spawn a fresh subprocess with no handed-over in-memory
  guards; and
- response scripts cover exact, stale, crossed, partial, malformed, EOF,
  timeout, and refusal outcomes.

At least one test must prove this exact parent/guest ordering:

```text
start:
  parent reserve+claim
    -> Machine API request
      -> guest wildcard bind
        -> gvproxy expose
          -> exact receipt
            -> parent activate

stop:
  parent withdraw
    -> Machine API stop
      -> gvproxy unexpose
        -> exact absence receipt
          -> guest stop completion
            -> parent release
```

The whole-machine proof separately asserts:

```text
withdraw every parent publication
  -> stop VMM/API forwarding
    -> stop gvproxy
      -> authenticate exact gvproxy absence
        -> release every parent publication and retain/release SSH as specified
          -> record machine stopped
```

## Static Seam Checklist

- [x] `MachineRootLayout` has no network root or authority setter.
- [x] Direct host mutation owns exactly one parent manager.
- [x] Live-server delegation constructs no local manager.
- [x] Embedded lifecycle retains the injected start authority.
- [x] Production machine SSH code contains no raw port-authority open.
- [x] Config provenance is authenticated before the first mutation/effect.
- [x] Guest manager is claimed before filesystem/listener/provider work.
- [x] Guest workload and network roots are distinct named concepts.
- [x] Guest container uses the injected `OciNetworkProcess`.
- [x] Parent issues provider handle and monotonic generation.
- [x] Guest boot/node/IP/PID/port values cannot mint parent identity.
- [x] Parent publication reserves before Machine API I/O.
- [x] Exact receipts, not HTTP status or handle presence, activate/release.
- [x] Ambiguous start/stop retains nonterminal conflict authority.
- [x] Whole-machine stop withdraws parent publications before gvproxy stop.
- [x] WSL2 stays provider-managed, unavailable, and fail-closed.
- [x] No IP address is used as workload identity.
- [x] PDP/PEP, service naming, TLS/interception CA, cluster transport, and
      system projection ownership are unchanged.
- [x] `nimbus-network` has only the `nimbus-core` workspace edge and no effect
      import.
- [x] Every E1-E17 criterion, including the sole item review and one permitted
      narrow correction review, has exact
      evidence in the closeout ledger.

## Module-Size and Complexity Disposition

The intended concept-owned files are:

- `machine/network_composition.rs` for parent manager/provenance ownership;
- `machine/publication_authority.rs` for the parent publication state machine;
  and
- `machine/api/network_composition.rs` for the guest manager/backend
  composition.

Do not add publication or manager switchboards inline to `machine/api.rs`,
`machine/backend.rs`, `machine/handlers.rs`, or
`machine/manager/ports.rs`. Test-heavy files may move intact private test
modules to concept-owned children when they cross the repository threshold.
Every affected file at 1,500 lines or above requires a recorded owner
disposition; files at 2,000 or above must be decomposed or carry a strong
ownership exception.

Candidate measurements:

| File | Lines | Disposition |
| --- | ---: | --- |
| `crates/nimbus-network/src/port_lease.rs` | 1,922 | Retain as the explicit deep-module exception already owned by the plan. It is the public durable lease state-machine root; exact-plan mechanics are extracted into `plan_batch.rs`, and lifetime mechanics remain in the `port_lease/lifetime/` concept tree. Splitting the public transition surface by line count would scatter the invariants that validation must check atomically. The file remains below the mandatory 2,000-line decomposition threshold. |
| `crates/nimbus-network/src/port_lease/lifetime.rs` | 1,645 | Retain as a narrow threshold exception. It owns one coherent process-lifetime/recovery state machine; NNC4.6e keeps exact-batch reservation mechanics in `lifetime/batch_reservation.rs` and colocates dead-lifetime batch recovery with the lifetime state machine rather than duplicating it in an upper adapter. |
| `crates/nimbus-cli/src/machine/publication_authority.rs` | 932 | Below threshold and concept-owned: durable parent publication intent plus whole-machine reconciliation helpers. |
| `crates/nimbus-cli/src/machine/backend.rs` | 1,059 | Below threshold; remains the forwarded sandbox adapter while portable intent and plan construction live in `publication_authority.rs`. |
| `crates/nimbus-cli/src/machine/manager/stop.rs` | 604 | Below threshold and owns whole-machine provider-stop translation, including exact process-incarnation signaling. |
| `crates/nimbus-cli/src/machine/manager/process_identity.rs` | 248 | Below threshold and concept-owned: parent-authenticated PID plus OS birth-token capture and observation. |
| `crates/nimbus-cli/src/machine/manager/tests/stop_cleanup.rs` | 865 | Below threshold and concept-owned behavioral proof. |
| `crates/nimbus-cli/src/machine/api/service_workloads.rs` | 1,152 | Below threshold. Production Machine API workload choreography remains together; its explicit test-only observation facade does not add production authority. |
| `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_evidence.rs` | 703 | Below threshold and concept-owned: authenticated exposed/absent provider evidence, durable encoding, and manifest-authority lookup. |

No affected file reaches 2,000 lines. The three new machine authority concept
owners remain below 1,000 lines, and the strict Clippy cleanup replaces the
eight-argument guest-readiness switchboard with one named process set.

## Pre-Review and Closeout Gates

Focused tests run throughout Bands A-D. Before structured review:

1. every E1-E16 test and static assertion is green;
2. full affected `nimbus-network`, `nimbus-machine`, `nimbus-sandbox`,
   `nimbus-cli`, and `nimbus-server` suites pass with exact counts and skips;
3. affected all-target/all-feature check, strict Clippy, and warning-denied
   rustdoc pass;
4. `cargo fmt --all --check`, `git diff --check`, dependency/effect scans, the
   live composition verifier and its self-test pass;
5. module sizes have explicit dispositions;
6. docs and site gates pass; and
7. the executable diff is candidate-frozen and its SHA-256 is recorded.

### Candidate closeout ledger

| Criterion | Status | Evidence |
| --- | --- | --- |
| E1 | pass | `MachineRootLayout` serializes exactly five artifact roots, rejects the removed `network_state_root` field, and exposes no network setter or resolver. |
| E2 | pass | Parent provenance/alias/substitution suite passes 5/0/0; authentication precedes attempted-root, state, lease, provider, and artifact effects. |
| E3 | pass | Direct commands retain one outer manager, live-server dispatch returns before local composition, embedded start injects its retained authority, and scoped tests cannot claim the production singleton. |
| E4 | pass | SSH lifecycle receives a manager-derived handle; substituted caller roots cannot release foreign authority or delete either artifact tree. |
| E5 | pass | Provider instance is parent-issued and persisted; generation advances durably before launch and stale/guest-minted authority is rejected. |
| E6 | pass | Guest manager claim precedes control/listener/backend effects; network and `service-sandboxes` artifact roots remain distinct; one injected OCI process owns guest network state. |
| E7 | pass | The eight-test parent packet proves atomic full-batch conflict before Machine API I/O and distinct parent/guest roots for the same numeric port. |
| E8 | pass | Only exact complete authenticated receipts activate; partial, crossed, stale, untyped, lost, EOF, timeout, and refusal outcomes retain an unpublished fence. |
| E9 | pass | Live and fresh-process stop execute withdraw, guest stop/unexpose, exact absence, release; ambiguous evidence retains the exact binding and generation. |
| E10 | pass | Six-test whole-machine cleanup suite proves all publications are fenced before provider stop and release only after exact gvproxy absence; SSH follows its explicit restart/delete lifecycle. |
| E11 | pass | Start rejects every nonterminal prior provider-instance plan before image/provider/state effects; stale receipts cannot activate/release and exact absence permits deterministic next generation. |
| E12 | pass | Inspect is byte-unchanged/read-only; strict DTOs deny unknown fields and require parent authority without policy, naming, socket, or raw-address identity. |
| E13 | pass | krunkit/vfkit share the host-managed suite; WSL2 returns its named unavailable error before parent manager, guest, artifact, or provider mutation. |
| E14 | pass | Source-derived 62-authority/36-risk census is fully classified; no mixed `MachineRootLayout`, production raw SSH primitive reopen, guest-issued parent handle, unleased publication, or unclassified Machine API mutation remains. NNC4.6f retains repository-wide constructor-policy closure. |
| E15 | pass | Cargo metadata reports `nimbus-core` as the sole workspace edge; aggregate NNCV004/NNCV007/NNCV012 and direct effect scans pass. |
| E16 | pass | Focused happy/edge/error, contention, alias, substitution, crash/restart, stale-generation, partial-batch, and exact/ambiguous tests assert phases, identities, ordering, paths, and forbidden-effect counts. |
| E17 | pass | Final affected tests, all-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, module sizes, 62-authority/36-risk/five-local-IPC census, composition census, live verifier 16/16, verifier self-test 51/51, and six `machine-os` lanes are green. The sole full item review and one permitted narrow correction review are fully dispositioned. Docs 108/site 17/17 pass after final ledger truth-up. |

Then run exactly one complete structured review with GPT-5.6 Sol, xhigh
reasoning, and fast mode. Disposition every finding against E1-E17. Only an
accepted finding that materially changes executable code permits exactly one
narrow correction review focused on that defect. After any accepted executable
change, rerun its affected proofs and all final item gates.

NNC4.6e is complete only when this proof, the canonical plan header, plan index,
task row, checkpoint row, exact review disposition, final counts, executable
digest, and one containing item commit agree.
