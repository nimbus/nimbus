# NNC5.3 Complete Host-Managed Attachment Readiness

Status: `complete; R1-R18 green`

Owner: `NNC5.3`

Starting commit: `fc4827b06c672fae7b5f68c9e718100cec3ba83b`

Starting tree: `25a7acfb8c9a5bf63f302cd9ac8563266fb7a92d`

## Outcome boundary

NNC5.3 makes host-managed Container and Krun attachment readiness an exact,
read-only composition of desired, durable, and observed evidence. A namespace
path is one provider artifact, never readiness authority.

The item covers the common host-managed route:

- exact current durable attachment identity, generation, digest, epoch,
  selected provider, association, stable handle, and `Active` phase;
- exact live IPAM authority and a completed Netavark setup attempt;
- present, inspectable namespace and Netavark status artifacts;
- the expected deny-by-default firewall/egress pin observed in the exact
  namespace;
- exact active Netavark listener publication lifetimes, or an explicit empty
  publication set;
- the existing exact PEP listener, lifetime, worker, audit, policy, and reload
  evidence; and
- one provider-neutral `NetworkObservation` whose `Ready=True` value carries
  the exact current attachment version and selected provider.

Container machine-forwarded publication is a distinct provider composition
with gvproxy receipts and process-local proxy workers. It is prospectively
split into acceptance-bearing NNC5.3a before either item changes executable
code. NNC5.3 does not silently treat machine-forwarded evidence as Netavark
evidence.

## Read-only source audit

### Historical fail-before call graph

```text
configure_network
  -> OciAttachmentLifecycle::attach
     -> prepare_attach
        -> Active + Present => AlreadyActive
     -> return assigned IPs immediately

skipped on AlreadyActive:
  - exact listener lifetime validation/recovery
  - egress pin application
  - backend publication/forwarding
  - process-local lifetime registration
  - attachment hold confirmation

runtime inspection
  -> workload/application readiness
  -> exact PEP readiness
  -> Ready
```

### Authority and complexity map

| Concern | Current source | Audit result |
| --- | --- | --- |
| Portable desired attachment | `backends/oci/network/attachment_lifecycle/plan.rs` | One pure compiler owns plan ID, generation, digest, provider requirements, and stable handle. Reuse it. |
| Durable attachment authority | `nimbus-network/src/attachment_state.rs` | Exact tenant, resource version, association, provider, handle, and phase already exist. No second readiness store is needed. |
| Provider inspection | `attachment_lifecycle/recovery.rs::inspect_provider` | Exact IPAM and Netavark-attempt evidence are read, but `Ready + namespace` reports `Present` without requiring the status projection. |
| Active recovery | `attachment_lifecycle/recovery.rs::prepare_attach` | `Active + Present` returns `AlreadyActive`; the lifecycle exits before every post-Netavark readiness condition is revalidated. |
| Netavark listener lifetimes | `port_lifecycle/netavark_lifetime.rs` | One process-local registry owns the non-cloneable lifetime batch, but it has no read-only exact inspection seam. Fresh-process recovery currently enters cleanup authority; the portable authority already supports exact provider-managed binding reclaim after provider inspection. |
| Firewall/egress pin | `network/egress_pin.rs` | Installation is fail-closed, but no read-only provider observation proves the exact table/chain/rules still exist. Path existence is incorrectly used as a control-flow proxy in Krun. |
| PEP | `oci/egress/readiness.rs` | The exact composer already authenticates tenant/listener identity, lease, binding, lifetime, worker, audit, policy bytes, and reload attempt. Compose it; do not duplicate it. |
| Container status | `container/runtime.rs::detect_runtime_status` | Combines application and PEP readiness, but never reads durable attachment, IPAM, Netavark, status, pin, or forwarding evidence. It may repair a missing PEP; NNC5.6 remains the owner of eliminating inspection side effects. |
| Krun launch | `krun/vm/lifecycle.rs::ensure_execute_egress_enforced` | Requires Linux, `netns_path.exists()`, and exact PEP readiness. The test helper is even coarser: path plus coarse PEP readiness. |
| Krun status | `krun/vm/lifecycle.rs::running_status_with_egress` | Combines application and PEP readiness only. |
| Endpoint publication | both backend readiness modules | Correctly withdraws endpoints when final sandbox status is not `Ready`; the defect is the incomplete upstream readiness decision. |
| Machine-forwarded publication | Container runtime and `network/{forwarding,proxy}.rs` | Uses a different listener provider, persisted gvproxy receipts, and process-local proxy workers. NNC5.3a owns its end-to-end readiness. |

