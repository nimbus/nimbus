# NNC4.6g Standalone KV Network Composition

Date: 2026-07-28

Status: `complete; exact item commit payload frozen`

Owner: NNC4.6g in
`docs/private/plans/nimbus-network-control-plane-plan.md`.

Source commit:
`762d053e974b1b7d8e831b4216a206268f60e238`

Source tree:
`a428060a939951de31dd45cd0ef3db21f3581998`

Owner worktree:
`/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit`

## Unit Of Value

NNC4.6g gives the standalone `nimbus kv` process the same typed local-node
network authority as start, dev, and standalone Compose without making KV a
child of those lifecycles:

```text
operator-owned LocalNodeNetworkRoot
  -> LocalNetworkManagerBootstrap
    -> LocalNetworkAuthority
      -> KV listener config / prepared claim / active listener
    -> NetworkCapabilityRegistry::new(empty)
  -> frozen and retained LocalNetworkManager
  -> nimbus-kv reserve -> bind/adopt -> activate
  -> bound-listener startup observation
  -> nimbus-kv RESP serve loop
```

This is one reviewable item because root selection, manager retention, empty
capability truth, pre-bind conflict, and post-bind observability are one
standalone process contract. It does not reopen NNC4.6d's start, dev, Compose,
server, or OCI composition. Machine host/guest authority remains NNC4.6e, and
the complete constructor/root/primitive-handle census remains NNC4.6f.

## Source-Proven Current State

```text
KvCommand
  --control-data-dir
  -> flag > NIMBUS_CONTROL_DATA_DIR > ./data
  -> println!("listening")
  -> create/open KV data store
  -> NimbusKvListenerConfig(raw PathBuf)
  -> PreparedKvListener::prepare
       -> LocalPortLeaseAuthority::open(raw path)
  -> bind / adopt / activate
```

| Current owner/site | Audited behavior | Defect NNC4.6g closes |
| --- | --- | --- |
| `nimbus-cli/src/kv.rs` | Resolves a raw network root from control-plane flag/environment/CWD fallback. | Standalone KV can select a different host-global authority from start/dev/Compose on the same logical node. |
| `nimbus-cli/src/kv.rs` | Prints the configured address and credential before store creation, durable reservation, or kernel bind. | A failed launch can emit a false listening claim, and port `0` can never report the actual kernel address. |
| `nimbus-kv/src/listener.rs` | Public config stores `PathBuf`; each prepare opens `LocalPortLeaseAuthority` from that path. | Production bypasses manager composition, and a path remains an authority locator instead of a retained capability. |
| `nimbus-kv/src/listener.rs` | Prepared and active listeners retain only the primitive port authority. | Dropping an outer manager-derived config could release the process-composition claim while the listener remains live. |
| `nimbus-kv/src/server.rs` | `bind_listener` returns the leased socket, but only private `serve_leased` can consume it. | The CLI cannot observe a successful actual bind before starting the server without bypassing or duplicating the serve lifecycle. |
| `nimbus-network/src/manager.rs` | Already provides staged bootstrap, manager-derived authority, immutable registry freeze, alias authentication, and process-lifetime fencing. | KV does not consume the canonical seam. |
| `nimbus-kv/tests/network_listener.rs` | Proves contention, crash recovery, provider-assigned activation, external adoption, failure receipts, and cleanup through raw-root construction. | The lifecycle is strong, but it does not prove manager-derived retention or alias pinning. |

`nimbus-kv` already owns every socket and RESP effect. Its existing lifecycle
orders reserve/claim before bind, adopts exact provider evidence, activates
before serving, records proven no-effect failures, and reconciles dead
process-bound owners. NNC4.6g preserves that state machine.

## Target Ownership And Ordering

```text
nimbus-operator                 nimbus-network
typed node-root policy          portable manager / authority / registry
         |                               |
         +------------> nimbus-cli <-----+
                         standalone KV composition
                                  |
                                  v
                           nimbus-kv listener
                    durable lease + real TCP effect
                                  |
                                  v
                         post-bind observation
                                  |
                                  v
                            RESP serve loop
```

Mandatory startup order:

1. Parse and validate tenant, credential, bind, and store-mode inputs without a
   network or filesystem effect.
