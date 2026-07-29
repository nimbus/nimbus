# NNC4.4 Machine-Provider Networking Modes Proof

Status: `complete; M1-M12 green`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Scope

NNC4.4 replaces the machine provider's ambiguous networking boolean with a
typed, source-grounded connectivity profile. It proves that:

- krunkit and vfkit use Nimbus-managed host networking;
- WSL2 is intended to use provider-managed networking but has no available
  Nimbus adapter or capability registration today;
- desired requirements, static provider topology, current-target availability,
  and runtime readiness are different facts;
- unsupported exposure, isolation, and sovereignty requirements reject with
  typed deterministic diagnostics; and
- VMM command/device capabilities never become segment-allocation,
  workload-attachment, ingress, or provider-effect authority.

This item also corrects one directly related audit defect: the non-Unix machine
stub currently reports a possibly active machine stopped without performing or
inspecting a provider stop. NNC4.4 cannot truthfully preserve unavailable-WSL2
fail-closed behavior until that false-success path is removed.

## Written Acceptance Contract

| ID | Verifiable success criterion |
| --- | --- |
| M1 | `MachineProviderCapabilities` uses the canonical `NetworkManagementMode`; `uses_provider_networking` and its boolean accessor are deleted with no compatibility alias. Krunkit and vfkit map to `NimbusHostManaged`; WSL2 maps to `ProviderManaged`. |
| M2 | A concrete, machine-owned connectivity value composes only existing portable attachment, exposure, and sovereignty dimensions. It is closed data plus deterministic evaluation: no trait, callback, async operation, socket, process, probe, environment lookup, provider handle, registry mutation, or effect enters the value seam. |
| M3 | Static topology, capability evidence, target availability, and runtime readiness remain distinct. Krunkit/vfkit may return current capability evidence only on macOS. Linux/Windows do not advertise those macOS adapters. WSL2 reports its intended `ProviderManaged` mode but returns a typed unavailable result on every current target; it never produces a production capability registration. |
| M4 | Krunkit and vfkit have distinct `MachineProvider` identities but identical conservative machine-connectivity evidence: Nimbus host management, virtual-machine guest attachment, a workload-namespace-equivalent guest boundary, host loopback exposure, local-only control, no external dependency, and offline reconciliation. No service-port, public/private reachability, IPv6, tenant-segment, DNS, TLS, forwarding, or workload-ingress fact is inferred from the management SSH listener. |
| M5 | Exact requirements pass; host-managed and provider-managed evidence cannot substitute. Missing or wrong management, attachment, isolation, exposure, control-plane locality, external dependency, and offline-restart evidence produces `NetworkCapabilityMismatch` values in deterministic dimension order. A mismatch is diagnostic only and never selects a different provider. |
| M6 | CLI lifecycle decisions exhaustively match `NetworkManagementMode`. The current host-managed launch composition remains structurally gvproxy/VMM/SSH-lease based. An unavailable provider-managed mode returns the canonical named provider error; the current unreachable `post_start_networking` provider-managed `Ok(())` path is removed. |
| M7 | Unix WSL2 launch rejection remains before provider command resolution and host-listener reservation. A possibly active Unix WSL2 stop returns the same named unavailable error and retains authority rather than reporting release. Idempotent stop of an already stopped/uninitialized record may remain a no-op. |
| M8 | The non-Unix stub's unavailable start and possibly active stop both return the canonical named provider error. Active stop preserves runtime/provider evidence, does not clear `last_error`, does not write `Stopped`, and performs no false provider effect. Stub SSH-port release returns success only when no runtime authority is retained; retained runtime evidence rejects. The same stub behavior tests run locally through a test-only module alias and compile in the real Windows module. |
| M9 | `MachineVmmBackend` remains the existing CLI-private two-adapter command/device effect seam and may retain its current gvproxy Unix-socket grammar. The machine provider facts and new connectivity values acquire no segment allocator, allocated segment, CIDR/super-net, cluster transport, `NetworkPlan`, registry-mutation, Netavark, policy, service-name, socket, or provider-effect authority; the VMM seam acquires none of the allocation, attachment, registration, policy, naming, or lifecycle authority it does not already own. |
| M10 | NNC4.3's normal container registration continues to reject machine-forwarder composition. NNC4.4 does not reuse the sandbox attachment registration, invent WSL2 ingress/forwarding evidence, or manufacture a provider ID. WIN2/WIN5 remain the owners for the real Windows adapter and NAT-versus-mirrored reachability proof. |
| M11 | Cross-target provider facts live with `nimbus-machine`; VMM/effect tests remain with `nimbus-cli`. Happy, edge, error, false-success, exact-diagnostic, target, and static-boundary tests pass. Workspace metadata still reports exactly `nimbus-network -> nimbus-core`; no dependency or authority cycle is added. |
| M12 | After all M1-M11 evidence is green, affected checks, strict Clippy, warning-denied rustdoc, format/diff checks, static verifier and self-test, docs gates, and exactly one candidate-frozen GPT-5.6 Sol/xhigh/fast structured review pass. Only an accepted executable-code defect may trigger one narrow correction review. |