### Confirmed expected red

The preserved NNC0.6 regressions were run without changing source:

```text
timeout 1200 cargo nextest run -p nimbus-sandbox \
  --run-ignored ignored-only \
  -E 'test(/nnc0_6_(container_is_not_ready_at_partial_attachment_boundary|krun_rejects_netns_path_without_complete_attachment_evidence)/)'
```

Result: `0 passed; 2 failed`.

- Container returned `Ready` for workload liveness with only a namespace
  artifact and no Netavark status or complete attachment evidence.
- Krun returned `Ok(())` for a namespace artifact plus a ready PEP while
  Netavark status and egress-pin evidence were absent.

Both failures occur at their named NNCF6 assertions. The vendored Brotli
warnings are pre-existing and unrelated.

NNCV019 is also an exact expected-red gate:

- the live aggregate verifier exits `1` with NNCV000-NNCV018 green `19/19`
  and NNCV019 alone red;
- `missing-common-module`, `missing-container-consumer`,
  `missing-krun-consumer`, `missing-pin-inspection`,
  `missing-active-reconciliation`, and `readiness-effect-capability` each fail
  exclusively as NNCV019; and
- Bash parse, Node syntax, and ShellCheck pass.

## Binding design

### One sandbox-owned read model

Add one concept-owned `attachment_readiness` module beside the shared
attachment lifecycle. It is an application adapter, not a provider god trait.
It collects each authority once, classifies once, and returns:

```text
OciAttachmentReadinessState
  Ready(OciAttachmentReadinessEvidence)
  NotReady(OciAttachmentReadinessFailure)
```

Failures are a closed, named set for missing, false, conflicting, stale, and
unknown evidence. The ready value contains the exact portable
`NetworkObservation`, not a free boolean. The provider-neutral observation is
`Ready=True` only after every required sandbox-private facet is true.

The read model performs no provider effect, PEP start/reload, port mutation,
attachment transition, allocator mutation, cleanup, release, finalization,
capacity reuse, workload restart, or endpoint publication.

### Desired, durable, and observed separation

| Layer | Owner | Required evidence |
| --- | --- | --- |
| Desired | existing pure OCI attachment plan compiler | exact plan ID, generation, digest, selected provider requirement |
| Durable | `LocalNetworkAttachmentAuthority`, IPAM journal, port leases | exact attachment version/association/handle/phase, provider attempt, listener identity/lifetime |
| Observed | sandbox provider inspectors and PEP composer | namespace/status presence, Netavark/IPAM agreement, pin rules, live listener lifetime, PEP health |

No IP address, filename, path, or socket address becomes workload identity.
Paths and addresses remain provider observations checked against stable
tenant-qualified identity.

### Small provider capabilities

Keep provider effects in `nimbus-sandbox` and introduce only the capability
needed for real substitution:

```text
OciEgressPinProvider
  apply(exact namespace + expected PEP assignment)
  inspect(exact namespace + expected PEP assignment)
```

The real adapter owns `nsenter`/`nft` invocation and fails closed on absent,
malformed, conflicting, or uninspectable rules. Deterministic substitutes
exercise true, false, unknown, and substituted evidence without Linux or
privilege. The trait does not enter `nimbus-network`, and it cannot create,
delete, allocate, forward, or evaluate policy.

Netavark/IPAM inspection remains in the existing OCI provider adapter. PEP
inspection remains in the existing egress composer. Listener inspection is a
read-only view over the existing port authority and retained process-lifetime
registry.

### Reconciliation versus readiness

Readiness remains read-only. The launch/restart attachment lifecycle may,
before the readiness gate:

1. inspect the exact surviving Netavark effect;
2. acquire dead-owner recovery authority;
3. reclaim the unchanged provider-managed binding at a higher process
   lifetime generation;
4. retain the new non-cloneable lifetime batch;
5. idempotently apply the exact egress pin; and
6. preserve the existing `Active` attachment version.

That mutation path is inspect-before-reclaim and remains inside the existing
attachment lifecycle. It never recreates Netavark from presence, changes
desired identity, releases capacity, or claims cleanup convergence.

An `Active` attachment may no longer return before process-local publication
and pin reconciliation. If exact presence cannot be authenticated, the launch
fails closed with existing authority preserved for NNC5.4/NNC8.3
reconciliation.

### Consumers

- Container and Krun pre-runtime gates require the same complete attachment
  state after PEP activation and before workload/provider launch.
- Running status composes application readiness with the same read-only
  attachment result.
- `NotReady` and inspection errors withdraw published endpoints through the
  existing status path.
- Service naming/binding remains in `nimbus-services`.
- NNC5.6 still removes restart/repair authority from workload inspection; this
  item must not expand it.

## Written acceptance criteria

| ID | Criterion | Verifiable success proof |
| --- | --- | --- |
| R1 | Exact fail-before is preserved. | The two NNC0.6 regressions fail `0/2` at their named false-ready assertions before production edits; a new static verifier condition fails only for missing complete-readiness seams. |
| R2 | One common read model owns the decision. | Container and Krun call the same concept-owned collector/classifier; source scans find no backend-local duplicate complete-readiness switchboard. |
| R3 | Desired and durable identity are exact. | Missing authority plus wrong tenant, attachment, plan ID, generation, digest, epoch, association, selected provider, stable handle, and non-`Active` phase each return a named not-ready result and preserve bytes. |
| R4 | Netavark and IPAM evidence are complete. | Only exact live IPAM, exact `Ready` provider attempt, assigned-IP agreement, and present regular namespace/status artifacts pass; absent, stale, malformed, conflicting, and unknown substitutions fail. |
| R5 | Firewall/pin evidence is actual and exact. | The real provider observation requires the expected default-drop output chain and exact own-PEP permit; absent table/chain/rule, allow-all policy, sibling port, wrong namespace, command failure, malformed output, and unknown fail closed. |
| R6 | Host listener publication is exact. | Every desired binding has one exact active Netavark binding and current retained lifetime; empty desired bindings pass explicitly. Partial, duplicate, wrong provider/address/lease/generation/epoch/lifetime, missing registry, and dead owner fail. |
| R7 | PEP evidence is composed, not duplicated. | Existing exact PEP readiness passes; missing assignment/registration, stale policy/reload, wrong lease/provider/address/lifetime, dead worker, and unhealthy audit map to named attachment-not-ready evidence. |
| R8 | The final observation is portable and fenced. | `Ready=True` contains the exact attachment `NetworkResourceVersion`, selected provider, and `Active` observed phase; no IP/path/address is identity. Any facet false/unknown yields no ready observation. |
| R9 | Active restart revalidates process-local state. | Exact provider presence plus dead process lifetimes reclaims the same bindings at a higher lifetime generation and re-applies the exact pin without a second Netavark setup; live-owner, stale, or substituted evidence performs no mutation. |
| R10 | Launch is fail-closed in both backends. | Container and Krun cannot reach runtime/provider spawn from each incomplete facet; exact complete evidence reaches the existing spawn boundary once. |
| R11 | Running status and endpoints are truthful. | A live workload becomes `NotReady` and publishes zero endpoints when any attachment facet is lost; restoring exact evidence permits existing application readiness to recover. |
| R12 | Readiness inspection has no effects. | Recording substitutes and source scans prove zero provider, port, attachment, allocator, PEP, cleanup, release, finalization, reuse, restart, or endpoint mutations across ready/not-ready/error inspection. |
| R13 | Crash/reopen and substitution are deterministic. | Same-generation reopen produces the same result; stale/future/equal-generation-different-digest, dropped status, lost pin, lost lifetime, and lost PEP never normalize into readiness. |
| R14 | Provider modes stay honest. | Host-managed Container/Krun pass through NNC5.3; machine-forwarded Container remains explicitly routed to NNC5.3a and cannot be mistaken for Netavark publication evidence. |
| R15 | Boundaries remain exact. | `nimbus-network -> nimbus-core` remains the sole workspace edge and has no sockets, provider binaries, policy, proxy, Netavark, nft, gvproxy, transport, or cloud imports; cleanup/reuse remains NNC8.3-owned. |
| R16 | Complete affected gates pass. | Focused happy/edge/error/substitution/reopen tests, full affected suites, all-target/all-feature check, strict Clippy, warning-denied rustdoc, dependency/effect/census/verifier checks, format/diff, and docs gates pass with exact counts. |
| R17 | Item-level review cadence is honored. | Exactly one GPT-5.6 Sol/xhigh/fast full review runs only after R1-R16 and the complete frozen item are green; only an accepted material executable defect permits one narrow correction review. |
| R18 | Exact checkpoint is durable. | Code, tests, verifier, proof, routing, and recovery ledger commit together; no push or PR occurs. |