2. Resolve one `LocalNodeNetworkRoot` using explicit
   `--network-state-dir`, then `NIMBUS_NETWORK_STATE_DIR`, then the
   operator-owned platform default.
3. Bootstrap one `LocalNetworkManager` before the KV data store, durable
   listener reservation, kernel bind, or success output.
4. Freeze exactly `NetworkCapabilityRegistry::new([])`. Standalone KV supplies
   neither an attachment nor an ingress-provider capability bundle.
5. Construct the KV data store in its existing storage owner.
6. Derive the KV listener config from the manager authority.
7. Reserve/claim the durable listener identity in `nimbus-kv`.
8. Bind and activate the concrete socket in `nimbus-kv`.
9. Construct a startup observation from the bound listener's actual address.
10. Emit the listening and credential output.
11. Transfer the exact leased listener into the existing RESP serve loop.

The retained manager, manager-derived listener authority, active listener,
store, and server configuration live for the serve future. On a synchronous
failure after bind, the existing confirmed-close settlement path remains
mandatory.

## Frozen Seam

The semantic public seam is:

```rust
impl NimbusKvListenerConfig {
    pub fn from_network_authority(authority: LocalNetworkAuthority) -> Self;

    pub fn from_network_authority_for_incarnation(
        authority: LocalNetworkAuthority,
        incarnation: impl AsRef<str>,
    ) -> Self;

    pub fn reconstruct_direct(
        state_root: impl AsRef<Path>,
    ) -> Result<Self, PortLeaseError>;

    pub fn reconstruct_direct_for_incarnation(
        state_root: impl AsRef<Path>,
        incarnation: impl AsRef<str>,
    ) -> Result<Self, PortLeaseError>;
}

pub async fn serve_listener(
    listener: NimbusKvListener,
    config: NimbusKvConfig,
) -> Result<(), KvError>;
```

Exact names may improve before the expected-red packet is frozen, but these
semantics may not:

1. Production accepts a `LocalNetworkAuthority`, not a raw path or primitive
   authority.
2. Config, prepared claim, and active listener each retain the authority
   variant so a live manager-derived listener retains the process-composition
   claim mechanically.
3. The listener lifecycle clones the already-open port authority from the
   retained capability; it never reopens production authority from a path.
4. Direct reconstruction is one explicitly named test/embedder/recovery seam.
   It opens once during construction and is classified by NNC4.6f. It is not a
   compatibility alias and no production CLI call may use it.
5. `serve_listener` exposes the existing leased-listener transition. It does
   not expose the raw Tokio socket, move socket effects into the CLI, or create
   a second serve loop.
6. A startup observation is constructible only from a successfully active
   `NimbusKvListener` and uses `local_addr()`, never the requested address.

## Frozen Architecture Decisions

1. `nimbus-operator` remains the sole typed platform-path-policy owner.
   `nimbus-cli` composes the standalone process because it already depends on
   operator, network, KV, and server crates.
2. `nimbus-kv` remains the listener effect and RESP protocol owner.
   `nimbus-network` gains no Tokio, socket, protocol, CLI, operator, server, or
   storage dependency.
3. KV data paths, engine/control paths, project roots, and the current working
   directory never become fallback network authority.
4. Standalone KV has no attachment provider and does not report itself as a
   general ingress provider. Its frozen registry is exactly empty. The
   listener lease is durable authority, not a fabricated provider capability.
5. The manager is frozen before listener work even though the registry is
   empty. Empty capability truth and host-global lease authority are distinct
   concerns.
6. A managed KV listener retains `LocalNetworkAuthority`; a direct
   reconstruction retains the once-opened `LocalPortLeaseAuthority`. Both pin
   their original store across alias retargeting.
7. The production CLI neither opens `LocalPortLeaseAuthority` nor accepts a
   primitive handle. It injects the manager-derived capability.
8. Stable `ListenerId`, `PortLeaseId`, generation, epoch, provider-attempt
   handle, provenance, and lifetime fence remain unchanged. An IP address is
   not workload identity.
9. Tenant credential admission and KV durable-data ownership remain distinct
   from host-port allocation authority.