## Source-Grounded Current Model

### Provider facts and callers

`crates/nimbus-machine/src/provider.rs` owns three provider identities and one
five-field record. Four production decisions interpret its networking boolean:

| Current decision | Owner | Risk |
| --- | --- | --- |
| Post-start forwarding | `nimbus-cli/src/machine/manager/readiness.rs` | Provider-managed mode silently returns `Ok(())`, even though no such launch plan is currently supported. |
| Host listener withdrawal | `nimbus-cli/src/machine/manager/stop.rs` | A negative boolean decides whether durable host authority is fenced. |
| API-forwarder stop | `nimbus-cli/src/machine/manager/stop.rs` | The same mode is reinterpreted independently. |
| gvproxy stop/release proof | `nimbus-cli/src/machine/manager/stop.rs` | The same mode is reinterpreted a third time. |

`MachineVmmBackend` is an earned CLI-private effect seam with real krunkit and
vfkit adapters. It owns VMM binary resolution, command construction, and the
gvproxy transport/device pairing. It does not own complete network attachment,
segment allocation, service publication, or sovereignty evidence.

The current macOS management endpoint proves only an IPv4 TCP loopback SSH
listener under a stable `ListenerId` and shared `PortLease`. An IP address is
observed bind evidence, never machine or workload identity.

### Corrected unavailable-provider evidence

The initial plan premise said unavailable WSL2 start and stop already failed
closed. The audit narrowed that claim:

- Unix `vmm_backend` rejects WSL2 before helper resolution, provider effects,
  and host-listener reservation.
- Unix `stop_provider_machine` rejects WSL2; the stop coordinator retains
  authority and returns error.
- The non-Unix stub start returns an error after recording failed state.
- The non-Unix stub stop currently changes any possibly active record to
  `Stopped`, clears `last_error`, writes the record, and returns success without
  a provider stop or inspection.
- The non-Unix stub SSH-port release currently returns success even when
  runtime evidence is retained.

The last two behaviors are false completion evidence, not an acceptable stub.
M8 owns their correction.

## Target Value Seam

The implementation should use a small concrete value composition, not another
provider interface:

```text
MachineProviderCapabilities
  network_management_mode: NetworkManagementMode
                  |
                  v
MachineConnectivityCapabilities
  attachment: NetworkAttachmentCapabilitySet
  exposures: ordered set<NetworkExposure>
  sovereignty: NetworkSovereigntyCapabilities
                  |
                  v deterministic comparison
MachineConnectivityRequirements
  attachment: NetworkAttachmentCapabilitySet
  exposures: ordered set<NetworkExposure>
  sovereignty: NetworkSovereigntyRequirements
                  |
                  v
MachineConnectivitySatisfactionError
  provider + ordered NetworkCapabilityMismatch values
```

The values may be publicly constructible for future source-owned adapters and
deterministic tests. They do not register themselves, select another provider,
or perform an effect. Availability is a separate typed result from
`MachineProvider`; WSL2's enum presence and `ProviderManaged` topology never
become capability evidence.

## Conservative Provider Matrix