## Implemented result

One sandbox-private collector now owns the host-managed Container/Krun
readiness decision. It authenticates the exact desired plan, durable
attachment record and association, Netavark/IPAM attempt and artifacts,
egress pin, retained listener lifetimes, and existing PEP readiness before it
constructs a portable `NetworkObservation`. The collector has read authority
only. Container and Krun both invoke it at their pre-spawn boundary and from
their running-status path.

The launch lifecycle separately reconciles an `Active` attachment before
readiness inspection. It authenticates the surviving provider effect, reclaims
only exact dead-owner listener bindings at a higher lifetime generation,
retains the replacement batch, and reapplies the exact pin without a second
Netavark setup or attachment-version change. Readiness inspection itself
cannot call this mutation seam.

The real egress-pin adapter now has a small inspect capability beside its
existing apply capability. Inspection executes `nft -j -nn list table` inside
the exact namespace and structurally requires one active flag-free table, one
expected IPv4/IPv6 default-drop output chain, one loopback allowance, one
real anonymous-set established-flow allowance, and one exact assigned-PEP
destination allowance. Chain-only evidence, dormant tables, duplicate or
unknown executable rules, hidden allow policies, jump verdicts, missing rules,
sibling ports, malformed JSON, command failures, and substituted namespaces
fail closed.

Netavark status is an attempt-bound observed projection rather than arbitrary
provider JSON. The strict envelope authenticates schema, tenant, attachment,
the exact durable setup attempt, and assigned addresses. Empty JSON, foreign
exact projections, wrong schemas, wrong addresses, and unknown fields cannot
substitute for current readiness evidence.

The coherent test groups were moved out of two composition roots:

- `attachment_lifecycle.rs` is 1,485 lines; its 102-line Active reconciliation
  child owns both Active authority authentication and that mutation seam.
- Container `runtime/lifecycle.rs` is 1,982 lines; its 234-line
  `tests/attachment_readiness.rs` child owns the NNC5.3 integration proofs.

No machine-forwarded gvproxy behavior changed. That provider mode remains the
acceptance-bearing NNC5.3a item.

## Verification evidence