10. Server/KV conflicts are settled in the shared durable authority before the
    losing KV kernel bind. No probe-then-bind port scan is introduced.
11. Logical service names, DNS, forwarding, proxy/PEP behavior, certificates,
    machine networking, cluster transport, system projection, and sandbox
    provider effects remain outside this item.
12. Pre-launch breaking-change policy applies: remove the ambiguous raw-path
    `new`/`for_incarnation` surface instead of retaining a compatibility shim.

## Frozen Acceptance Criteria

| ID | Criterion |
| --- | --- |
| G1 | `nimbus kv` exposes `--network-state-dir` and consumes `LocalNodeNetworkRoot`; explicit flag, environment/platform policy, invalid-path behavior, help surface, and project/CWD independence are proven by KV integration plus the operator resolver suite. `--control-data-dir`, `NIMBUS_CONTROL_DATA_DIR`, `./data`, KV data paths, and control paths are absent from KV network-root selection. |
| G2 | Standalone KV bootstraps the manager before KV data-directory/store creation, durable listener reservation, kernel bind, or success output. A second same/alias/divergent manager attempt returns typed active/attempted evidence and leaves the attempted root and KV data target untouched. |
| G3 | Production listener config is constructed from `LocalNetworkAuthority`. Config, prepared claim, and active listener retain that manager-derived capability through serve. Dropping temporary outer manager/config values cannot admit another composition while the listener lives; final listener/composition drop permits deterministic reopen. |
| G4 | Alias retargeting after bootstrap cannot redirect prepare, bind, activation, settlement, or restart reconciliation. The original canonical authority receives the records and the retargeted foreign root remains untouched. |
| G5 | Direct raw-root construction is replaced by an explicitly named, fallible, one-open reconstruction seam for tests/embedders. Production CLI and production listener preparation contain no raw-root or `LocalPortLeaseAuthority::open` path. NNC4.6f remains the owner of the workspace-wide final census. |
| G6 | KV-only composition freezes exactly zero capability bundles and retains that immutable registry for the process lifetime. No attachment, ingress, DNS/name, forwarding, certificate, proxy, machine, or provider capability is synthesized. |
| G7 | A server listener and standalone KV listener requesting the same exact host binding through one manager conflict in the durable port authority before the losing KV binder runs. A kernel probe can bind the address after the rejected KV preparation, proving no losing socket effect occurred; stable winner/loser identities remain in the diagnostic. |
| G8 | The CLI listening observation is unavailable until `bind_listener` returns an active `NimbusKvListener`; it renders the kernel's actual nonzero address, including a provider-assigned request for port `0`. A durable conflict, external `AddrInUse`, store failure, invalid input, or manager failure emits no listening/credential success output. |
| G9 | The exact leased listener transfers to `serve_listener`; no raw-socket extraction or second server loop exists. Synchronous setup/output failure after bind closes and settles the lease, while ambiguous task/process interruption retains the existing fence for reconciliation. |
| G10 | Existing NNC3.6/NNC3.8 contention, adoption, activation, failure-receipt, external ownership, crash/restart, withdraw/release, and tenant-isolated RESP behavior remain green with direct reconstruction used only in explicit tests. |
| G11 | Desired listener identity, durable lease/provider evidence, kernel binding, and emitted observation remain distinct. Observed output never becomes desired state or lease authority. |
| G12 | `nimbus-network -> nimbus-core` remains the sole initial workspace edge; effect/transport/policy/naming/cluster scans remain empty. No IP address becomes workload identity and no upper crate enters `nimbus-network`. |
| G13 | Focused fail-before proofs, full `nimbus-kv` and affected `nimbus-cli` suites, all-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, exact bind/construction census, live verifier/self-test, and docs/site gates pass with exact counts recorded here. |
| G14 | After G1-G13 are green and the item diff is candidate-frozen, exactly one GPT-5.6 Sol/xhigh/fast structured review is run and every finding is dispositioned. Material executable correction permits exactly one narrow correction review. The ledger is closed and one exact NNC4.6g item commit is created; no push or PR occurs. |

## Candidate Acceptance Evidence

