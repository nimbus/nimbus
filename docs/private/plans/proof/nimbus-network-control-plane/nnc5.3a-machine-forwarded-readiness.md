# NNC5.3a Complete Machine-Forwarded Readiness

Status: `complete; R1-R18 green; sole full and one narrow correction review dispositioned`

Owner: `NNC5.3a`

Starting commit: `5b1dd5b18bbdcf6d00374a7c5d8edef446530552`

Starting tree: `1f03dc4255bb51ba12b5872a1e6e05740de52540`

## Outcome boundary

NNC5.3a makes Container machine-forwarded attachment readiness an exact,
read-only composition of desired, durable, and observed evidence. A durable
`Exposed` receipt records a Nimbus-authorized operation whose exact native
effect was observed after mutation; it does not prove that the same provider
generation or forwarding route remains current.

The completed decision must compose:

- the NNC5.3 common attachment base: exact current durable attachment,
  Netavark/IPAM attempt and status, firewall/egress pin, and PEP evidence;
- the exact persisted machine-forwarder provider instance and generation;
- one complete, canonical, tenant- and sandbox-qualified durable `Exposed`
  receipt batch;
- one fresh adapter-translated current-forwarding observation from one bounded
  native gvproxy route-list response, authenticated under the same
  lifecycle-issued provider instance and generation;
- the exact process-local proxy registration, normalized route set, worker
  liveness, active listener leases, and non-cloneable process lifetimes; and
- one provider-neutral `NetworkObservation` emitted only after every facet is
  authenticated.

The machine provider remains an effect adapter in `nimbus-sandbox`.
`nimbus-network` gains no socket, HTTP, gvproxy, Netavark, nftables, PEP,
policy, service-name, cluster-transport, or machine-provider code.

## Read-only source and substitution audit

### Current false-ready call graph

```text
launch_manifest
  -> configure_network
     -> common OCI attach
     -> apply egress pin
     -> start exact local proxy workers and active listener lifetimes
     -> gvproxy /expose
     -> persist exact Exposed receipts
  -> start/authenticate PEP
  -> require_authenticated_egress_readiness
  -> require_complete_host_managed_attachment_readiness
     -> machine mode returns Ok without inspecting attachment or publication
  -> spawn tenant runtime

detect_runtime_status
  -> application readiness
  -> exact PEP readiness
  -> machine mode treats PEP readiness alone as network readiness
  -> Ready/endpoints may publish
```

### Existing authorities and target seam

| Concern | Existing owner | Audit result and target |
| --- | --- | --- |
| Desired machine mode | `ContainerRunnerExecutionConfig::network_publication_mode` plus `machine_port_forwarder` | The persisted mode is required independently of optional provider authority. `MachineForwarded` without the exact lifecycle-issued handle/generation and `HostManaged` carrying machine authority both fail closed, including for an empty publication set. Do not infer identity from host, port, IP, or path. |
| Durable provider operation | `runtime/machine_port_evidence.rs` | The strict sibling record already authenticates version, phase, tenant, sandbox, provider handle/generation, ordered binding batch, and outcomes. Reuse it as historical evidence only. |
| Current gvproxy forwarding | `oci/network/forwarding.rs` | The real gvproxy client contract is status-only `POST /expose`, status-only `POST /unexpose`, and read-only batch `GET /all`; it has no `/inspect`. Translate exactly one bounded native route list into a non-serializable Nimbus observation under the parent-issued provider handle/generation. Missing, wrong, duplicate, conflicting, malformed, oversized, timed-out, EOF, or refused evidence is provider-unknown. Never retry `/expose` as inspection. |
| Local route intent | `oci/network/proxy.rs::MachinePortProxyRoute` | The normalized guest listener, container target, and external publication address already exist. Compare the complete ordered route set; do not add a second route model. |
| Process-local worker/lifetime authority | `MachinePortProxyLifetimeRegistry` | `Running` registrations retain exact bindings, leases, routes, workers, live lifetime batch, and publication state. Add a read-only exact inspection seam; `Stopping`, recovered authority, dead workers/listeners, partial sets, or missing entries fail closed. |
| Active durable listener authority | `OciPortLeaseCoordinator` | Reuse `require_active_machine_bindings_with_lifetimes`; an `Active` lease without the exact live lifetime guard is not current reachability. |
| Common attachment base | `attachment_lifecycle/attachment_readiness.rs` | Refactor the NNC5.3 collector into one base plus honest host-managed and machine-forwarded publication completion. Do not duplicate durable attachment/IPAM/Netavark/pin/PEP checks in Container. |
| Pre-spawn consumer | `ContainerSandboxBackend::launch_manifest` | Replace the machine bypass with the complete mode-aware decision before runtime spawn. |
| Live-status consumer | `ContainerSandboxBackend::detect_runtime_status` | Replace the PEP-only machine branch with the same complete read-only decision. Existing status projection then withdraws/restores endpoints. |
| Inspection side effects | `inspect_sync -> maybe_restart_after_exit` | NNC5.6 still owns removal of restart effects. NNC5.3a may add only readiness inspection and must not expand restart, repair, cleanup, or reuse authority. |