| Gate | Candidate result |
| --- | --- |
| Preserved fail-before | Before production edits, the two named NNC0.6 regressions failed exactly `0/2` at their false-ready assertions. They are no longer ignored and pass exactly `2/2` against the implemented production seam. |
| Common read-model matrix | Ten tests cover the exact ready observation, pin false/unknown/missing assignment, PEP failure, lost provider artifacts, lost listener lifetime, malformed/non-regular artifacts, explicit empty listeners, machine-mode rejection, missing/wrong durable phase, and every tenant/attachment/plan/generation/digest/epoch/association/provider/handle substitution with byte preservation. The attempt-bound status matrix additionally rejects empty JSON, a foreign exact projection, an unsupported schema, wrong assigned addresses, and unknown fields before exact-byte restoration returns Ready. |
| Pin provider | `10/10` tests pass for rendered IPv4/IPv6 policy, real anonymous-set JSON, active-table evidence, chain-only/dormant rejection, own-PEP exactness, sibling-port rejection, non-IP rejection, absent/malformed/unknown command and namespace results, and deterministic observation substitution. |
| Listener lifetime and Active recovery | `5/5` listener-lifetime tests plus the common Active-restart proof pass. A conflicting retained batch is not suppressed when the desired set is empty. Exact retained FreshLaunch and claim-free RestartRetained authorities succeed; a foreign launch claim fails before pin or provider effects. Dead-owner bindings are reclaimed once at a higher lifetime generation and the exact pin is reapplied; live-owner/substituted evidence is byte-preserving and effect-free; repeated reconciliation is idempotent and performs no second Netavark setup. |
| Backend launch/status behavior | The combined exact readiness/pin/lifetime selection passes `30/30`; the Netavark/provider-operation lane passes `47/47`. Container and Krun both reach their existing spawn boundary only with complete current evidence. A live workload withdraws `Ready` and every endpoint when Netavark status is removed, restores both from the exact bytes, and Krun also withdraws on exact PEP loss. |
| Crash/reopen determinism | Container and Krun same-generation reopen return the same complete readiness result. Lost status, pin, listener lifetime, or PEP and stale/future/equal-generation-different-digest substitutions never normalize into readiness. |
| Review-correction regression | The four accepted full-review defects first fail exactly `0/4`, then pass `4/4`: structurally hidden nft allow/jump authority, foreign Active FreshLaunch authority, syntactic or cross-attempt Netavark status, and a conflicting retained empty listener batch. The narrow review's real nft-shape and dormant-table findings fail `0/2`, then pass `2/2`. |
| Full affected suite | `timeout 1200 cargo nextest run -p nimbus-sandbox` passes `859` tests with `21` declared skips and zero failures. |
| Portable regression suite | `cargo nextest run -p nimbus-network --no-capture` passes `235/235` with one declared subprocess skip. |
| Affected quality | `cargo check -p nimbus-sandbox --all-targets --all-features`, strict no-deps Clippy with `-D warnings`, and warning-denied no-deps rustdoc all pass. Only the existing vendored Brotli diagnostics appear outside the strict affected crate. |
| Dependency/effect boundary | `cargo metadata --format-version 1 --no-deps` reports `nimbus-core` as the sole `nimbus-network` workspace dependency. The live aggregate verifier passes NNCV000-NNCV019 `20/20`; its adversarial self-test passes `78/78`, including all six NNCV019 mutations. |
| Script and patch quality | Bash parse and Node syntax pass. ShellCheck passes with only the aggregate script's documented pre-existing SC2034/SC1091 exclusions. `cargo fmt --all --check` and `git diff --check` pass. |
| Documentation | `scripts/check-docs.sh` passes `108` pages and `scripts/verify-nimbus-docs-site.sh` passes `17/17` conditions. |

## Structured review disposition

The one full item review ran only after R1-R16 and the complete NNC5.3
candidate were green:

- reviewer: GPT-5.6 Sol;
- reasoning/service: `xhigh`, fast;
- thread: `019fb601-90ee-7313-bb1a-a2877c01da02`;
- overall confidence: `0.97`; and
- scope: the complete NNC5.3 item and R1-R18, not an implementation chunk.

The raw structured result contained four concrete findings below the helper's
default P0 display threshold. The owner inspected and accepted all four:

| Finding | Disposition | Correction and proof |
| --- | --- | --- |
| P2 `0.98`: substring-based nft inspection could accept a hidden allow policy or jump verdict. | `accepted` | Inspect exact `nft -j -nn` structure and reject duplicates, substitutions, and unknown executable rules. The exact regression is red before the correction and green inside `4/4`; the complete pin/readiness lane is `28/28`. |
| P2 `0.96`: an `Active` attachment reconciled effects before authenticating the supplied attach authority. | `accepted` | Authenticate exact retained FreshLaunch or claim-free RestartRetained authority before reconciliation. Foreign authority now fails before pin/provider effects; exact same-generation replay and restart recovery remain idempotent. |
| P2 `0.93`: any syntactically valid Netavark status JSON could authenticate a current provider attempt. | `accepted` | Persist and strictly authenticate the attempt-bound status envelope. Empty, foreign, wrong-schema, wrong-address, and unknown-field projections all return NotReady; restoring the exact bytes returns Ready. |
| P3 `0.97`: empty-listener reconciliation suppressed every registry inspection error. | `accepted` | Propagate retained-batch and inspection conflicts even when desired bindings are empty. The exact conflicting-empty regression is red before the correction and green afterward while preserving the batch. |