| ID | Status | Evidence |
| --- | --- | --- |
| G1 | `pass` | KV parsing/help proves `--network-state-dir` and removal of `--control-data-dir`; the KV root test excludes control-environment, data-path, and CWD fallbacks. Full `nimbus-operator` is 43 passed, including explicit/environment/platform precedence, invalid explicit/environment/platform roots, Linux/macOS/Windows policy, project-CWD independence, and effect-free resolution. |
| G2 | `pass` | `manager_and_store_failures_emit_no_success_and_precede_later_effects` proves same-root, lexical-alias, and divergent second compositions retain typed active/attempted evidence, create no KV data, emit no success, and leave the divergent authority untouched. `standalone_kv_freezes_empty_registry_and_rejects_divergent_root_before_mutation` independently proves the empty freeze and pre-mutation failure. |
| G3 | `pass` | Production constructs only through `from_network_authority*`; config, prepared claim, and active listener retain `NimbusKvListenerAuthority::ManagerDerived`. The manager-derived integration drops the outer config and manager, proves the active listener still excludes another composition, then proves final close/drop admits deterministic reopen. |
| G4 | `pass` | The manager-derived integration retargets the symlink after bootstrap but before listener preparation. Reconciliation, reserve/claim, bind, activation, and settlement all write the original canonical authority; the foreign root stays absent and the source record reaches `Released`. |
| G5 | `pass` | Old raw-path `new`/`for_incarnation` constructors are deleted. `reconstruct_direct*` is fallible and contains the sole one-time primitive open; production CLI and listener preparation use the retained capability. The exact composition census classifies both managed construction and direct reconstruction. |
| G6 | `pass` | Standalone preparation freezes `NetworkCapabilityRegistry::new(Vec::new())`, retains the frozen manager, and asserts exactly zero selections. The static composition census rejects fabricated provider construction. |
| G7 | `pass` | `server_and_kv_conflict_durably_before_the_losing_kv_bind` proves one manager produces stable server winner/KV loser identities, rejects the KV claim in durable authority, and leaves the kernel address untouched for the prepared server owner. |
| G8 | `pass` | Provider-assigned port `0` renders the actual nonzero kernel address only after active bind. Invalid input, same/alias/divergent manager rejection, KV-store failure, durable conflict, and external `AddrInUse` all emit zero success bytes. |
| G9 | `pass` | Public `serve_listener` consumes the exact `NimbusKvListener` and delegates to the one existing RESP loop. It independently revalidates both the configured and actual kernel addresses before serving; a confirmed policy rejection closes the concrete socket and settles the exact Active lease to `Released`. Output failure likewise closes and settles to `Released`; the existing synchronous setup-failure and process-crash suites preserve exact cleanup or fenced reconciliation. |
| G10 | `pass` | Full `nimbus-kv`: 25 passed, 0 failed, 3 skipped. This includes two-process contention, external adoption/collision, provider-assigned activation, failure receipts, crash/restart reconciliation, close/release, loopback-policy enforcement at the leased serve transition, RESP authentication, and tenant isolation. |
| G11 | `pass` | Stable `ListenerId`/request/fence remain in config, durable lease/provider evidence remains in `nimbus-network`, the real socket remains in `nimbus-kv`, and `KvStartupObservation` is created only from `NimbusKvListener::local_addr`; output contains no authority handle. |
| G12 | `pass` | Full `nimbus-network`: 200 passed. Cargo metadata reports the sole workspace edge `nimbus-network -> nimbus-core`; NNCV012-NNCV015 report no forbidden effect/dependency, duplicate definition, address identity, or unclassified composition seam. |
| G13 | `pass` | Focused CLI 8/0, manager-derived alias 1/0, accepted-review correction 1/0, operator 43/0, network 200/0, full KV 25/0/3, and full CLI 902/0/1 pass. Affected all-target/all-feature check, strict Clippy, warning-denied rustdoc, format/diff, exact core-only edge, construction census, live verifier 16/16, verifier self-test 51/51, docs 108 pages, and site 17/17 pass. |
| G14 | `pass` | Full Sol/xhigh/fast thread `019fae60-9997-7170-ab36-a596f6cf4a02` found one P1 at the new public serve transition. It was accepted with a reachability qualification, reproduced as a bounded fail-before timeout, and corrected with local policy validation plus exact close/settlement. The one permitted narrow correction review, thread `019fae69-a064-72e3-bc44-eb576a581848`, is clean with `patch is correct` at 0.96. The complete twelve-path corrected payload, including required plan-index routing truth-up, has no unstaged path; final executable SHA-256 is `4506cd001b9df11bab5f70fa77a3a33a0324fa0ca9bccb485797f04bf9722acd`. No further review is warranted. |