### Provider observation contract

For a non-empty desired binding batch, the adapter performs exactly one
bounded native `GET /services/forwarder/all`. The native response contains
only gvproxy route fields:

```text
local = exact desired external publication
remote = exact guest listener publication
protocol = tcp
```

Each desired local/protocol slot must have exactly one route and its remote
must match exactly; missing, wrong, duplicate, or conflicting slot evidence
fails closed. The sandbox adapter translates that native list into a distinct
non-serializable current-observation type under the exact parent-issued
provider endpoint, handle, and generation already fenced by lifecycle
configuration. Nimbus identity/generation fields are never added to gvproxy
request or response shapes. A persisted operation receipt cannot be supplied
where a fresh current observation is required.

This contract is grounded in the official gvproxy client documentation and
source:

- [gvisor-tap-vsock client package](https://pkg.go.dev/github.com/containers/gvisor-tap-vsock/pkg/client)
- [v0.8.9 client implementation](https://github.com/containers/gvisor-tap-vsock/blob/v0.8.9/pkg/client/client.go)

An empty desired binding set has no forwarding resource to inspect. It still
requires the exact empty durable evidence record, exact empty live
registration, and the common attachment/PEP base, but it does not fabricate a
claim that gvproxy currently owns a nonexistent route.

## Binding design

### One base, two honest publication completions

The existing attachment collector is decomposed conceptually:

```text
inspect common base
  desired + durable attachment
  IPAM + attempt-bound Netavark status
  exact egress pin
  exact PEP
  -> base evidence (not yet Ready)

host-managed completion
  exact Netavark listener lifetimes
  -> portable Ready observation

machine-forwarded completion
  exact durable Exposed batch
  exact fresh provider observation
  exact local routes/workers/listener lifetimes
  -> portable Ready observation
```

Base evidence cannot be published as complete readiness. Wrong publication
mode is a named failure. The portable observation is constructed only by the
mode-specific completion.

### Desired, durable, and observed separation

| Layer | Owner | Required machine evidence |
| --- | --- | --- |
| Desired | persisted Container manifest and pure OCI attachment plan | stable tenant, sandbox, attachment, bindings, leases, route inputs, provider handle, provider generation |
| Durable | attachment authority, IPAM journal, port authority, machine evidence record | exact `Active` attachment, exact Netavark attempt/status, exact active listener bindings, complete ordered `Exposed` receipt batch |
| Observed | sandbox provider inspectors, process registry, PEP composer | exact pin, current typed gvproxy forwarding, normalized routes, live workers/listeners/lifetimes, current PEP |

No IP address, route, socket, filename, or receipt is workload identity.
Observed addresses are compared only under stable tenant-qualified identities
and fenced generations.

### Failure and recovery behavior

- Missing, false, stale, crossed, partial, malformed, or unknown evidence
  yields a named NotReady result and no portable Ready observation.
- Provider inspection performs no exposure, withdrawal, bind, start, stop,
  attachment transition, cleanup, release, or capacity reuse.
- Provider timeout, refusal, EOF, unsupported route-list response, or
  ambiguous reply
  preserves all desired and durable authority for a later observation.
- A dead local worker or lost listener lifetime remains fenced. Readiness does
  not restart or replace it; NNC5.4/NNC5.6/NNC8.3 retain their existing
  reconciliation ownership.
- Live status withdraws endpoints through the existing status projection.
  Exact restored evidence permits the next read-only inspection to restore
  readiness without rewriting desired state.

## Written acceptance criteria

| ID | Criterion | Verifiable success proof |
| --- | --- | --- |
| R1 | The historical false-ready is captured before production edits. | A machine pre-spawn case currently succeeds with no complete attachment/publication evidence, and a running-status case currently reports Ready from application+PEP alone; both fail at named assertions before the correction. |
| R2 | One common base owns attachment readiness. | Host-managed Container/Krun and machine-forwarded Container use the same desired/durable attachment, IPAM/Netavark status, pin, and PEP collector; source scans find no copied Container switchboard. |
| R3 | Publication modes are explicit. | The persisted runner configuration carries a required `HostManaged` or `MachineForwarded` mode independently of optional provider authority. Host-managed completion requires Netavark lifetimes; machine completion requires exact forwarder authority and machine evidence; crossed or missing authority returns a named failure even for zero bindings. Base evidence alone cannot emit Ready. |
| R4 | Persisted provider authority is exact. | Missing forwarder plus substituted provider instance/generation, tenant, sandbox, binding order, phase, outcome, and partial/duplicate receipt batches fail closed without mutation. |
| R5 | Current forwarding has a typed read-only provider seam. | One exact native `GET /all` batch passes. Unsupported status, missing/wrong/duplicate/conflicting local/remote/protocol routes, partial batch, oversized/malformed response, timeout, EOF, and refusal return provider-unknown and never call `/expose`; `/inspect` is absent. Parent-issued handle/generation remain adapter authority and never leak into the native wire shape. |
| R6 | Historical receipts are insufficient. | Exact persisted `Exposed` receipts with missing, stale, ambiguous, or unavailable current provider observation remain NotReady; a fresh exact observation is required for every non-empty binding. |
| R7 | Local route registration is exact. | Missing or `Stopping` registration and partial, reordered, duplicated, stale-target, wrong-external-address, wrong-binding, wrong-lease, or wrong-tenant/sandbox route registrations fail closed. |
| R8 | Listener lifetime authority is exact. | Every desired binding has one exact active MachinePortProxy binding and current live lifetime guard. Durable Active alone, recovered authority, wrong epoch/generation/lifetime/provider/address, and partial batches fail. |
| R9 | Every local provider worker is current. | Exact worker count and live listener ownership pass; dead/exited workers, missing workers, extra workers, or a registration with `publication_may_exist=false` fail closed. |
| R10 | Empty publication is explicit. | An exact empty desired/evidence/registration set passes the machine publication facet without provider I/O and publishes no endpoint; any non-empty sibling evidence conflicts. |
| R11 | The final observation is portable and fenced. | Ready contains the exact attachment `NetworkResourceVersion`, selected provider, `Active` observed phase, and `Ready=True`; no address/path/receipt becomes identity and any failed facet emits no Ready observation. |
| R12 | Pre-spawn consumes the complete decision. | Machine Execute cannot reach creator/runtime spawn until R2-R10 and the existing application-independent network prerequisites are green; the exact complete row reaches the existing spawn boundary once. |
| R13 | Live status and endpoints are truthful. | A live workload becomes NotReady and publishes zero endpoints when current forwarding, receipt, route, worker, lifetime, common-base, pin, or PEP evidence is lost; exact restoration permits application readiness to recover. |
| R14 | Inspection is read-only and byte-stable. | Recording provider substitutes, request census, registry snapshots, and authority/artifact byte snapshots prove zero exposure, withdrawal, bind, start, stop, transition, cleanup, release, finalization, or reuse across ready/not-ready/error inspection. |
| R15 | Failure and reopen semantics are deterministic. | Same-generation repeated inspection returns the same decision; fresh process without live registry fails closed; stale/future/equal-generation-different-provider evidence never normalizes into readiness. |
| R16 | Ownership and dependency boundaries remain exact. | Provider HTTP, sockets, Netavark, nft, proxy workers, and PEP remain in `nimbus-sandbox`; `nimbus-network -> nimbus-core` is still the sole workspace edge and NNC5.4/NNC5.6/NNC8.3 remain untouched owners. |
| R17 | Complete verification and review cadence pass. | Focused happy/edge/error/substitution tests, full affected suite, check, strict Clippy, warning-denied rustdoc, dependency/effect/verifier mutations, format/diff, and docs gates pass with exact counts; then exactly one GPT-5.6 Sol/xhigh/fast item review runs. Only an accepted material executable defect permits one narrow correction review. |
| R18 | Exact checkpoint is durable. | Code, tests, verifier, proof, routing, and recovery ledger commit together as NNC5.3a; no push or PR occurs. |

## Fail-before packet

Before changing production behavior:

1. add `nnc5_3a_machine_pre_spawn_rejects_missing_complete_readiness`, which
   proves the current machine bypass accepts no attachment, receipt, provider,
   route, worker, or lifetime evidence;
2. add `nnc5_3a_machine_live_status_rejects_pep_only_readiness`, which proves
   the current machine branch reports Ready from application+PEP while the
   common attachment and forwarding evidence are absent;
3. run those two tests together and record the exact `0/2` result and named
   assertions;
4. extend NNCV019 before candidate closeout so missing machine current
   inspection, receipt/registry composition, pre-spawn/status consumers, or
   read-only boundary each fail exclusively; and
5. add exact provider and registry substitution matrices before their
   production seams are considered complete.

### Confirmed expected red

Before any production source changed:

```text
timeout 1200 cargo nextest run -p nimbus-sandbox \
  -E 'test(/nnc5_3a_machine_(pre_spawn_rejects_missing_complete_readiness|live_status_rejects_pep_only_readiness)/)' \
  --no-fail-fast
```

Result: `0 passed; 2 failed; 880 skipped`, exit `100`.

- The pre-spawn gate returned success with no common attachment, receipt,
  current provider, route, worker, or listener-lifetime evidence.
- The live-status gate returned `Ready` from application plus PEP evidence
  alone.

Both failures occur at their named NNC5.3a assertions. Only the two
concept-owned tests, their module declaration, this proof, and the recovery
ledger differed from the NNC5.3 checkpoint. The emitted vendored Brotli
diagnostics are pre-existing and unrelated.

## Candidate implementation and verification

### Bound seams

- The OCI readiness owner now emits only a non-publishable common base until
  either host-managed Netavark lifetimes or machine-forwarded publication
  evidence completes it.
- `CurrentMachinePortForwardingObservation` is a distinct non-serializable
  type constructible only by the forwarding adapter after one exact native
  `GET /all` batch is translated under lifecycle-issued provider authority.
  `/inspect` does not exist and `/expose` is never an inspection fallback.
- `MachinePortProxyLifetimeRegistry`, the actual process-lifetime owner,
  composes the exact normalized routes, `Running` registration, binding and
  lease batches, `publication_may_exist`, live listener-lifetime authority,
  worker count/liveness, durable `Exposed` receipts, and fresh provider
  observation. It alone can construct the non-cloneable
  `MachineForwardedPublicationReadiness` proof.
- Machine completion consumes that proof and authenticates tenant, sandbox,
  provider handle, and generation before the common base can become a
  portable `Active`/`Ready=True` observation.
- Container pre-spawn and live status consume the same mode-complete decision.
  The previous machine bypass and PEP-only readiness path are gone.
- Persisted `ContainerNetworkPublicationMode` prevents absent machine
  authority—including for an empty desired set—from being reinterpreted as
  host-managed publication.
- The registry mutex remains held across the bounded read-only provider
  inspection so teardown cannot cross the checked generation before the
  observation is emitted. One native batch request uses one deadline, so the
  critical section is not multiplied by binding count. Provider effects,
  cleanup, repair, restart, and capacity reuse remain in their existing
  owners.

The same provider-protocol correction also aligns the adjacent existing
mutation paths with gvproxy: expose/unexpose bodies contain only native route
fields, mutation responses are status-only, and exact post-mutation `GET /all`
observation is what permits Nimbus to mint `Exposed`, `Withdrawn`, or
`ExactAlreadyAbsent` evidence. This is directly required to make the current
readiness proof truthful; it does not move effect ownership.

The R15 fresh-process substitution deterministically clones the configured
backend while replacing its process-local registry with a new empty registry;
it is not an operating-system subprocess. It proves that durable `Active`
state cannot replace the lost in-memory lifetime authority. Existing
real-process lifetime and lock suites in `nimbus-network` remain the proof
that process-owned guards do not survive owner death or reopen.

### Acceptance evidence and structured-review correction

| Proof | Result |
| --- | --- |
| Historical fail-before | Exact `0/2`, exit `100`, `880` skipped; the named pre-spawn bypass and PEP-only status assertions fail before production edits. |
| Full-review correction fail-before | After adding the two invariant-specific regressions and before correcting production, exact `0/2`, exit `100`, `896` skipped: the old adapter rejected the real native list because it expected invented `/inspect` evidence, and an empty machine-forwarded manifest with missing authority returned success through the host-managed branch. The same command passes `2/2` after correction. |
| Provider observation and mutation protocol | `12/12`, `884` skipped: native status-only expose/unexpose shapes, one native batch list, exact translation, missing/wrong/duplicate/conflicting/partial routes, malformed/oversized/status/timeout/EOF/refusal failures, ambiguous mutation settlement, inspect-before-retry, no `/expose` inspection fallback, and empty-set no-I/O. |
| Machine composition | `10/10`, `886` skipped after correction: exact complete evidence, historical-only rejection, explicit missing/crossed mode authority including zero bindings, exact empty set, registry/route/lease/worker substitutions, fresh registry loss, and endpoint withdrawal/restoration. |
| Provider cleanup and restart regression | `32/32`, `864` skipped: native status-only withdrawal plus exact batch absence observation remains compatible with cleanup and restart lifecycle behavior. |
| Durable receipt and publication mode | `8/8`, `888` skipped: seven strict machine-evidence tests plus host/machine completion crossing. The one transient nextest leak marker disappeared in an isolated `1/1` rerun. |
| Read-only state preservation | Exact success and provider-unknown tests preserve every regular desired/durable artifact byte-for-byte and preserve typed registry bindings, leases, routes, worker liveness, live claims/lifetimes, publication flag, and entry count. |
| Full affected Sandbox | `875/875`, `21` declared skips. An unrelated IPAM case in the pre-review run and an unrelated process-stat parser in the corrected run each received a transient nextest leak marker and then passed normally in isolation; the corrected isolated rerun is `1/1` with `895` skipped. There were zero failures. |
| Portable network regression | `235/235`, one declared subprocess skip. |
| Static contract | Live NNCV000-NNCV019 passes `20/20`. Correction adds five NNCV019 mutations that reject invented `/inspect`, per-binding deadlines, Nimbus fields in native route shapes, missing explicit persisted mode, and optional-authority mode inference; the enlarged aggregate mutation suite passes exactly `92/92`. |
| Affected quality | All-target/all-feature Sandbox check, strict no-deps Clippy with `-D warnings`, and warning-denied no-deps rustdoc pass. Only pre-existing vendored Brotli diagnostics appear outside the strict affected crate. |
| Dependency/effect boundary | `cargo metadata --format-version 1 --no-deps` reports `nimbus-core` as the sole `nimbus-network` workspace dependency; the live verifier rejects upper-crate, transport, cloud-SDK, provider-effect, duplicate-authority, and address-identity mutations. |
| Script and patch quality | Bash parse, Node syntax, ShellCheck with the aggregate script's documented SC2034/SC1091 exclusions, `cargo fmt --all --check`, and `git diff --check` pass. |
| Documentation | After correction, `scripts/check-docs.sh` passes `108` link-clean pages and `scripts/verify-nimbus-docs-site.sh` passes `17/17` conditions. |
| Sole full item review | GPT-5.6 Sol/xhigh/fast thread `019fb6bf-c23c-75b0-b6e3-a427db6ad3b6` reviewed frozen tree `8e3c86d30ad77ff1054816ab420ff3b8af0b8bbe` / executable SHA-256 `bbfd721b5b7251688d1f500fa1b0cd5ad5a5e445fba850d4cb0893055b5c59f4`. Confidence `0.97`; TruffleHog clean. Three material findings were accepted: invented gvproxy `/inspect`, empty-set mode reinterpretation, and per-binding deadline multiplication under the registry lock. No second full review is permitted or needed. |
| Corrected frozen identity | `31` exact owned paths; reviewed staged tree `044565f3cf20d201dda583e0aa726d2ed583a31c`; executable SHA-256 `5eff00940c96fc7f727cc4f57ae530e752e5332edec693e57a81ecab86fd6f17`. All behavioral, static, quality, dependency, script, format/diff, and documentation gates are green. |
| Sole narrow correction review | GPT-5.6 Sol/xhigh/fast thread `019fb710-06ff-7b40-814a-8cd58490eba7`; one pass, TruffleHog clean, zero findings, “patch is correct” at confidence `0.96`. It confirms explicit persisted mode across lifecycle paths, one native bounded batch observation under the registry lock, no Nimbus authority in gvproxy wire shapes, and exact post-mutation evidence before receipt minting. No further NNC5.3a review is warranted. |

## Owned paths

The final item scope is limited to:

- `crates/nimbus-sandbox/src/backends/container/runtime/attachment_readiness.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/attachment_readiness.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/attachment_readiness.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/forwarding.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/forwarding/receipt.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/process.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/process/machine_proxy_lifetime.rs`
- `crates/nimbus-sandbox/src/backends/oci/network.rs`
- `crates/nimbus-sandbox/src/backends/oci/egress/readiness.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/execution_cleanup.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/lifecycle.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/manifest.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/provider_context.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/restart.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/runner/recovery.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_evidence.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_port_evidence/tests.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/machine_ports.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/attachment_readiness.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/machine_forwarded_readiness.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup.rs`
- `crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/forwarder_observer.rs`
- `crates/nimbus-sandbox/src/backends/oci/network/dto.rs`
- `scripts/verify-nimbus-network-attachment-readiness.mjs`
- `scripts/nimbus-network-control-plane/attachment-readiness-contract.sh`
- `scripts/verify-nimbus-network-control-plane.sh`
- `docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json`
- `docs/private/plans/proof/nimbus-network-control-plane/nnc4.6e-machine-network-authority-realms.md`
- this proof and the canonical plan/recovery ledger.

## Non-goals

NNC5.3a does not:

- retry `/expose` to infer current state, invent `/inspect`, or add a mutating
  readiness probe;
- implement provider cleanup, release, finalization, capacity reuse, or
  listener replacement;
- remove existing `inspect_sync` restart side effects owned by NNC5.6;
- move gvproxy, HTTP, socket, proxy, Netavark, nftables, IPAM, PEP, or policy
  behavior into `nimbus-network`;
- introduce a general `NetworkProvider`;
- change PDP/PEP, ingress certificate/interception CA, logical service naming,
  DNS, cluster transport, machine-provider capability, compute saga, or
  `nimbus-system` projection ownership;
- use an IP address, socket, route, filename, or receipt as workload identity;
  or
- push, open a PR, alter the original dirty checkout, or begin NNC5.4.

## Acceptance ledger

| Criterion | Status | Evidence |
| --- | --- | --- |
| R1 | `green` | Before production edits, the two named regressions failed exactly `0/2`, with the pre-spawn bypass and PEP-only live status captured at their invariant-specific assertions. |
| R2 | `green` | Host-managed Container/Krun and machine-forwarded Container share one attachment/IPAM/Netavark/pin/PEP base; the Container composition root delegates to its concept-owned child. |
| R3 | `green` | Required persisted `HostManaged`/`MachineForwarded` mode is independent of optional authority; host completion requires Netavark lifetimes, machine completion requires exact authority plus the registry-owned proof, and crossed/missing authority—including zero bindings—fails closed. |
| R4 | `green` | Missing forwarder and every receipt version/phase/tenant/sandbox/provider/generation/binding/order/outcome/partial/duplicate substitution fail closed. |
| R5 | `green` | The provider suite passes `12/12`; one native `GET /all` is translated under lifecycle-issued authority, every named missing/wrong/duplicate/conflicting/malformed/partial/unavailable/transport case is unknown, native shapes exclude Nimbus authority fields, `/inspect` is absent, and `/expose` is never inspection. |
| R6 | `green` | Exact durable `Exposed` receipts remain NotReady when current provider inspection is unavailable; bytes and receipts remain unchanged. |
| R7 | `green` | The 15-case registry matrix rejects missing/stopping/crossed and partial/duplicate/reordered/stale route, binding, lease, identity, and address evidence before provider I/O. |
| R8 | `green` | Only exact `Live` listener authority satisfies readiness; missing/recovered or inexact active lifetime evidence fails through the shared lease coordinator. |
| R9 | `green` | Exact worker count and double-checked listener ownership pass; missing, extra, dead, or unpublished workers fail closed. |
| R10 | `green` | Exact empty desired/durable/registry evidence passes with zero provider I/O and zero endpoint publication. |
| R11 | `green` | The final observation retains the exact attachment version and selected provider with `Active` and `Ready=True`; no failed facet emits Ready. |
| R12 | `green` | The historical pre-spawn bypass is corrected and the static consumer contract requires the mode-complete gate before the existing spawn boundary. |
| R13 | `green` | Live status transitions Ready → NotReady/zero endpoints → Ready when one exact native route-list observation is present, unavailable, then restored. |
| R14 | `green` | Success and provider-unknown paths preserve desired/durable bytes plus exact process registry, route, worker, claim, lifetime, and publication snapshots; static effect mutations fail exclusively. |
| R15 | `green` | Repeated exact inspection is deterministic; a new empty process registry, stale/future/crossed provider evidence, and same-generation substitutions fail closed. The deterministic registry replacement is explicitly not claimed as an OS subprocess. |
| R16 | `green` | Effects remain in Sandbox, `nimbus-network` retains only the `nimbus-core` workspace edge, and NNC5.4/NNC5.6/NNC8.3 authorities are unchanged. |
| R17 | `green` | The sole full review is complete and its three accepted executable findings are corrected. Focused correction suites, Sandbox `875/875`, live verifier `20/20`, mutations `92/92`, affected check/strict Clippy/rustdoc, core-only edge, script/format/diff, and docs `108` plus site `17/17` are green. The one narrow Sol/xhigh/fast correction review is clean at `0.96`; no further review is warranted. |
| R18 | `green` | The `31` exact owned paths, this proof, and the canonical recovery ledger are staged as one NNC5.3a checkpoint under reviewed tree `044565f3cf20d201dda583e0aa726d2ed583a31c` and executable SHA-256 `5eff00940c96fc7f727cc4f57ae530e752e5332edec693e57a81ecab86fd6f17`. The commit containing this row makes the owner worktree clean; no push or PR occurs. |