| Provider | Static mode | Current capability evidence | Availability |
| --- | --- | --- | --- |
| Krunkit | `NimbusHostManaged` | macOS only: `VirtualMachineGuest`, workload-namespace-equivalent guest boundary, loopback, local-only, no external dependency, offline reconcile | Available only on macOS; runtime readiness remains effect-owned |
| Vfkit | `NimbusHostManaged` | Identical to krunkit, with a distinct provider identity and VMM adapter | Available only on macOS; runtime readiness remains effect-owned |
| WSL2 | `ProviderManaged` | None. NAT/mirrored exposure, forwarding, isolation, and readiness are not yet Nimbus evidence. | Typed unavailable on every current target |

`PortLeaseEffectScope::ProviderManaged` on the macOS gvproxy listener means an
out-of-process effect owns the socket bind. It does not change the machine
topology to `NetworkManagementMode::ProviderManaged`; a regression test pins
that semantic distinction.

## Failure Matrix

| Case | Required result |
| --- | --- |
| Exact host-managed requirements against krunkit/vfkit on macOS | Pass with no effect. |
| Provider-managed requirements against krunkit/vfkit | Typed management mismatch; attachment/isolation differences remain visible in stable order. |
| Required public/private exposure against loopback-only evidence | Typed exposure mismatch. |
| Required tenant segment or provider boundary against the guest boundary | Typed isolation mismatch. |
| Evidence broader than maximum admitted control-plane locality | Typed locality mismatch. |
| Evidence requires an unapproved external facility | Typed external-dependency mismatch. |
| Offline restart required but evidence cannot prove it | Typed offline-restart mismatch. |
| WSL2 capability request on any current target | Typed provider unavailable; no registration or fallback. |
| Krunkit/vfkit capability request off macOS | Typed provider unavailable; no false advertisement. |
| Provider-managed post-start enters the current host launch path | Canonical unavailable error, never `Ok(())`. |
| Possibly active WSL2 stop on Unix | Canonical unavailable error; retained authority. |
| Possibly active non-Unix stub stop | Canonical unavailable error; state/runtime unchanged and no state write. |
| Non-Unix release with retained runtime evidence | Error; no release attestation. |
| Already stopped/uninitialized stop | Idempotent `Ok(())` is allowed because no live effect is claimed stopped. |
| Normal container registration with machine forwarding | Existing typed refusal remains. |
| VMM adapter supplied as segment/attachment authority | Impossible by type and source scan. |

## Fail-Before Test Packet

Before production source edits:

1. Add a pure `nimbus-machine` integration test importing the missing typed
   connectivity requirements, capabilities, satisfaction error, and
   `network_management_mode`.
2. Assert the exact provider mode matrix, krunkit/vfkit equality, exact match,
   mutual mode rejection, and exposure/isolation/sovereignty mismatch order.
3. Assert WSL2 and off-target providers cannot return capability evidence.
4. Add CLI tests referencing exhaustive typed-mode lifecycle behavior, the
   provider-managed post-start rejection, and the non-Unix stub's active-stop
   and retained-runtime-release contract.
5. Run the focused missing-API tests before implementation. Expected red must
   be compiler errors naming only the absent typed contracts; existing
   unavailable-provider tests are green baseline evidence and must not be
   mislabeled expected red.

## Owned Paths

The admitted implementation may touch only:

- `crates/nimbus-machine/src/provider.rs`;
- one small concept-owned `crates/nimbus-machine/src/networking.rs`;
- `crates/nimbus-machine/src/lib.rs`;
- a focused `crates/nimbus-machine/tests/provider_networking.rs`;
- narrow typed-mode callers/tests under
  `crates/nimbus-cli/src/machine/manager/`;
- `crates/nimbus-cli/src/machine/stub/manager.rs`;
- a test-only non-Unix stub-module alias in
  `crates/nimbus-cli/src/machine/mod.rs`;
- directly related machine provider/stub tests;
- this proof, the canonical plan, and the plan routing index.

`nimbus-network` production source, sandbox/provider effects, server ingress,
compute, system projections, egress/proxy, services, cluster transport, and the
Windows provider implementation remain unchanged.