## Expected-Red Packet

Expected-red tests are added before production implementation:

| Packet | Fail-before behavior | Corrected proof |
| --- | --- | --- |
| R1 typed KV root | CLI still exposes `control_data_dir` and has no typed KV network preparation seam. | KV parses `--network-state-dir`, uses operator policy, and never derives network state from data/control/CWD. |
| R2 manager-derived constructor | KV listener config accepts only raw paths and reopens during prepare. | Managed constructors retain `LocalNetworkAuthority`; direct construction is explicitly named and fallible. |
| R3 manager and alias lifetime | A managed config/listener seam does not exist, so alias pinning and final-drop fencing cannot be expressed. | Retargeted aliases remain on the original authority, the listener retains the claim, and final drop permits reopen. |
| R4 honest empty registry | KV has no frozen manager or inspectable registry. | KV freezes and retains exactly zero selections before listener work. |
| R5 server/KV pre-bind conflict | KV is constructed from an independent raw root, so the production shared-manager server/KV conflict cannot be assembled. | The durable conflict fires before KV bind and the kernel address remains bindable by the prepared server owner. |
| R6 post-bind observation | Success text is emitted before bind and renders the requested address. | A bound-listener type-state renders the actual address; every pre-bind failure has no success observation. |
| R7 leased serve transfer | Only the private leased-listener serve transition exists. | The public narrow transition consumes the exact leased listener and retains existing cleanup behavior. |
| R8 static construction census | Production KV contains control-root fallback, raw-path config, and a primitive reopen. | Only typed-root bootstrap, manager-derived construction, empty freeze, bind-before-observe, and exact serve transfer remain. |

An expected-red compile failure counts only when it names a missing target seam.
Privacy mistakes, stale imports, malformed fixtures, or unrelated failures
must be corrected before evidence is recorded.

## Expected-Red Evidence

The tests were added without production behavior. Stale test imports and an
`expect_err` debug bound were corrected before evidence was accepted.

| Command | Result | Target-seam evidence |
| --- | --- | --- |
| `cargo test -p nimbus-kv --test network_listener manager_derived_listener_pins_alias_and_retains_process_claim_until_final_drop --no-run` | exit 101 | Two E0599 errors: `reconstruct_direct_for_incarnation` and `from_network_authority_for_incarnation` do not exist. The compiler identifies the old raw-path `new`/`for_incarnation` constructors as the only candidates. |
| `cargo test -p nimbus-cli kv::tests::kv_network_state_dir_is_an_explicit_node_root_and_control_flag_is_removed --no-run` | exit 101 | Fourteen target-seam errors: missing `network_state_dir`, typed root/composition/error types, manager-derived KV constructor, and prepare-and-announce type state. No privacy, fixture, stale-import, or unrelated compile error remains. |

This proves R1-R8 are not already satisfied by the source checkpoint. The next
change may implement only those missing seams and their lifecycle behavior.

## Structured Review Disposition

The complete candidate-frozen item received its sole full structured review
with GPT-5.6 Sol, xhigh reasoning, and fast service tier in thread
`019fae60-9997-7170-ab36-a596f6cf4a02`.

The helper's pre-engine secret scanner first rejected two synthetic diff
presentations: a deleted human-readable `Dev credential:` label was parsed as
an assignment when combined with a later environment-name literal, and an
unchanged `credentials` test-helper parameter appeared in a hunk header. No
model invocation occurred for those rejected attempts. The successful review
used a full repository mirror whose eleven path diffs were proved equivalent
after only those disclosed lexical neutralizations; the owner executable
remained frozen at
`fde38da3c4166a224de9a71f1d2f7a6027c13590f8c3bc8b241ee7e019b7a326`.

