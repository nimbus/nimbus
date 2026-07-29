# NNC4.5 — Egress Readiness Dependency

Status: `complete; E1-E18 green`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md` NNC4.5

## Outcome

NNC4.5 composes one proxy-required network readiness dependency without moving
policy or provider effects:

- `nimbus-network` owns transport-free desired, durable, and observed
  readiness values plus pure exact-match evaluation;
- `nimbus-proxy` remains the PEP and is the only source that can inspect its
  active policy, audit health, and worker liveness;
- `nimbus-sandbox` remains the PEP lifecycle and listener-effect composition
  owner and authenticates one running PEP against its exact tenant-qualified
  listener, active port lease, provider binding, process lifetime, and expected
  policy;
- container and krun status may report `Ready` and expose endpoints only after
  that authenticated check succeeds;
- `nimbus-egress` remains the pure PDP, and no authorization rule, forwarding
  behavior, DNS behavior, credential/DLP behavior, interception CA, or proxy
  transport moves.

This is one reviewable unit of value. The transport-free values would be an
unearned seam without a real consumer; a sandbox-only boolean would preserve
the current caller-local ambiguity. The implementation is therefore ordered
into internal slices, but none is a separately complete or separately
autoreviewed plan item.

## Recovery Checkpoint

| Field | Value |
| --- | --- |
| Owner worktree | `/Users/jack/src/github.com/nimbus/nimbus-network-architecture-audit` |
| Owner branch | `codex/nimbus-network-architecture-audit` |
| Last completed commit | `0896776f3980eba7ab5e78f6ad7c3c6e5e7e280d` (`NNC4.4 model machine provider networking modes`) |
| Last completed tree | `3e394d851e4fbc6d115c464e0f2c6f1a0fcfe9c0` |
| Current dirty paths | The exact completed NNC4.5 network/proxy/sandbox/services implementation and concept-owned tests named below; this proof, the canonical plan/routing closeout, and the line-only PEP bind-census reconciliation. The narrow-review health correction adds only the proxy-owned `worker/health.rs` state machine and its existing request/reload consumers. No source outside the admitted item set is dirty. |
| Last green | E1-E18 and every gate: network 198/0/0, proxy 164/0/0, sandbox 697/0/24, services 90/0/1; focused correction proofs 30/0/2; affected check, strict Clippy, warning-denied rustdoc, format/diff, exact core-only dependency, 66/66 authority plus 35/35 non-authority census, live verifier 15/15, verifier self-test 45/45, docs 108 pages, and site 17/17. |
| Next command | Commit the exact NNC4.5 implementation/proof/ledger closeout, then begin NNC4.6 with a read-only composition-root and node-store initialization audit. |
| Blocker | None. |

## Source Audit

### Current authority and defect map

| Concern | Current source | Audit result |
| --- | --- | --- |
| Portable desired plan | `crates/nimbus-network/src/plan.rs` | Capability requirements are digest-bound, but runtime readiness dependencies are not typed or digest-bound. |
| Portable observed state | `crates/nimbus-network/src/status.rs` | Exact plan/resource generation, digest, epoch, provider, and conditions exist. It does not carry the process-lifetime fence of a live listener. |
| Durable listener authority | `crates/nimbus-network/src/port_lease.rs`, `port_lease/lifetime.rs` | The active record already owns exact listener ID, tenant, generation, epoch, provider binding, and process-lifetime generation. Reuse it; do not create another lease authority. |
| PEP readiness | `crates/nimbus-proxy/src/policy_state.rs`, `worker.rs` | `WorkloadPepReadiness` exposes public fields and only policy presence/audit health. It does not prove the expected policy or worker liveness and can be fabricated by upper crates. |
| PEP registry | `crates/nimbus-proxy/src/engine.rs` | `with_pep` and `with_attachment` take separate lifecycle reads. A composition owner cannot atomically inspect the running PEP and its lifecycle attachment. |
| Sandbox PEP composition | `crates/nimbus-sandbox/src/backends/oci/egress.rs` | `readiness` reads only the PEP. The already-running branch authenticates lease/lifetime but returns without checking the requested policy. |
| Durable reload attempt | `crates/nimbus-sandbox/src/backends/oci/egress/reload.rs`, `container/runtime/egress_reload.rs` | Desired and attempt generations are durable and exact-inspectable. Stable state does not expose the current expected attempt to readiness; `Applying` can coexist with a coarse ready PEP. |
| Container launch/status | `container/runtime.rs`, `container/runtime/status.rs` | Launch starts the PEP before runtime, but status can report Ready from application/no-probe state without exact current-policy evidence. |
| Krun launch/status | `krun/vm/lifecycle.rs`, `krun/vm/readiness.rs` | Initial launch checks netns plus any policy-bearing PEP. Later status can report Ready after the PEP is absent, unhealthy, stale, or stopped. |
| Service binding | `crates/nimbus-services/src/manager/activation.rs`, `registry.rs` | Services correctly own logical names/bindings, but trust coarse `SandboxStatus::Ready`. Backend status must therefore withdraw Ready/endpoints when its required PEP evidence is not current. |
| Pseudo outbound broker | `crates/nimbus-services/src/outbound.rs` | Public, test-only, and has no production caller. It accepts fabricated `WorkloadPepReadiness`; it is not a second PEP adapter or readiness authority. |

### Dependency and effect audit

The initial dependency remains exactly:

```text
nimbus-network -> nimbus-core
```

NNC4.5 adds no network-crate edge to `nimbus-egress`, `nimbus-proxy`,
`nimbus-sandbox`, `nimbus-services`, transport libraries, provider SDKs, or
cloud/cluster crates. `nimbus-network` imports no socket address, socket,
Axum, Pingora, Netavark, nftables, gvproxy, Iroh, or policy type for this work.

## Binding Design

### Desired

Add a canonical `NetworkReadinessRequirement` value containing:

- one stable `NetworkResourceId`;
- one exact `NetworkProviderId`;
- one provider-neutral `NetworkConditionKind`.

`NetworkPlan` contains an explicitly constructed canonical requirement set.
The set rejects duplicates, is part of serialization, is included in the
domain-separated plan digest, and participates in equal-generation conflict
classification. A direct plan has an empty set. A proxy-required plan contains
the tenant-qualified PEP listener’s `Ready` requirement.

The requirement does not contain a `NetworkResourceVersion`: doing so would
recursively require the plan digest that is itself derived from the
requirement. The version is created only after the complete plan digest exists.

### Durable dependency

Add `NetworkReadinessDependency`, constructed from:

- the exact requirement already present in a `NetworkPlan`;
- a `NetworkResourceVersion::for_plan` for that requirement;
- one exact active `PortLeaseRecord`;
- the lease’s exact `PortLeaseId` and `PortLeaseLifetime`.

The constructor fails unless all of these match:

- plan ID, generation, and digest;
- requirement resource ID and provider ID;
- port request owner, generation, and lease epoch;
- `PortLeasePhase::Active`;
- adopted provider binding;
- active nonzero process lifetime.

The dependency contains no address-as-identity field and no
`NetworkProviderHandle`. The opaque provider handle remains durable authority
interpreted only by its owning adapter; observed/dependency values carry only
the stable provider registration ID.

### Observed evidence

Add `NetworkReadinessEvidence` containing:

- the exact durable dependency fence;
- one canonical `NetworkCondition` matching the requirement’s condition kind.

Pure evaluation accepts readiness only when each desired requirement has
exactly one current durable dependency and exactly one exact evidence value
whose condition is `True`. It rejects missing, duplicate, conflicting, foreign,
stale, future, equal-generation/different-digest, wrong-epoch, wrong-provider,
wrong-lease, and old-lifetime inputs with typed diagnostics. `False` and
`Unknown` remain honest unsatisfied observations, not authority mutations.
Deleting a projection/evidence value makes readiness unsatisfied but does not
change desired state, the port lease, provider handle, or cleanup authority.

### Sandbox PEP evidence

`nimbus-sandbox` adds one concept-owned PEP readiness composer. It reads a
running `WorkloadPep` and its `RegisteredArtifacts` under the same
`EgressEngine` lifecycle lock and verifies:

1. tenant-qualified workload/listener identity;
2. exact persisted `EgressProxyAssignment`;
3. exact active port-lease request and provider binding;
4. exact retained non-cloneable process lifetime equal to durable
   `active_lifetime`;
5. actual listener address equal to the adopted binding;
6. PEP worker still live;
7. sticky audit sink healthy;
8. active sandbox policy bytes equal the manifest’s current compiled policy;
9. the active durable reload attempt is exact when one exists;
10. no durable reload is still `Applying`.

No independent `NetworkProvider`, `ForwardingProvider`, PEP registry role, or
policy interface is introduced. The engine earns one concrete
`with_pep_and_attachment` lifecycle accessor because the sandbox has a real
two-object atomic inspection need.

### Readiness consumers

- Container pre-spawn and every running-status calculation use the same
  sandbox-owned composer.
- Krun pre-spawn and every running-status calculation use that same composer.
- A failed dependency check maps a live workload to `NotReady`, clears visible
  published endpoints, and prevents service binding. It does not stop, restart,
  detach, release, or mutate policy.
- Plan-only sandbox preparation has no live PEP dependency and retains its
  existing non-execute semantics.
- Continuous orchestration after a snapshot and same-generation complete
  attachment evidence remain NNC5/NNC6 responsibilities; NNC4.5 does not claim
  that a returned snapshot can never become stale.

## Ownership And Non-Goals

| Owner | Remains authoritative for |
| --- | --- |
| `nimbus-egress` | PDP grammar, compilation, authorization, and the `requires_proxy_enforcement` decision. |
| `nimbus-proxy` | PEP transport, forwarding, policy state, audit health, credential/DLP enforcement, TLS interception, and worker lifecycle. |
| `nimbus-sandbox` | PEP process/listener lifetime, port-effect composition, OCI manifests, exact policy attempt reconciliation, and backend readiness consumption. |
| `nimbus-services` | Logical service name, residency, readiness wait, and binding materialization. |
| `nimbus-compute` | Future admitted plan compilation and the cross-domain workload/network saga. |
| `nimbus-network` | Portable requirement/dependency/evidence values, exact fencing, and pure satisfaction. |

NNC4.5 does not:

- compile production `NetworkPlan` values in sandbox; NNC6.2 owns that;
- define full Netavark/IPAM/firewall/pin/forwarding attachment readiness;
  NNC5.2/NNC5.3 own it;
- make inspection side-effect-free or move restart authority; NNC5.6 and
  NNC6.4a own that;
- guarantee no tenant instruction before complete same-generation attachment;
  NNC6.4 owns the full saga gate;
- add readiness fields to `SandboxHandle` or portable endpoint/attachment
  handles; NNC7.2/NNC7.3 own those projections;
- change PDP rules, PEP forwarding, DNS, credentials, DLP, Pingora, or
  interception CA behavior;
- introduce service DNS/xDS/Consul, shared-listener/source-IP attribution,
  QUIC/H3/MASQUE, provider auth, cluster transport, or cloud networking;
- interpret IP addresses as workload, listener, provider, or lease identity.

## Written Acceptance Criteria

| ID | Criterion | Verifiable proof |
| --- | --- | --- |
| E1 | Desired requirements are canonical and digest-bound. | Direct versus PEP-required plans have different pinned digests; duplicate requirements reject; equal generation plus changed readiness requirement conflicts. |
| E2 | The dependency is exact durable state, not a boolean. | Constructor accepts only matching Active lease/provider/lifetime and rejects every mismatched field with a typed error. |
| E3 | Evidence is observed and exact. | Exact evidence satisfies; missing, `False`, `Unknown`, duplicate, conflicting, foreign, stale/future generation, digest, epoch, lease, provider, and old lifetime fail. |
| E4 | Projection loss cannot mutate authority. | Dropping all evidence makes evaluation unsatisfied while the serialized plan/dependency/lease bytes remain unchanged. |
| E5 | PEP evidence is unforgeable through public readiness fields. | External crates cannot construct `WorkloadPepReadiness`; they consume read-only accessors or the sandbox-owned authenticated result. |
| E6 | Active policy is exact. | Initial exact policy passes; different policy bytes, untagged expected reload, different/conflicting reload attempt, or durable `Applying` state fails. |
| E7 | Audit and worker health are required. | Policy-bearing but audit-unhealthy or worker-dead PEP reports not ready. Prepared/bound but unregistered PEP yields no evidence. |
| E8 | Listener authority is exact. | Wrong tenant, sandbox/listener, lease request, provider, address, phase, generation, epoch, or active lifetime produces no ready evidence. |
| E9 | Existing-running reuse cannot preserve stale policy. | Calling ensure/reuse with a changed expected policy fails or reconciles through the durable reload path; it never returns ready from the old policy. |
| E10 | Container launch/status consume the composer. | Execute launch refuses before runtime spawn without exact PEP evidence; a live/no-probe container becomes `NotReady` and exposes no endpoints after PEP evidence is withdrawn or becomes stale. |
| E11 | Krun launch/status consume the same composer. | Execute launch refuses without exact evidence; a live endpoint-bearing VMM becomes `NotReady` and withdraws that endpoint after PEP evidence is absent/unhealthy/stale. |
| E12 | Service publication fails closed through backend status. | `service_binding_from_handle` cannot produce a binding for the not-ready/endpoint-withdrawn result; logical naming remains service-owned. |
| E13 | Applying is not ready. | Crash/ack-loss state with exact policy active but manifest still `Applying` stays unsatisfied until exact inspection and durable completion. |
| E14 | Overflow fails closed. | `PolicyGeneration` increment at `u64::MAX` returns an error and preserves the last-known-good policy. |
| E15 | Dead pseudo-authority is removed. | Source census finds no production caller of `nimbus-services::outbound`; the public test-only pseudo-broker is deleted rather than adapted into another policy/readiness owner. |
| E16 | Dependency/effect boundaries remain exact. | Metadata proves `nimbus-network -> nimbus-core` is its sole workspace edge; source scans find none of the forbidden dependencies/effects in `nimbus-network`. |
| E17 | Existing PEP/PDP and cleanup behavior is preserved. | Full proxy and sandbox suites pass, including policy, audit, reload, stop, quarantine, port lifetime, trust-anchor, and interception tests. |
| E18 | One item review cadence is honored. | Exactly one Sol/xhigh/fast full structured review runs only after E1-E17 and all quality/static/docs gates are green on the frozen item diff. |

## Fail-Before Packet

Production behavior is not edited until these tests fail at their named
assertions:

1. **Portable dependency model**
   - a PEP-required `NetworkPlan` type/constructor test fails to compile before
     the readiness requirement contract exists;
   - same-generation readiness requirement changes are not yet conflict-bound;
   - old lifetime evidence is not yet rejectable.
2. **Container stale desired policy**
   - persist reload desired generation 2/`Applying`;
   - retain a running generation-1 PEP and a live no-probe runtime;
   - current inspection reports `Ready`;
   - expected result is `NotReady` with no endpoints and no policy/lease
     mutation.
3. **Krun lost PEP**
   - construct a live no-probe runtime whose PEP was stopped/removed after
     launch;
   - current inspection reports `Ready`;
   - expected result is `NotReady` with no endpoints.
4. **Exact lifetime/provider**
   - retain a running PEP but substitute old lifetime or foreign provider
     evidence in the durable lease;
   - current coarse readiness passes;
   - expected authenticated composition rejects before publication.
5. **Worker exit**
   - terminate the PEP worker while retaining policy state and registry entry;
   - current readiness stays true;
   - expected readiness is false.
6. **Overflow**
   - exercise a `u64::MAX` policy generation;
   - current saturating increment returns the same generation;
   - expected result is an error with unchanged last-known-good state.

The NNC0.6 netns-only partial-attachment tests stay ignored/expected-red for
NNC5. They are not enabled or claimed by this item.

## Implementation Order And Owned Paths

The following paths are admitted only after the corresponding expected-red
proof exists:

1. **Portable value slice**
   - `crates/nimbus-network/src/readiness.rs` (new)
   - `crates/nimbus-network/src/lib.rs`
   - `crates/nimbus-network/src/plan.rs`
   - focused `nimbus-network` unit/integration tests
2. **PEP evidence slice**
   - `crates/nimbus-proxy/src/policy_state.rs`
   - `crates/nimbus-proxy/src/worker.rs`
   - `crates/nimbus-proxy/src/worker/policy_reload.rs`
   - `crates/nimbus-proxy/src/engine.rs`
   - concept-owned proxy tests
3. **Sandbox composition slice**
   - `crates/nimbus-sandbox/src/backends/oci/egress.rs`
   - a concept-owned `egress/readiness.rs` child
   - `crates/nimbus-sandbox/src/backends/oci/egress/reload.rs`
   - `crates/nimbus-sandbox/src/backends/oci/port_lease.rs` only for a narrow
     exact active-listener verification helper
   - container and krun lifecycle/readiness callers plus concept-owned tests
4. **Consumer cleanup slice**
   - delete `crates/nimbus-services/src/outbound.rs`
   - remove its module export from `crates/nimbus-services/src/lib.rs`
   - add/adjust service binding regression tests only if backend-status coverage
     cannot prove E12 directly
5. **Closeout**
   - this proof, the canonical plan, and the plan routing index

No compute, tenant, egress-PDP, system, server, machine, KV, cluster, ingress
TLS, secret, or identity source path is admitted.

## Adjacent Owner Reconciliation

| Plan | NNC4.5 relationship |
| --- | --- |
| `nimbus-sandbox-egress-regression-and-seams-plan.md` SERS3 | NNC4.5 consumes only the now-urgent PEP pre-spawn/status parity slice. KVM lane, full attachment proof, port-pool, and firewall work remain there/NNC5. |
| `nimbus-proxy-policy-hardening-plan.md` | No policy grammar, IO phase, ECH, or PDP dependency work moves. |
| `nimbus-proxy-density-and-datapath-plan.md` | Dependency identity must not assume one process/socket or source-IP attribution; density work remains measurement-gated. |
| `nimbus-masque-h3-egress-plan.md` | Proxy-required QUIC/UDP remains deny-by-default; no H3/MASQUE support is added. |
| `nimbus-tenant-admission-audit-plan.md` | Tenant quota/admission and audit export remain above this allocation/readiness seam. |
| `service-identity-provider-auth-plan.md` | Workload identity/provider credentials and the egress interception CA stay distinct. |
| `research/nimbus-sandbox-modernization-review-2026-07.md` | Preserves the PDP/PEP split, per-sandbox listener defense, and sandbox effect locality. Shared-listener and cluster-transport recommendations remain separate. |
| NNC5/NNC6/NNC7 | Full attachment evidence, plan compilation, cross-domain saga ordering, and portable handle projection remain explicitly downstream. |

## Verification Gates

Before candidate freeze:

- every E1-E17 row has a named passing test or static command with exact count;
- full `nimbus-network`, `nimbus-proxy`, `nimbus-sandbox`, and
  `nimbus-services` affected suites pass with declared skips;
- affected all-target/all-feature check passes;
- strict Clippy with `-D warnings` passes for affected crates;
- warning-denied rustdoc passes for affected crates;
- `cargo fmt --all --check` and both diff checks pass;
- metadata proves the exact core-only `nimbus-network` workspace edge;
- forbidden dependency/effect and duplicate-readiness-authority scans pass;
- the network control-plane verifier and adversarial self-tests pass;
- `bash scripts/check-docs.sh` and
  `bash scripts/verify-nimbus-docs-site.sh` pass with exact counts.

Only then freeze the complete item diff and run exactly one structured
`autoreview` with `gpt-5.6-sol`, `xhigh`, and fast service tier. An accepted
finding that materially changes executable code permits affected proof reruns
and exactly one narrow correction review focused on that defect. Docs/ledger
wording, formatting, non-material cleanup, elapsed time, or internal diff
chunking do not permit another review.

## Verification Evidence

### Fail-before

The six-part packet was red before its corresponding production behavior
changed:

| Packet | Red evidence |
| --- | --- |
| Portable dependency/lifetime model | The no-run integration target exited `101` exclusively because the readiness requirement, dependency, evidence, and old-lifetime rejection API did not exist. |
| Container stale desired policy | The semantic regression observed `Ready` with generation-1 PEP evidence while generation 2 remained desired/`Applying`; the named assertion required `NotReady`. |
| Krun lost PEP | The semantic regression observed `Ready` for a live no-probe VMM after its PEP disappeared; the named assertion required `NotReady`. |
| Exact lifetime/provider | The portable constructor/evaluator target could not express the exact active-lifetime/provider fence before the new model existed; its completed mismatch matrix now rejects each substituted field. |
| Worker exit | A retained active policy remained ready after the worker stopped; the named assertion required readiness withdrawal. |
| Generation overflow | Saturating increment preserved `u64::MAX`; the named assertion required an error with byte-identical last-known-good policy. |

### Acceptance matrix

| Criteria | Passing proof |
| --- | --- |
| E1-E4 | `crates/nimbus-network/tests/readiness_dependency.rs`: 13/13. It pins direct versus PEP-required digests; rejects duplicate requirements, nonmember dependencies, and equal-generation conflicts; evaluates provider-distinct requirements independently; authenticates every durable field; rejects every missing/false/unknown/duplicate/conflicting/foreign/stale/future/substituted evidence shape; and proves projection loss leaves serialized desired/durable authority unchanged. |
| E5-E7, E14 | Proxy focused regressions pass 1/1 for overflow, 1/1 for stopped-worker readiness withdrawal, 2/2 for stopped/audit-unhealthy reload rejection without policy mutation, and 2/2 for worker/audit transitions serialized against authenticated control effects. `WorkloadPepReadiness` fields are `pub(crate)` with public read-only accessors; full proxy passes 164/0/0. |
| E6, E8, E9, E13 | Concept-owned OCI readiness tests pass 3/3 within the 6/6 readiness filter: stale policy reuse rejects; independent request-owner/generation/epoch/listener/sandbox/provider/phase/lifetime substitutions reject; and only the exact completed reload attempt satisfies. |
| E10 | `container_ready_rejects_active_pep_for_prior_desired_policy_attempt` passes 1/1 and exercises the real pre-spawn gate plus running status/endpoint withdrawal. |
| E11 | `krun_inspect_withdraws_ready_projection_when_pep_dependency_is_absent_or_not_ready` passes 1/1, exercises the real pre-spawn gate, proves `Ready` with one visible `published-api` endpoint, and then proves `NotReady` with that exact endpoint absent after PEP loss. |
| E12 | `not_ready_endpoint_withdrawal_cannot_materialize_a_service_binding` passes 1/1; the full services suite passes 90/0/1, where the sole declared ignore remains the downstream NNC0.6 complete-attachment baseline. |
| E15 | `crates/nimbus-services/src/outbound.rs` and its module export are absent; no production `nimbus_proxy` dependency or outbound pseudo-broker reference remains in `nimbus-services`. |
| E16 | Cargo metadata reports exactly `["nimbus-core:normal"]` for `nimbus-network` workspace edges. NNCV004 and NNCV012 pass; manifest/effect scans find no upper-crate, transport, socket, process, or provider-effect dependency in the portable crate. |
| E17 | Full suites: network 198/0/0, proxy 164/0/0, sandbox 697/0/24, services 90/0/1. Existing policy, audit, reload, stop, quarantine, lifetime, trust-anchor, and interception coverage remains green. |
| E18 | The complete 188,633-byte first candidate received exactly one full Sol/xhigh/fast review after every pre-freeze gate passed. Its accepted executable defects received affected proof reruns and exactly one 224,286-byte narrow Sol/xhigh/fast correction review. The narrow review's one accepted P1 race is corrected with focused and full affected proofs; cadence permits and requires no further structured review. |

### Quality, structure, and boundary gates

- `cargo check -p nimbus-network -p nimbus-proxy -p nimbus-sandbox
  -p nimbus-services --all-targets --all-features` passes.
- Strict affected Clippy with `--no-deps -- -D warnings` passes. The only
  emitted warnings are pre-existing vendored Brotli diagnostics outside the
  affected crates.
- Warning-denied affected rustdoc passes.
- `cargo fmt --all --check` and `git diff --check` pass.
- The bind census passes with 66/66 production authority occurrences and 35/35
  classified non-authority risks across 26 logical sites. NNC4.5 changed only
  five PEP listener line locations; the inventory identity, classification,
  owner, and behavior are unchanged.
- The aggregate verifier passes 15/15 and its adversarial self-test passes
  45/45. The self-test first caught the stale PEP listener line locations; the
  line-only inventory correction restored both the live and mutation proofs.
- `scripts/check-docs.sh` passes 108 pages; the docs-site verifier passes all
  17/17 conditions.
- Modularity is within the repository contract:
  `nimbus-network/src/readiness.rs` is a coherent 726-line deep module; the
  OCI egress test composition root remains 1,947 lines after its intact
  readiness-only fixture/test family moved to a 400-line concept-owned child.
  `nimbus-proxy/src/worker.rs` is 1,510 lines and remains an explicit
  1,500-1,999 exception: it is the security-ordering composition root for one
  request lifecycle, while NNC4.5 moved the new 168-line process-health state
  machine and its concurrency proofs into concept-owned `worker/health.rs`.
  Mechanically splitting the remaining request phases would obscure their
  audit-before-forward and terminal-record ordering; future decomposition stays
  with the proxy architecture owner and must preserve those behavioral proofs.

## Structured Review And Correction Disposition

The sole full item review used GPT-5.6 Sol, xhigh reasoning, fast service tier,
and one 188,633-byte pass in reviewer thread
`019fabe1-4055-7a41-87ef-0d83f6b0d829`. It reported six findings and judged the
candidate incorrect at 0.98. Owner source inspection accepts all six:

| Finding | Disposition and required correction |
| --- | --- |
| P1 replacement PEP loses completed reload-attempt identity | Accepted. Starting a replacement from desired policy bytes alone creates an untagged active policy while durable completed state expects `active_attempt()`. Add a restart/replacement regression and seed or replay the exact durable attempt before readiness authentication. |
| P1 reload applies to unauthenticated registration | Accepted. `ensure_reload_registration` currently checks only `readiness().is_some()`. Add an attachment/listener-authority authentication mode that permits stale policy bytes but rejects dead, foreign, or stale lifecycle evidence before the durable receipt can complete. |
| P2 dependency accepts a requirement absent from its plan | Accepted. `validate_version` authenticates the plan/version but not desired-set membership. Add typed nonmember rejection and a focused regression. |
| P2 requirement matching omits provider identity | Accepted. Construction permits provider-distinct requirements while evaluation conflates them. Use one consistent provider-inclusive exact key and prove both providers evaluate independently. |
| P2 E8 sandbox substitution coverage incomplete | Accepted. Add composer-level mutations for lease request/owner, provider, phase, generation, epoch, sandbox/listener identity, and lifetime; do not rely only on portable constructor tests. |
| P2 krun endpoint withdrawal proof vacuous | Accepted. Make an endpoint visible in the Ready precondition, then prove PEP loss changes Ready/nonempty to NotReady/empty. |

Because the first four findings require executable corrections, the agreed
cadence permits affected proof reruns followed by exactly one narrow correction
review focused on these six defects. No second full item review was run.

The correction packet is now green:

- missing plan membership and provider-distinct matching failed before their
  portable fixes, then passed 13/13; full network passes 198/0/0;
- release-authority replacement exposed policy generation 1 instead of the
  durable generation 2, then passed after both manifest-aware PEP start helpers
  replayed the exact stable attempt before readiness;
- a foreign listener assignment previously completed reload, then failed
  before PEP or manifest mutation after reload inspection/effect moved under
  the workload-local lifecycle lock and exact attachment authentication;
- the reload-focused packet passes 4/0/2 in the owner replay and 9/0/2 in the
  delegated broader filter; OCI egress passes 54/54;
- the strengthened E8 composer matrix and non-vacuous krun endpoint transition
  pass; full sandbox passes 697/0/24;
- all affected quality, boundary, verifier, self-test, and docs gates pass.

The one permitted narrow correction review used GPT-5.6 Sol, xhigh reasoning,
fast service tier, and one 224,286-byte pass in reviewer thread
`019fac02-f9c1-7603-996d-0fac0592e63f`. It accepted one P1 at 0.94: worker
liveness or sticky audit health could transition after the sandbox's
attachment authentication snapshot but before the PEP policy-reload effect.
The final correction stays in `nimbus-proxy`, the owner of both facts and the
effect:

- `worker/health.rs` owns one process-lifetime health state and transition gate;
- every production worker-stop and durable-audit-failure transition takes the
  gate exclusively;
- `reload_policy_for_attempt` takes the gate shared, rechecks both facts, and
  holds it through the policy mutation;
- stopped-worker and audit-unhealthy fail-before regressions each exited 101
  because the old code returned a generation-2 receipt, then pass while proving
  the retained policy remains untagged;
- two deterministic concurrency tests prove worker and audit transitions
  cannot cross an authenticated control effect and that later effects reject;
- proxy focused worker proofs pass 6/6, full proxy passes 164/0/0, reload
  integration passes 9/0/2, and full sandbox remains 697/0/24.

This correction changes executable code, so affected proofs and quality gates
were rerun. The narrow review was the permitted correction review itself;
there is no third structured review.

## Status Ledger

| Checkpoint | Status | Evidence |
| --- | --- | --- |
| Read-only ownership/call-graph audit | `done` | Three independent read-only packets plus owner inspection converged on the same desired/durable/observed value seam and sandbox-owned exact composer; no packet edited files. |
| Adjacent owner reconciliation | `done` | SERS3 PEP parity is consumed narrowly; proxy policy/density/MASQUE, tenant admission, identity, NNC5 attachment, NNC6 saga, and NNC7 projection owners remain distinct. |
| Written acceptance contract | `done` | E1-E18, fail-before packet, admitted paths, non-goals, order, and gates are recorded above. |
| Expected-red proofs | `done` | Portable API/lifetime exited 101; container stale-policy, krun lost-PEP, worker-exit, and overflow reached their intended semantic assertions before each corresponding production correction. |
| Implementation | `done` | Portable desired/durable/observed readiness, exact proxy evidence, one sandbox composer, both OCI consumers, service fail-closed regression, and dead pseudo-broker deletion are complete within the admitted paths. |
| Candidate freeze and gates | `done` | The final corrected candidate restores E1-E17: network 198/0/0, proxy 164/0/0, sandbox 697/0/24, services 90/0/1, focused corrections 30/0/2, affected quality/static gates, verifier 15/15 plus self-test 45/45, docs 108, and site 17/17. |
| Sole full structured review | `done` | Actual GPT-5.6 Sol/xhigh/fast thread `019fabe1-4055-7a41-87ef-0d83f6b0d829`; six accepted findings, including four executable defects. No second full review ran. |
| Narrow correction review | `done` | Actual GPT-5.6 Sol/xhigh/fast thread `019fac02-f9c1-7603-996d-0fac0592e63f`; its one accepted P1 health/effect race is corrected with 2/2 fail-before conversions, 2/2 transition-serialization proofs, full proxy 164/0/0, and full sandbox 697/0/24. No further review is permitted or needed. |
| Commit and ledger closeout | `done` | This proof, the exact implementation, and the canonical transition to NNC4.6 are one owning checkpoint commit; no push or PR is authorized. |