Because these findings materially changed executable code, cadence permitted
exactly one narrow correction review after all corrected proofs were green.
It ran with GPT-5.6 Sol, `xhigh`, fast:

- thread: `019fb62f-556a-79b0-8c37-bf95fb7fde68`;
- overall confidence: `0.99`; and
- frozen executable SHA-256:
  `5cc3b95cb983a6373395666e93ee4379b39ba70835511bc7a4ab1f5b3c0c516d`.

The narrow review accepted two defects in the nft correction:

| Finding | Disposition | Correction and proof |
| --- | --- | --- |
| P1 `0.99`: the matcher used a bare array while real nft JSON wraps an anonymous connection-state set in `{"set": [...]}`. | `accepted` | The real-shape test fails before correction and passes after it; the matcher now requires the anonymous-set envelope, and the final readiness lane passes `30/30`. |
| P2 `0.96`: chain-only inspection could not prove that the containing table was active, so a dormant table could appear ready. | `accepted` | Inspection now lists the whole table and requires one exact table with no active flags. Chain-only and `flags=["dormant"]` evidence fail closed. The two narrow regressions are red `0/2`, corrected `2/2`. |

The final executable SHA-256 is
`9cc7cf33173f493ebcf152bff5d0714df8a272f432df65425a391d6b6483590d`.
Focused `30/30`, full sandbox `859/859` with 21 declared skips, affected
check, strict Clippy, warning-denied rustdoc, format/diff, and boundary gates
pass after the corrections. Per cadence, no third review ran or is warranted.

## Acceptance ledger

| Criterion | Status | Evidence |
| --- | --- | --- |
| R1 | `green` | Historical false-ready is preserved at `0/2`; the production regression is now `2/2`; NNCV019 was exclusively red and all six mutations now fail closed inside a `78/78` aggregate self-test. |
| R2 | `green` | Both backends call `attachment_readiness::inspect_host_managed_readiness`; NNCV019 proves the common module and each consumer are mandatory. |
| R3 | `green` | The substitution matrices cover missing authority and every named stable identity/version/association/handle field, with durable and allocator bytes unchanged. |
| R4 | `green` | Exact IPAM/provider attempts, IP agreement, namespace/status regularity, malformed status, loss, empty/foreign/wrong-schema/wrong-address/unknown-field projections, and unknown/error evidence are covered by the common matrix and full lifecycle suite. |
| R5 | `green` | Ten real/deterministic pin tests structurally cover the active flag-free table, real anonymous-set shape, exact default-drop/own-PEP contract, chain-only/dormant evidence, hidden allow policy, jump/extra verdicts, missing/duplicate/substituted rules, and every named failure class. |
| R6 | `green` | Five exact listener-lifetime tests cover empty, conflicting retained empty, partial, duplicate/substituted, missing/dead-owner, live-owner, and higher-generation recovery behavior. |
| R7 | `green` | The common collector composes the existing exact PEP readiness value; common/backend tests cover missing assignment, lost registration/worker, unhealthy/stale evidence, and endpoint withdrawal without duplicating PEP policy. |
| R8 | `green` | The ready matrix asserts the exact `NetworkResourceVersion`, provider, `Active` phase, and `Ready=True`; non-ready rows emit no observation and no address/path becomes identity. |
| R9 | `green` | Exact retained FreshLaunch and claim-free RestartRetained authorities authenticate before effects; a foreign claim is rejected. Active restart reclaims once, raises only the lifetime generation, reapplies the exact pin, preserves attachment bytes/version, and records zero second Netavark setup; live/substituted evidence does not mutate. |
| R10 | `green` | Both concrete pre-spawn gates delegate to the common collector; incomplete matrices stop there, while complete evidence reaches each existing spawn boundary once. |
| R11 | `green` | Live Container/Krun tests prove loss and exact restoration of attachment evidence withdraws/restores status and endpoints; Krun also proves PEP-loss withdrawal. |
| R12 | `green` | Recording substitutes, allocator-operation assertions, byte snapshots, and NNCV019's effect-capability mutation prove the read model cannot mutate providers, ports, attachment, allocator, PEP, cleanup, reuse, restart, or endpoints. |
| R13 | `green` | Same-generation reopen and every named lost/stale/future/substituted facet are deterministic and fail closed. |
| R14 | `green` | Host-managed Container/Krun are green; machine-forwarded publication returns `UnsupportedPublicationMode` and remains solely NNC5.3a-owned. |
| R15 | `green` | The sole workspace edge is `nimbus-network -> nimbus-core`; NNCV004/NNCV012/NNCV019 and the source census prove the effect and ownership boundaries. |
| R16 | `green` | Focused, full affected, quality, dependency/effect/census, mutation, format/diff, and docs gates above all pass with exact counts. |
| R17 | `green` | The sole full GPT-5.6 Sol/xhigh/fast item review (`019fb601-90ee-7313-bb1a-a2877c01da02`, `0.97`) found four accepted defects, red `0/4` then corrected `4/4`. The sole narrow correction review (`019fb62f-556a-79b0-8c37-bf95fb7fde68`, `0.99`) found two accepted nft defects, red `0/2` then corrected `2/2`. All affected proofs are green; no third review ran or is warranted. |
| R18 | `green` | The exact code, tests, verifier/census truth-ups, proof, and recovery ledger form one staged item checkpoint. This proof and the `done` ledger transition commit with that item; no push or PR occurs. The resulting commit hash is recorded by NNC5.3a's first recovery truth-up because a commit cannot self-record its own hash. |