| Finding | Disposition | Reproduction and correction |
| --- | --- | --- |
| P1: public leased-listener serving bypasses the loopback-only policy | `accepted with reachability qualification` | Safe downstream construction was already guarded because public `bind_listener` and `adopt_listener` both reject non-loopback addresses and `NimbusKvListener` fields are private. The exported transition nevertheless lacked a local invariant and could become unsafe through an internal/future construction path. `leased_listener_serve_revalidates_loopback_and_settles_rejection` deliberately constructs an Active non-loopback listener through the crate-private effect seam while supplying a loopback config. Before correction it timed out after 250 ms in the accept loop (0 passed, 1 failed); after correction it returns `InvalidInput`, closes the descriptor, records the exact lease as `Released`, and permits immediate kernel-port reuse (1 passed, 0 failed). |

The material correction adds one validation function at the public transition:
both the configured bind and the listener's actual kernel address must remain
loopback-only before the store or accept loop begins. A confirmed rejection
consumes the exact `NimbusKvListener` through its existing
`close_after_confirmed_local_error` state transition, so cleanup failure remains
combined with the primary policy error and ambiguous task cancellation still
retains its fence.

Affected proofs were rerun after the correction: full `nimbus-kv` is 25 passed
with 3 skipped; full `nimbus-cli` is 902 passed with 1 skipped; affected
all-target/all-feature check, strict Clippy, warning-denied rustdoc,
format/diff, live verifier 16/16, and verifier self-test 51/51 pass.

The exactly one permitted narrow correction review ran in thread
`019fae69-a064-72e3-bc44-eb576a581848`. It reports zero findings, `patch is
correct`, and 0.96 confidence. No further review is permitted or warranted
under the item cadence.

## Behavioral Proof Matrix

| Behavior | Proof owner |
| --- | --- |
| Flag/help/root parity and control/data/CWD independence | `nimbus-cli/src/kv.rs` tests plus `nimbus-operator::paths` suite |
| Duplicate/divergent pre-effect failure and empty registry | CLI standalone-KV composition tests |
| Manager retention, alias retarget, direct reconstruction | `nimbus-kv/tests/network_listener.rs` |
| Server/KV exact-port conflict before binder | CLI standalone-KV composition test using public server and KV listener seams |
| Provider-assigned actual-address observation and failed-bind silence | CLI bound-startup-observation tests |
| Exact leased-listener transfer and synchronous cleanup | `nimbus-kv` server/listener tests |
| Existing crash/restart and two-process contention | `nimbus-kv/tests/network_listener.rs` |
| Tenant-isolated RESP behavior | `nimbus-kv/tests/resp_server.rs` |
| Dependency/effect/construction invariants | network verifier plus source-derived bind inventory |

## Failure, Rollback, And Reconciliation

| Failure point | Required result |
| --- | --- |
| Invalid root or duplicate process composition | Typed failure before attempted-root, data-store, lease, socket, registry-observation, or output effect. |
| Empty registry construction | Deterministic local error; bootstrap drops and a later clean launch can reclaim the root. |
| KV store creation | Manager claim drops; no listener lease or socket exists and no success output was emitted. |
| Durable reserve/claim conflict | Typed stable winner/loser evidence; no losing kernel bind; no success output. |
| Kernel bind failure | Existing durable no-effect failure receipt; no success output. |
| Adoption/activation failure | Socket closes and exact claimed lease settles through the existing combined-error path. |
| Leased serve transition receives a non-loopback configured or actual address | Reject before store creation or accept, close the concrete descriptor, settle the exact Active lease through the confirmed-local-error path, and preserve combined cleanup evidence. |
| Output failure after active bind | Confirmed local close and lease settlement before returning the output error. |
| Synchronous server setup failure | Existing listener close/settle path returns the primary and cleanup evidence. |
| Task cancellation or process crash | Active process-bound fence remains; existing dead-owner reconciliation inspects and releases before exact reuse. |
| Explicit shutdown | Withdraw, confirmed close, release, and durable record remain ordered in `nimbus-kv`. |