## Static Seam Checklist

The candidate must prove:

1. `nimbus-network` has exactly one outgoing workspace edge: `nimbus-core`.
2. `uses_provider_networking` has zero production or test matches.
3. `crates/nimbus-network` has zero `MachineProvider`, `Krunkit`, `Vfkit`, or
   `Wsl2` matches.
4. Machine provider/connectivity/VMM paths have zero segment allocator,
   allocated segment, CIDR/super-net, cluster transport, `NetworkPlan`,
   registry-mutation, Netavark, policy, naming, socket-bind, or network-provider
   authority. Existing VMM command construction may continue to name its
   gvproxy Unix-socket device grammar.
5. No WSL2 attachment or ingress provider registration exists.
6. No `MachineNetworkProvider`, `NetworkProvider`, or equivalent new trait is
   introduced.
7. The ordinary sandbox registration still refuses machine-forwarder mode.
8. Windows-plan WIN2/WIN5 remain linked as implementation/reachability owners.
9. New files remain below repository modularity thresholds and retain one
   concept-owned responsibility.

## Platform Proof

The owner host is macOS. Local tests can execute the macOS capability path and
the Unix lifecycle. The non-Unix stub module must also be compiled under a
test-only alias so its pure failure behavior executes locally; the same tests
compile inside the real non-Unix module under the Windows target. The existing
Windows workflow remains compile-only, so the proof must not claim a native
Windows execution result unless one is actually obtained.

NNC4.4 does not characterize WSL2 NAT or mirrored mode and does not claim
Windows localhost reachability. Those remain WIN5 acceptance work.

## Acceptance Results

| Criterion | Result | Evidence |
| --- | --- | --- |
| M1 | Pass | `uses_provider_networking` has zero repository matches; all three providers return the exact typed mode. |
| M2 | Pass | The 263-line machine-owned value module is concrete synchronous data plus deterministic comparison; effect and provider-trait scans are empty. |
| M3 | Pass | macOS alone returns krunkit/vfkit evidence; off-target Apple adapters and WSL2 return `ProviderUnavailable`. |
| M4 | Pass | The macOS test proves equal distinct-provider profiles, exact guest attachment/isolation, loopback exposure, local-only control, no external dependency, and offline restart. |
| M5 | Pass | Exact requirements pass; both substitution directions and the complete ordered isolation/exposure/sovereignty mismatch vector are pinned. |
| M6 | Pass | Readiness and stop exhaustively match `NetworkManagementMode`; provider-managed post-start fails with the named provider and no forward child. |
| M7 | Pass | Existing launch rejection precedes listener authority; stop rejects provider-managed mode before host cleanup. |
| M8 | Pass | The locally executed non-Unix alias proves named start rejection, no-mutating active stop, and retained-runtime release refusal 3/3. |
| M9 | Pass | Source scans find no allocator, segment, CIDR, plan, registry, Netavark, cluster, socket-bind, or provider authority in the provider/connectivity/VMM seam. |
| M10 | Pass | The target-independent sandbox matrix still rejects machine-forwarder registration 1/1; no WSL2 registration exists; WIN2/WIN5 remain linked owners. |
| M11 | Pass | Full affected behavior and quality gates are green; metadata reports exactly `nimbus-network -> nimbus-core`; new files remain below thresholds. |
| M12 | Pass | All required gates pass. Frozen staged tree `3cd1418c0175ebec3a117f8971df037f3b7831f2` received the one full Sol/xhigh/fast review with zero findings and no correction cycle. |

## Evidence Ledger