## Fail-before packet

Before production behavior changes:

1. retain the two historical NNC0.6 failures as exact `0/2`;
2. add a common readiness-state-machine matrix whose complete row passes and
   whose missing/substituted facet rows fail against the current missing seam;
3. add real Container/Krun pre-spawn cases that currently accept partial
   attachment evidence;
4. add live-status/endpoint cases that currently publish with missing
   attachment evidence;
5. add an `Active + Present` fresh-process case proving the current early
   return skips lifetime recovery and pin reconciliation;
6. add provider-effect recording substitutes proving current status has no
   common read-only inspector; and
7. add a verifier mutation contract that fails exclusively when the common
   collector, both consumers, exact pin inspection, or no-effect boundary is
   removed.

## Prospective NNC5.3a boundary

NNC5.3a is independently acceptance-bearing:

- exact configured machine provider instance and generation;
- every persisted `Exposed` receipt matched to tenant, sandbox, binding, route,
  provider instance, and generation;
- exact live local proxy registration, route set, provider worker, listener
  lease, and process lifetime;
- typed current forwarding observation or a fail-closed provider-unknown
  result;
- the same durable attachment/IPAM/Netavark/pin/PEP base evidence;
- pre-spawn and running-status consumption; and
- no claim that a historical receipt alone proves current forwarding.

The split is by provider ownership, not by review chunk. NNC5.3 and NNC5.3a
each deliver one end-to-end provider mode with their own fail-before,
acceptance, proof, review, and commit.

## Non-goals

NNC5.3 does not:

- move Netavark, nftables, namespace, IPAM, forwarding, PEP, socket, or probe
  effects into `nimbus-network`;
- add a general `NetworkProvider`;
- change egress policy, proxy forwarding, TLS interception, certificate,
  service naming, DNS, cluster transport, or machine-provider ownership;
- make workload inspection side-effect-free beyond the new readiness read
  model (NNC5.6/NNC6.4a);
- run cleanup, release, finalization, or capacity reuse (NNC8.3);
- project portable status/endpoints into `nimbus-system` (NNC7);
- use IP addresses, paths, filenames, or socket addresses as workload
  identity; or
- push, open a PR, or alter the original dirty checkout.