No rollback rewrites durable history. Retry uses the same stable request only
for the same generation/epoch/claim; a fresh process incarnation receives a
fresh stable listener identity and reconciles dead process-bound evidence.

## Seam Checklist

- [x] One typed logical-node root policy; no KV-local fallback policy.
- [x] One process manager claim; no independent production composition.
- [x] One immutable, honestly empty KV registry.
- [x] One manager-derived listener authority retained through active serve.
- [x] One explicitly named direct reconstruction seam; no compatibility shim.
- [x] One durable port authority shared with server; no host-port scan.
- [x] Socket effects remain in `nimbus-kv`.
- [x] KV data durability remains in `nimbus-kv`/`nimbus-storage`.
- [x] Tenant admission remains separate from allocation authority.
- [x] Startup observation follows actual activation and owns no authority.
- [x] Every public bind/adopt/serve transition enforces the loopback-only KV
      policy; confirmed serve rejection closes and settles the exact lease.
- [x] Desired/durable/effect/observed states remain distinct.
- [x] No provider capability is inferred from listener existence.
- [x] No service naming, DNS, forwarding, certificate, proxy, machine, or
      cluster ownership enters the item.
- [x] `nimbus-network` keeps only its `nimbus-core` workspace edge.
- [x] Every G1-G14 row has named executable/static evidence and exact counts.
- [x] Exactly one item-level review, one authorized narrow correction review,
      ledger closeout, and one exact item commit payload.

## Status Ledger

| Field | Value |
| --- | --- |
| Current phase | Complete. G1-G14 pass; the exact twelve-path corrected item payload is frozen for its one commit. |
| Last durable checkpoint | NNC4.6d commit `762d053e974b1b7d8e831b4216a206268f60e238`, tree `a428060a939951de31dd45cd0ef3db21f3581998`. |
| Dirty paths | Twelve staged owned paths and zero unstaged paths: standalone CLI/KV production and tests, exact inventory/census updates, canonical plan and routing index, completed NNC4.6d proof truth-up, and this proof. Start/dev/Compose/server behavior and every forbidden seam remain untouched. |
| Owned executable paths | `crates/nimbus-kv/src/listener.rs`, `crates/nimbus-kv/src/server.rs`, `crates/nimbus-kv/src/lib.rs`, narrow KV tests, `crates/nimbus-cli/src/kv.rs`, exact bind/construction verifier and inventory updates. |
| Forbidden paths | Start/dev/Compose/server behavior, OCI/sandbox implementation, machine realms, proxy/egress, services/naming, cluster, system projections, and `nimbus-network` effect/dependency expansion. |
| Last green | Focused CLI 8/0, alias lifecycle 1/0, review correction 1/0, operator 43/0, network 200/0, KV 25/0/3, CLI 902/0/1, affected check/Clippy/rustdoc, format/diff, core-only edge, verifier 16/16, and self-test 51/51. |
| Initial candidate identity | Pre-review-metadata staged tree `5f71145c17a6a1c4893e340eb5f8bdf95c01de4e`; full-diff SHA-256 `0e2b3fa32e4b575288e40981bf4f415f21f9da7277a0b9350367e5da5133b3c3`; reviewed executable SHA-256 `fde38da3c4166a224de9a71f1d2f7a6027c13590f8c3bc8b241ee7e019b7a326`. |
| Corrected candidate identity | Pre-ledger corrected staged tree `50a25de555c554d69933f088df591e13b5a8e104`; pre-ledger full-diff SHA-256 `ff9021c0e878ea4b9acfe2bd57902106c26443428af670771908653253a9ddea`; final executable SHA-256 `4506cd001b9df11bab5f70fa77a3a33a0324fa0ca9bccb485797f04bf9722acd`; eleven staged paths and zero unstaged paths. |
| Review state | Full thread `019fae60-9997-7170-ab36-a596f6cf4a02` produced one accepted P1 with a reachability qualification and exact correction proof. Narrow thread `019fae69-a064-72e3-bc44-eb576a581848` is clean at 0.96. Both authorized runs are fully dispositioned; no further review is warranted. |
| Next | Commit this exact NNC4.6g payload, then begin NNC4.6e with a read-only parent-host/guest-node machine-authority audit. |
| Blocker | None. |