| Checkpoint | Evidence |
| --- | --- |
| Dirty-state recovery | NNC4.3 is durable at commit `f0d36a5bff4ff96dc4c1eb0ca668e391f81af4e4`, tree `5bfaa90f7d1f52ac07b73d35fd8bbd0d5193e074`. NNC4.4 initially owned only the staged plan/routing transition. |
| Read-only audits | Three bounded audits completed with zero edits: machine/VMM ownership, cross-crate substitution, and WSL2/non-Unix fail-closed behavior. |
| Audit correction | Unix WSL2 start/stop is fail closed, but non-Unix active stop and retained-runtime release currently report false success. M8 owns the correction. |
| Written contract | M1-M12, failure matrix, owned paths, fail-before packet, static checklist, and platform limits are frozen before source edits. |
| Expected red | `timeout 600 cargo test -p nimbus-machine --test provider_networking` exited 101 solely with E0432 for the three absent connectivity values and E0599 for the absent provider mode/capability methods. `timeout 600 cargo test -p nimbus-cli non_unix_ --lib` executed the test-only non-Unix stub module: 0 passed, 3 failed, 877 filtered because retained-runtime release returned `Ok`, active stop returned `Ok`, and start returned the generic unnamed-host error. `timeout 600 cargo test -p nimbus-cli provider_managed_post_start_networking_does_not_silently_succeed --lib` executed 0/1/879 and failed because the provider-managed branch returned `Ok(())`. |
| Focused behavior | The three exact red commands are green: machine connectivity 5/5; locally executed non-Unix stub 3/3 with 877 filtered; provider-managed post-start 1/1 with 879 filtered. Full `nimbus-machine --all-features` passes 21/21 across 16 unit plus 5 connectivity integration tests. CLI provider/bootstrap passes 19/19 with 861 filtered; stop/cleanup passes 4/4 with 876 filtered; provider selection passes 7/7 with 873 filtered. Exact requirements pass; both substitution directions reject; exposure/isolation/locality/external-dependency/offline mismatches preserve the pinned order. |
| Affected suites and quality gates | `cargo check -p nimbus-machine -p nimbus-cli --all-targets --all-features` passes. Full machine passes 21/21; full CLI passes 879 with 1 child-only ignore. Strict affected Clippy with `-D warnings` and warning-denied rustdoc pass; emitted warnings are confined to the existing vendored Brotli crates. Format and diff checks pass. |
| Provider/effect distinction | `machine_ssh_claim_precedes_provider_and_exact_observation_activates` passes 1/1 and proves that krunkit's topology is `NimbusHostManaged` while the out-of-process gvproxy bind retains `PortLeaseEffectScope::ProviderManaged`. |
| Static boundaries and docs | Metadata reports only `nimbus-network -> nimbus-core`. Legacy boolean, machine vocabulary in `nimbus-network`, new provider/registration traits, forbidden allocation/transport/effect references, connectivity-value effects, and WSL2 registrations each scan to zero. The connectivity value/test/provider/VMM/stub files are 263/246/137/285/324 lines, all below thresholds. Sandbox's target-independent machine-forwarder refusal passes 1/1. Docs pass 108 pages and site 17/17. |
| Platform boundary | The owner host is macOS. A real macOS-to-`x86_64-pc-windows-msvc` `cargo check` was attempted and stopped before Nimbus code in third-party `ring`/`libz-sys` C compilation because the host lacks Windows SDK C headers (`assert.h`, `sys/types.h`, `stdio.h`). This is not recorded as a Nimbus or Windows pass. The same non-Unix source executes locally through the alias; the existing Windows workflow remains the real-target compile owner. |
| Verifier recovery | The first live replay passed 14/15 and correctly rejected NNCV008 because the active ledger row lacked `Last green` and the recovery header contained two full hashes where exactly one commit hash is allowed. After repairing both fields, the live verifier passes 15/15 and the adversarial self-test passes 45/45. |
| Structured review | Command: `autoreview --mode local --engine codex --model gpt-5.6-sol --thinking xhigh --codex-speed fast` with NNC4.4/M1-M12 focus. Actual reviewer thread `019fab85-be1b-7030-a823-c61733826669` reviewed the complete 79,296-byte bundle in one pass, reported zero findings, and concluded `patch is correct` at 0.96. No finding was accepted or rejected; no executable correction and no narrow correction review are warranted. |
| Final candidate | Staged tree `3cd1418c0175ebec3a117f8971df037f3b7831f2`; staged diff SHA-256 `12cadbed16162fde8bd9a86032f6c4cdd948e9fe32f01d6403fc2020c6eed124`. The owning closeout commit records implementation, proof, and the transition to NNC4.5; no push or PR is authorized. |
