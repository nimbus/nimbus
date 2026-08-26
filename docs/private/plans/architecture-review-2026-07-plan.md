# Architecture Review 2026-07 — Improvement Control Plane

Status: `active`
Owner: this plan
Baseline: main @ `137cc632a1c8585545d200ea49f44bd236478175` for RR30 and RR31.
Proof root: `proof/architecture-review-2026-07/`.
Next action: Execute SA8 as a correction to the merged IMV1 codec. Keep the
dirty IMV7 closeout worktree unchanged. After SA8, triage SA1, SA3, and SA4.
Keep RR31 deferred during SA8 even though its activation trigger now holds.
Keep RR30 deferred until BLI3 merges; IMV1 has merged.
Provenance: full-workspace architecture review, 2026-07-06 (six parallel
subsystem mappings + second-pass gap/hygiene sweeps + extraction/identity
inventory). Review artifact:
`proof/architecture-review-2026-07/nimbus-architecture-review.html`.

## Current architecture reconciliation (2026-08-16)

The review verdict and decision records below are historical starting evidence.
The current 43-member workspace remains acyclic, but three staged decisions
have landed and supersede the original wording:

- `nimbus-compute` is the transport-free workload-saga coordinator consumed by
  server and CLI. Its normal dependency on `nimbus-network` carries portable
  network plans and lifecycle evidence without moving provider effects.
- `nimbus-network` is the transport-free connectivity-resource control plane.
  Its only outgoing workspace edge is `nimbus-core`; concrete effects remain
  in sandbox, server, KV, machine, proxy, and node owners.
- SI0 created `nimbus-workload-identity`. Admission-anchored workload identity
  projection remains distinct from provider credential minting.

The network architecture and completed closeout record are canonical in
[`archive/nimbus-network-control-plane-plan.md`](archive/nimbus-network-control-plane-plan.md)
and
[`../architecture/network/control-plane.md`](../architecture/network/control-plane.md).

## What This Plan Owns

Every improvement found by the 2026-07 architecture review: guarantee
repairs, seam repairs, consolidations, decompositions, test-infrastructure
gaps, UI hygiene, doc/spec truth-ups, and the two architecture decisions
(compute-plane extraction, workload identity). Items are tracked in the
band ledgers below; this file is the single status source for the campaign.

Non-goals: no new product surfaces; no changes owned by other active plans
(`distribution-plan.md`, FSV console work, SI identity minting). Where an
item touches another plan's lane, the item names the coordination.

## Review Verdict (context, one paragraph)

The architecture is sound: the 40-crate graph is acyclic; all load-bearing
invariants verified (core zero-I/O, runtime zero workspace deps, single
mutation path via call-site census over all five adapters, storage
atomicity, egress PDP/PEP split with pre-response decision logging);
`packages/convex` passes the thin-wrapper audit. Hygiene is strong (zero
TODO/FIXME debt in production code, disciplined typed errors). The findings
below are therefore consolidation and deepening work, not rot repair.

## Decision Records

### AD1 — Create `nimbus-compute` (compute-plane composition crate)

**Decision: yes, staged (CP1–CP3 below). Name: `nimbus-compute`.**

A crate between `nimbus-server` and the execution crates is justified, but
the extraction is smaller than "everything between server and engine":
the heavy orchestration already lives in `nimbus-services` (ServiceManager,
catalog, broker, templates), `nimbus-bridge` (runtime host boundary), and
`nimbus-system` (system-tenant records). What `nimbus-server` still owns is
the **compute-plane composition root**, and that is what moves:

- Transport-free today (moves nearly as-is): `src/execution/` (868 LoC,
  zero axum imports — invocation workers, provenance, fs_grants,
  subscriptions, runtime error mapping), `artifact_verifier_effects.rs`
  (860, only core+runtime types), `machine_lifecycle.rs` (62),
  `service_manager.rs` binding shim (36).
- Split required: `state.rs` (512) — extract a `ComputeState` holding
  engine + registries with **zero axum types**; the server keeps a thin
  `AppState` wrapping `ComputeState` plus transport concerns.
  `construction.rs` (273) splits the same way.
- Follow-on: the orchestration halves of `http/{deploy,services,
  sandboxes,sandbox_spec,scheduling,machines}.rs` become compute-owned
  functions; axum handlers reduce to extract → call → respond.

Dependency shape: `nimbus-server → nimbus-compute → {nimbus-engine,
nimbus-runtime, nimbus-bridge, nimbus-services, nimbus-sandbox,
nimbus-system, nimbus-node, nimbus-machine, nimbus-artifacts,
nimbus-provenance, nimbus-fs, nimbus-workloads}`; shared below both:
core, tenant, auth, storage, egress, blob. Roughly 13 of the server's 29
workspace deps move behind the new crate. `nimbus-cli` may consume
`nimbus-compute` directly for start/dev wiring, thinning the cli→server
coupling. Invariant for the new crate: **zero axum/http-transport
dependencies** (enforced by Cargo.toml + a grep gate in CI or the item
acceptance).

Naming rationale: `nimbus-workloads` is taken (desired-state, placement,
tenant workload policy/credential-projection **spec** types — a
control-plane crate, not request execution). Rejected: `nimbus-workload`
(hopeless confusion with `nimbus-workloads`), `nimbus-orchestrator`
(vague, k8s-loaded), `nimbus-execution` (too narrow for future scope).
`nimbus-compute` names the plane — admit → wire → execute tenant
workloads across isolates, sandboxes, and services — and gives future
surfaces (`nimbus run jobs`, `nimbus run exec`, agent workloads) a
canonical home.

Coordination: `nimbus-services` stays the service/sandbox/session resource
manager; compute composes it, never re-owns it. The connection-broker and
CB-plan seams are unaffected.

### AD2 — Workload identity: no new crate now

**Decision: do not create `nimbus-workload-identity` today. Revisit as the
first deliverable of `service-identity-provider-auth-plan.md` SI0.**

The identity ladder is already deliberate and healthy:

| Layer | Type | Home | Role |
| --- | --- | --- | --- |
| Routing key | `WorkloadId` | `nimbus-core/src/types.rs:68` | Opaque key; placed in core **on purpose** so `nimbus-proxy`'s PEP registry can key on it without depending on sandbox crates (reachability-lint enforced off the hot path) |
| Admitted identity | `WorkloadIdentity` | `nimbus-tenant/src/identity.rs:132` | Rich projection constructible **only** via `from_decision(&TenantIsolationDecision)`; renders SPIFFE-shaped `subject()`/`spiffe_id()` strings |
| Node-local name | `TenantWorkloadId` | `nimbus-node/src/host_lifecycle.rs:41` | systemd unit naming |

Credential issuance (OIDC/JWT minting, SPIFFE SVIDs, mTLS, provider
adapters) is entirely owned by the deferred SI plan (SI0–SI8, all todo,
activation-gated on a concrete provider need). The only live
credential-issuing machinery today is the egress-proxy MITM CA
(`nimbus-proxy/src/tls_authority.rs`), which is not workload identity.
Creating an identity crate now would be an empty shell, and moving
`WorkloadIdentity` out of `nimbus-tenant` would break a security property:
identity minting is unreachable without an admission decision. When SI0
activates, extract **issuance** (not the projection) into its own crate;
the projection stays admission-anchored in `nimbus-tenant`. Near-term doc
work is DS2.

Refinement (2026-07-06 plan audit): SI0 is provider-independent and
executable today — its largest component, the stable workload subject
projection, is already shipped (`nimbus-tenant/src/identity.rs:171
subject()`, `:203 spiffe_id()` render the SI Identity Contract exactly).
If the owner chooses to start early, the sanctioned shape is: execute SI0
(policy-input type, per-credential claim set, mint/deny audit schema,
fail-closed tests) and land those types in a new `nimbus-workload-identity`
crate depending only on `nimbus-core` + `nimbus-tenant`, with an
`IdentityIssuer` trait and no provider SDKs (per the SI plan's dependency
posture). The projection does not move. Production minting still hard-gates
on horizontal-scaling HS1 (cluster node identity) per the SI plan; a
local-dev issuer is the only pre-HS1 issuance path and must stay
explicitly non-production.

### AD3 — SR7/SR8: keep typed enum dispatch; delete EmbeddedPersistenceProvider (2026-07-07)

**Decision (SR7):** retain the typed `TenantPersistence`/`match_tenant_persistence!`
enum dispatch. Do NOT introduce `Arc<dyn ProviderObject>` now. Rationale:
the dispatch is documented-deliberate (`persistence/provider.rs:43`); the
capability traits carry generic closure methods that are not object-safe
as written, so erasure is a design project, not a refactor; object-safety
would box futures/closures through the hottest read path that is
monomorphized today; and GR1's evidence shows drift bugs come from
duplicated bodies, not macro fan-out — the exhaustive macros are a drift
DEFENSE. A 6th in-tree backend is bounded, compiler-enforced mechanical
work.

**Re-open conditions (either):** (a) a genuine out-of-tree/plugin backend
requirement appears; or (b) after SR1 is merged, a small prototype proves
an object-safe provider facade (including a decision for
`TenantPersistenceSnapshot`) removes more complexity than it adds. Absent
those, the enum stands.

**Decision (SR8):** delete `EmbeddedPersistenceProvider`
(`async_storage/traits.rs`) — one real method (`list_tenants`) plus an
associated read type doing too little; `TenantLifecycle` already covers
the surface. Move redb `list_tenants` to an inherent method, update the
`TenantLifecycle` impl, remove the export. SEQUENCE: after the
storage-lane (SR1/CO6/DS3) PR merges, to avoid same-file churn. If a
future SR7 re-open deliberately reuses the name for a deeper seam, that
ADR supersedes this deletion's naming, not its cleanup.

## Corrected / No-Action Ledger

Recorded so later reviews do not re-flag these:

- `nimbus-egress/src/policy.rs` split — **no action**: 1,532 raw lines but
  tests start at :936; production size is under threshold. The architecture
  quality ledger records the combined production-and-test file for threshold
  enforcement; split the tests by policy family if it reaches 2,000 lines.
- `nimbus-server` `http/{resource}.rs` vs `resource_control/{resource}.rs`
  — **legitimate seam** (route/response vs authorization), not duplication.
- `ws/` module, `nimbus-code-index`, `nimbus-license`, `nimbus-bin` —
  clean, no action.
- `nimbus-cli` `dev/` reusing `start` — correct seam, not duplication.
- Justified suppressions stay: NDB3 signal wiring (`zbus_client/mod.rs:62,
  :120`), MTN7 segment allocator (`oci/network/segment.rs:178`),
  `reversed_empty_ranges` idiom (blob/crypto), engine encryption doc'd
  allow.
- `backends/krun/bundle.rs` (1,406) and `mysql/backend.rs` (1,457) —
  coherent; watch, split only if they cross 1,500.
- `nimbus-adapters` 37-line facade — deliberate optional embedder facade.
- `packages/firebase` standalone wire client — by design (independent
  wire-protocol shape per ARCHITECTURE.md); only the committed-codegen
  question is actionable (DE16).
- Engine `.expect()` population — mostly disciplined invariant assertions;
  only the two named items in GR6 plus a confirmation pass.

## Band Ledgers

Status values: `todo` | `in_progress` | `deferred(<dependency>)` | `done (evidence)` |
`no-action (reason)` | `blocked (reason)`. Update the cell when the item
moves; one evidence line per completed item (test name/count or commit).

### Band GR — Guarantee and fail-closed repairs

| ID | Item | Where | Size | Status |
| --- | --- | --- | --- | --- |
| GR1 | Collapse the storage write shapes into one transactional core. LANDED as shared apply core (`store/write/apply.rs`) + single `begin_write` choke point; scope grew to THREE bodies (no-index point, with-index point, batch) and fixed two drift bugs (D1 with-index update_time stamping, D2 with-index delete binding removal) plus unified point-insert Conflict semantics. | `nimbus-storage/src/store/write/{apply,direct,batch,store_entry}.rs`, `index/maintenance/transaction.rs` | medium | done (PR #129 squash-merged bcac953bb, 2026-07-06; storage 328 / engine 308 / system 30 tests green; autoreview clean; spec at proof/architecture-review-2026-07/gr1-spec.md) |
| GR2 | Egress posture became a mandatory named constructor argument (scope grew from pairing-only): public `RuntimeEgressPosture`, silent CoarsePermissions default + `with_egress_gateway` builder + dead Missing arm deleted, server helper collapsed onto the `HostBridge + EgressGateway` bound, Cloudflare on NAMED `Gateway(DenyAllEgressGateway)` + new denied-fetch e2e test, facade exports. | `nimbus-runtime/src/{egress.rs,runtime/facade.rs}`, `nimbus-server/src/execution/invocations/mod.rs`, `adapters/cloudflare/host_bridge.rs`, `crates/nimbus/src/lib.rs` | small | done (PR #132 squash-merged 2e30e88e0, 2026-07-07; server 588 + bridge/testing/system 54 local, full CI green incl. runtime lane; 2 review passes; spec at proof/architecture-review-2026-07/gr2-spec.md) |
| GR3 | Durable-before-response decision logging, fail closed. LANDED far beyond the original scope: fallible DurableDecisionSink split from telemetry, durable-before-response on all terminals + forward-intent before allow-forward, per-request request_id correlation + intent/terminal/after-response record kinds, three-row intercept audit, ResponseStartedSignal confirmed-write arming, sticky audit_healthy enforced in-path, fail-closed upstream-head classification, worker.rs→terminal.rs decomposition. | `nimbus-proxy/src/{decision_log,worker,terminal,pingora_io,pingora_app,https_intercept,connect,policy_state,phase,fanout}.rs`, `nimbus-sandbox/oci/egress.rs`, `nimbus-services/outbound.rs` | small | done (PR #134 squash-merged 19ac348b9, 2026-07-07; proxy 130 + sandbox/services 334 tests; 15 autoreview passes, last clean; spec at proof/architecture-review-2026-07/gr3-spec.md) |
| GR4 | Trigger-execution worker at-least-once parity: candidates worker requeues on store error, execution worker only warns and drops (`begin_attempt`/`save`/`complete` failures). Add matching requeue/retry or document why persisted `RetryPending` makes drop safe. | `nimbus-engine/src/tenant/trigger_execution.rs:250-254` vs `trigger_candidates.rs:481-497` | small | done (PR #142 merged, 2026-07-07; dual-reviewed 3 rounds, 2 P1s + P2-follow-on + P3 fixed pre-merge) |
| GR5 | Consolidate path-sanitization onto the capability choke points: 13 independent traversal-defense implementations across fs/proxy/runtime/egress/cli. Route through the FsCaps/cap-std validation path (or one shared core helper) so divergence can't reopen traversal bugs. | `nimbus-egress/src/policy.rs`, `nimbus-fs/src/{memfs,object/mod}.rs`, `nimbus-proxy/src/request.rs`, `nimbus-runtime/src/runtime_capabilities/paths.rs`, cli ×3, +5 more | medium | done (PR #140 merged e60ece530, 2026-07-07; nimbus-core has_parent_dir_component + 3 sites; runtime site excluded for zero-workspace-deps; rest already single-source (scope truth-up)) |
| GR6 | Engine production unwrap/expect pass (~115 sites): confirm each is a true invariant; fix the two known non-invariants — `persistence_config.rs:533` key-provider `.unwrap()` → typed `EncryptionValidationError`; consider propagating `background_executor.rs:32`. | `nimbus-engine/src` (non-test) | small | done (PR #142 merged, 2026-07-07; dual-reviewed 3 rounds, 2 P1s + P2-follow-on + P3 fixed pre-merge) |
| GR7 | Prove the KV RESP surface cannot serve without credentials: verify `CredentialRegistry` is wired non-optionally in `serve`, add a deny test if absent. | `nimbus-kv/src/server.rs:241` | small | done (PR #135 merged 07ed44071, 2026-07-07; no-action-with-evidence: CredentialRegistry non-Option, existing unauthenticated_command_is_rejected proves RESP deny) |
| GR8 | Provenance negative-path tests: failing-verifier + policy-mismatch cases proving fail-closed admission at the provenance layer (or confirm equivalent coverage in `nimbus-artifacts/src/admission.rs` tests and record it). | `nimbus-provenance`, `nimbus-artifacts/src/admission.rs` | small | done (PR #135 merged 07ed44071, 2026-07-07; +failing-verifier admission test, policy-mismatch already covered) |
| GR9 | BlobGc write-intent pins: `BlobGc::sweep` (`nimbus-blob/src/gc.rs:54`) is enumerate-then-release protected only by a wall-clock grace_window — no write-intent pin registry, no seal-before-enumerate barrier (TOCTOU). Concrete repro: `nimbus-sandbox/src/volume.rs:271` puts a snapshot blob unpinned between `store.put` and recording the SnapshotId. Add a pin/lease registry so in-flight writes are protected by an explicit hold, not just grace time. (Surfaced by the 2026-07-07 DS3 storage inventory.) | `nimbus-blob/src/gc.rs`, `nimbus-sandbox/src/volume.rs:271` | medium | done (PR #154 merged 92d84ffc8, 2026-07-08; BlobPinRegistry write-intent pins, sweep retention arm, volume snapshot repro closed) |

### Band SR — Seam repairs

| ID | Item | Where | Size | Status |
| --- | --- | --- | --- | --- |
| SR1 | Seal the concrete-store leak in the async read seam: parameterize read closures over `TenantPointRead + TenantRangeScan + DurableJournal` instead of `Arc<TenantStore>`; move planner/journal helpers onto the capability traits. | `nimbus-storage/src/async_storage/traits.rs:27-53`, `nimbus-engine/src/engine/queries/` | large | done (PR #141 merged 7b79aed10, 2026-07-07; ReadCapabilities trait sum + compile-time proof; closures no longer name TenantStore; enum untouched (AD3)) |
| SR2 | Recompose the S3 surface over Seams A+B: resolve a per-tenant `(Arc<dyn BlobStore>, impl ObjectMetaStore)` once per request; delete the `S3ObjectBackend` facade (per-call tenant args + parallel manifest API). Pre-launch: breaking change preferred. | `nimbus-s3/src/backend.rs:8`, `nimbus-server/src/adapters/s3/listener.rs:50` | medium | done (PR #152 merged, 2026-07-07; facade deleted; per-request S3TenantResolver + lazy guarded blobs; tenant-safety P1 review-caught+fixed) |
| SR3 | One entry per ServiceManager verb: make `*_for_decision_async` the seam; demote `_async` / `_for_context_async` variants to private helpers. | `nimbus-services/src/manager/{sandboxes.rs:16-41, activation.rs:83-106}` | small | done (PR #144 merged, 2026-07-07; authz core + verb seam + HTTP-adapter mount seam; 1 test-quality fix) |
| SR4 | HTTP-mount seam mirroring `WireProtocolAdapter`: `build_router` iterates registered HTTP protocol adapters instead of hardcoded merges. | `nimbus-server/src/router.rs:563-571`, `adapters/wire.rs:22` | medium | done (PR #144 merged, 2026-07-07; authz core + verb seam + HTTP-adapter mount seam; 1 test-quality fix) |
| SR5 | Seal the `object_store` constructor leak: accept a Nimbus-owned cloud-config enum; build `Arc<dyn ObjectStore>` internally behind a feature-gated factory. | `nimbus-blob/src/object_store.rs:33,48` | small | done (PR #152 merged, 2026-07-07; BlobCloudConfig seals object_store out of public API) |
| SR6 | Consolidate `nimbus-bridge/src/host_calls/` sub-40-line dispatch shims into one `host_calls/dispatch.rs` keyed by call kind; ABI logic stays in `abi/`. | `nimbus-bridge/src/host_calls/{sync.rs,async_calls.rs,async_trace.rs}` | small | done (PR #148 merged, 2026-07-07; three shims -> host_calls/dispatch.rs call-kind ladder; server guard test repointed) |
| SR7 | (Speculative — requires SR1; discussion first.) Erase the three parallel 5-arm persistence enums + `match_*!` macros in favor of `Arc<dyn ProviderObject>` over the capability traits. `provider.rs:42-44` documents the enum dispatch as deliberate — treat as an ADR discussion, not a defect. | `nimbus-engine/src/persistence.rs:1-58`, `persistence/{provider,tenant,executor}.rs` | large | no-action (AD3, 2026-07-07: keep typed enum dispatch — drift-defending, monomorphized hot path; re-open only on out-of-tree-backend need or a post-SR1 prototype proving net complexity removal) |
| SR8 | Deepen or delete `EmbeddedPersistenceProvider` (abstracts almost nothing; own doc admits the migration contract lives on the concrete store). Resolve together with SR7. | `nimbus-storage/src/async_storage/traits.rs:16-20` | small | done (PR #145 merged 0e8ede679, 2026-07-07; EmbeddedPersistenceProvider deleted per AD3, grep-verified gone) |

### Band CO — Consolidations and shared primitives

| ID | Item | Where | Size | Status |
| --- | --- | --- | --- | --- |
| CO1 | Extract a generic `BackgroundWorker<T>`/`WorkQueue<T>` seam; subscription delivery, trigger candidates, and trigger execution supply only per-item closures (three hand-rolled `Mutex<VecDeque>+Condvar` scaffolds today). | `nimbus-engine/src/tenant/{subscription_delivery/{worker,queue}.rs, trigger_candidates.rs, trigger_execution.rs}` | large | done (PR #142 merged, 2026-07-07; dual-reviewed 3 rounds, 2 P1s + P2-follow-on + P3 fixed pre-merge) |
| CO2 | One operator-route authorization helper: collapse the triplicated `authorize_operator_*_route` flow + shared scope matchers + one parameterized audit recorder into `http/authz.rs`. | `nimbus-server/src/resource_control/{sessions.rs:272,445,463, services.rs:276,359,383, sandboxes.rs:128,193,224}` | medium | done (PR #144 merged, 2026-07-07; authz core + verb seam + HTTP-adapter mount seam; 1 test-quality fix) |
| CO3 | Generic `paginate_by_key(...) -> (page, PageMeta)` + one shared collection-metadata struct (three identical clamp/truncate/next-token blocks). | `sessions.rs:263`, `services.rs:74`, `sandboxes.rs:211` | small | done (PR #138 merged efbf56a30, 2026-07-07; http/pagination.rs shared helper) |
| CO4 | One private helper for the four near-identical service-lifecycle handlers (get/start/stop/restart differ only in manager verb + surface string). | `nimbus-server/src/http/services.rs:477-632` | small | done (PR #138 merged efbf56a30, 2026-07-07; ServiceLifecycleVerb helper) |
| CO5 | Callable adapter uses the canonical `StructuredHttpError::status()` mapping instead of re-matching every `Error` variant. | `adapters/cloud_functions/http/callable.rs:147-204` vs `error_envelope.rs:508` | small | done (PR #138 merged efbf56a30, 2026-07-07; canonical StructuredHttpError mapping + documented 6-variant legacy override) |
| CO6 | Split the mysql/postgres `write.rs` twins (1,499/1,486 prod lines of parallel logic): statement-building vs txn orchestration, sharing a SQL-write helper. | `nimbus-storage/src/{mysql,postgres}/write.rs` | medium | done (PR #141 merged 7b79aed10, 2026-07-07; sql/write_core.rs shared 6 txn methods + sql/row.rs; dialect-divergent kept per-backend) |
| CO7 | `now_millis()` + injectable `Clock` in `nimbus-core`; route the 27 inlined `duration_since(UNIX_EPOCH)` sites through it (unlocks deterministic time in tests; today only nimbus-services has a test clock). | 27 files across blob/bridge/cli/convex/core/crypto/dynamodb/fs/kv/license/s3/runtime | medium | done (PR #154 merged 92d84ffc8, 2026-07-08; Clock moved to nimbus-core (re-export shim), 17 sites swept, 5 census corrections). Follow-on semantic clock ownership, reliability, and cleanup completed in `archive/clock-architecture-reliability-plan.md`. |
| CO8 | Hoist `non_empty` into `nimbus-core` (owns `Error`); collapse the two evidence validators differing only in message text. | `nimbus-workloads/src/tenant.rs:706`, `nimbus-node/src/status.rs:663,671`, `host_lifecycle.rs:883` | small | done (PR #136 merged 4d5971779, 2026-07-07; non_empty hoisted to nimbus-core) |
| CO9 | Consolidate the three parallel `to_hex` helpers (nimbus-core helper or the `hex` crate). | `nimbus-blob/src/hash.rs`, `nimbus-engine/src/verification.rs`, `nimbus-sandbox/src/volume.rs` | small | done (PR #136 merged 4d5971779, 2026-07-07; hex_encode to nimbus-core (volume.rs was a delegate, not a dup)) |
| CO10 | Base64 audit: 10 per-crate wrappings — introduce shared named variants only where configs genuinely match. | artifacts, convex ×2, core, crypto, firebase ×2, mongodb, proxy, runtime | small | done (PR #149 merged, 2026-07-07; base64 helpers in nimbus-core) |
| CO11 | One backoff helper in `nimbus-sandbox` (three loops across oci/conmon/container backends). | `nimbus-sandbox/src/backends/{oci/network,conmon/lifecycle,container/runtime/lifecycle}.rs` | small | done (PR #149 merged, 2026-07-07; poll_until_deadline sandbox backoff) |
| CO12 | (Optional.) Collapse the nine per-RPC Firestore request-error enums into `FirestoreRequestError { rpc, kind }` if it reduces boilerplate without losing typed matching. | `nimbus-firebase/src/grpc/*` | medium | done (PR #149 merged, 2026-07-07; FirestoreRequestError collapse, 7-RPC table-driven test) |
| CO13 | De-duplicate the three Convex client classes: extract one coerce-and-delegate helper shared by `ConvexHttpClient`/`ConvexClient`/`ConvexReactClient` (~150 identical lines ×3). | `packages/convex/src/browser.ts:161,288,427` | small | done (PR #139 merged, 2026-07-07; mixin dedup / control-plane split / buf-generated protobuf + advisory-lock race fix) |
| CO14 | Split the control-plane `Nimbus` class + ~50 `NimbusService*`/`NimbusSandbox*`/`NimbusSession*` types out of the SDK barrel into `./control-plane`; keep `index.ts` a thin barrel (1,003 lines today). | `packages/nimbus/src/index.ts:32-433` | small | done (PR #139 merged, 2026-07-07; mixin dedup / control-plane split / buf-generated protobuf + advisory-lock race fix) |

### Band DE — Decomposition, naming, dead code

| ID | Item | Where | Size | Status |
| --- | --- | --- | --- | --- |
| DE1 | Decompose `nimbus-fs/src/object/mod.rs` (1,510 lines, 144 fns, 10 impls — an inline switchboard): move into concept-owned `read.rs`/`write.rs`/`range.rs` children, thin `mod.rs`. | `nimbus-fs/src/object/mod.rs` | medium | done (PR #151 merged, 2026-07-07; object/mod.rs -> read/write/range children) |
| DE2 | Split `nimbus-system/src/records.rs` (~1,398 prod lines, ~50 `*_async` fns spanning machine/deployment/source/subscription/scheduler/run domains) into `records/{domain}.rs` with shared helpers at the root. | `nimbus-system/src/records.rs` | medium | done (PR #151 merged, 2026-07-07; records.rs -> records/{domain}.rs) |
| DE3 | Split `nimbus-machine/src/lib.rs` (~735 lines, 5 unrelated concepts) into `roots.rs`/`paths.rs`/`image_source.rs`/`provider.rs`/`state.rs`; move `describe_machine_image_source` from nimbus-system onto the type as `Display`/`as_source_string()` (drift-prone inverse of `parse`). | `nimbus-machine/src/lib.rs:50-733`, `nimbus-system/src/records.rs:1362` | medium | done (PR #151 merged, 2026-07-07; machine lib.rs -> 5 concept files + as_source_string) |
| DE4 | Fold `nimbus-firestore` (93-line stub) into `nimbus-firebase` (or nimbus-core if genuinely shared with cloud-functions); delete the crate. | `crates/nimbus-firestore` | small | done (PR #151 merged, 2026-07-07; nimbus-firestore folded into nimbus-core, crate deleted) |
| DE5 | Rename the CLI JS-runtime module `node.rs` → `node_runtime.rs` (or `js_runtime.rs`) to stop conflating Node.js tooling with cluster-node modules (`node_service.rs`, `node_workload_executor.rs`). | `nimbus-cli/src/node.rs` | small | done (PR #136 merged 4d5971779, 2026-07-07; node.rs -> node_runtime.rs) |
| DE6 | Replace the hand-rolled HTTP/1.1-over-unix-socket parser in the machine guest-API client (~250 lines: `read_unix_http_request`, `parse_http_json_body`, …) with a unix-socket-capable HTTP client or the shared decoding helper `local_server_client.rs` already uses. Sequence BEFORE `windows-machine-support-plan.md` WIN4, which adds a Windows named-pipe transport to this same client. | `nimbus-cli/src/machine/client.rs:321-536` | medium | done (PR #140 merged e60ece530, 2026-07-07; hyper 0.14 UDS client, WIN4-ready transport seam) |
| DE7 | Audit `nimbus-cli/src/machine/stub` (heaviest `#[allow]` cluster: 16 allows) — wire it or delete it; pre-launch prefers deletion of dormant stubs. Coordinate with `windows-machine-support-plan.md` WIN2, which realizes these stubs as `cfg(windows)` modules: wire (don't delete) any stub WIN2 will consume, or record that WIN2 re-creates them fresh. | `nimbus-cli/src/machine{,/stub}` | medium | done (PR #140 merged e60ece530, 2026-07-07; stubs kept (WIN2-owned), per-item allow cleanup; Windows dead-code fix) |
| DE8 | Per-adapter `resolve(env, app_dir) -> AdapterEnablement` units under `start/adapters/`; the composition root iterates them (683-line resolver switchboard today). | `nimbus-cli/src/start/adapters.rs:222-574` | small | done (PR #140 merged e60ece530, 2026-07-07; start/adapters/ per-adapter modules, test module verbatim) |
| DE9 | Concept-owned renames: `cli machine/manager/helpers.rs` (→ `helper_paths.rs` + move env lock), `nimbus-convex/src/templates/helpers.rs` (→ `render.rs`), `nimbus-runtime/src/runtime/helpers.rs`; verify `adapters/convex/handlers/common.rs` is genuinely shared or rename. | four files | small | done (PR #155 merged 120e0485f, 2026-07-08; concept renames at ownership boundaries) |
| DE10 | Suppression sweep: strip stale `unused_imports` allows (15 repo-wide), unexplained clusters in `cli/compose` (7) and `runtime/backends/v8` (6); remove file-scope `#![allow(dead_code)]` in `triggers/dispatch.rs:1` + `persistence/tenant/trigger_delivery.rs:1` (masks live code); fix `adapters/mod.rs` pub/pub(crate) inconsistency + mongodb module-level allows; replace `too_many_arguments` suppressions (13) with params structs where natural. | repo-wide | small | done (PR #155 merged 120e0485f, 2026-07-08; stale-suppression sweep, workspace -D warnings clean) |
| DE11 | Collapse the subscription-metrics triple-hop pass-through facade; replace the five `subscribe*` overload ladder with an options struct; unify `list_cron_jobs*`/`load_cron_jobs*` aliases. | `nimbus-engine/src/{engine/mutations/commit_processing.rs:118-167, tenant/subscription_delivery_facade.rs, engine/subscriptions.rs:118-311, engine/scheduler/cron.rs:82-92}` | small | done (PR #142 merged, 2026-07-07; dual-reviewed 3 rounds, 2 P1s + P2-follow-on + P3 fixed pre-merge) |
| DE12 | Peel the self-contained `TenantCredentialProjection{Policy,Scope,Request,Binding}` cluster out of `nimbus-workloads/src/tenant.rs` (714 prod lines) into `tenant/credential_projection.rs`. | `nimbus-workloads/src/tenant.rs:376-551` | small | done (PR #155 merged 120e0485f, 2026-07-08; credential-projection peel) |
| DE13 | Fold the 8-line `nimbus-assets/src/integrity.rs` (`sha256_hex`, single caller) into `js_packages.rs`. | `nimbus-assets/src/integrity.rs` | small | done (PR #136 merged 4d5971779, 2026-07-07; integrity.rs folded into js_packages.rs) |
| DE14 | Extract one `write_firebase_functions_app(dir, index_ts)` test helper (seven near-identical fixture writers inflate the file to 1,474 lines). | `adapters/cloud_functions/execution.rs:1056-1396` | small | done (PR #138 merged efbf56a30, 2026-07-07; write_firebase_functions_app helper (6 of 7 writers)) |
| DE15 | Server misc: stop fabricating `TenantId::new("invalid-tenant")` for audit signatures (carry the raw string); standardize `crate::tenant::` import style in `resource_control/sandboxes.rs`; fix `ws/negotiation.rs` `Result<(),()>` return + double frame-type check. | `resource_control/services.rs:407-411`, `resource_control/sandboxes.rs:5,100`, `ws/negotiation.rs:121,205,215` | small | done (PR #138 merged efbf56a30, 2026-07-07; raw tenant audit string, import style, SendHelloError, dead-check removal) |
| DE16 | Stop committing generated Firestore protobuf (15.7k lines under `packages/firebase/src/gen/`): generate at build time via the existing `@bufbuild/protoc-gen-es` dev dep, gitignore `gen/`. | `packages/firebase/src/gen/**` | medium | done (PR #139 merged, 2026-07-07; mixin dedup / control-plane split / buf-generated protobuf + advisory-lock race fix) |

### Band TI — Test infrastructure

| ID | Item | Where | Size | Status |
| --- | --- | --- | --- | --- |
| TI1 | Adopt `EngineFixture` in nimbus-system tests (10+ open-coded `Arc::new(Engine::new(...))` sites; add the nimbus-testing dev-dep). | `nimbus-system/src/tests.rs:109,152,220,293,387,491,554,655,751,794` | small | done (PR #147 merged 7c00f35f9, 2026-07-07; EngineFixture in nimbus-system tests) |
| TI2 | Add shared control-plane fixture builders to nimbus-testing (PrincipalContext / LocalEnforcementBinding / reconciler + status-evidence scaffolding re-rolled inline by node/system/machine/workloads today). | `nimbus-testing/src/lib.rs` + consumers | medium | done (PR #147 merged 7c00f35f9, 2026-07-07; tenant_isolation_fixture in nimbus-testing, 5 consumers migrated) |
| TI3 | Add a counted/one-shot `FaultInjector` variant ("fail the Nth call") beside `BlockingFaultInjector`. | `nimbus-testing/src/faults.rs` | small | done (PR #142 merged, 2026-07-07; dual-reviewed 3 rounds, 2 P1s + P2-follow-on + P3 fixed pre-merge) |
| TI4 | Delete the `#[cfg(test)]` synchronous scheduler twins (~110 lines maintained twice); drive tests through `tick_at_async` on a current-thread runtime. | `nimbus-engine/src/scheduler.rs:73-183,259-334` | small | done (PR #142 merged, 2026-07-07; dual-reviewed 3 rounds, 2 P1s + P2-follow-on + P3 fixed pre-merge) |
| TI5 | One shared test-only `PauseBarrier` (armed/entered/released condvar barrier duplicated ~3×). | `tenant/subscription_delivery/pause.rs:9-92`, `tenant/trigger_candidates.rs:338-416`, `subscriptions/registry.rs:82-168` | small | done (PR #142 merged, 2026-07-07; dual-reviewed 3 rounds, 2 P1s + P2-follow-on + P3 fixed pre-merge) |
| TI6 | Stabilize the nimbus-engine timing/concurrency test-flake family. Across the 2026-07 campaign these flaked CI intermittently while passing 3-8/8 locally on clean rebuilds (PRs that touch ZERO engine files still hit them): `materialized_serving::concurrency::{concurrent_first_load_only_publishes_caught_up_newest_materialized_table, paused_first_load_catches_up_before_publication}`, `subscriptions::basic::subscription_snapshots_expose_covered_sequence_and_commit_timestamp_metadata`, `cooperative_*_multiple_parked_runtimes`, `engine_update_and_delete_drive_subscription_updates`. Root-cause the shared timing assumption (likely a fixed sleep/deadline in `nimbus-testing/eventual.rs` too tight under CI shard load, or a publish/catch-up race in the materialized-serving harness) and make them deterministic (barrier/await on observable state, not wall-clock). | `nimbus-engine/src/tests/{materialized_serving,subscriptions}`, `nimbus-testing/src/eventual.rs` | medium | done (PR #156 merged c70ae8aca, 2026-07-08; root cause = trigger-feed cursor-advance commits share the document sequence space; settle/pause barriers, honest 40/40 stress w/ run-count guard; 3-round review: 2 P1 test-logic bugs fixed, 2 P2 production follow-ups -> TI7/TI8) |
| TI7 | Engine follow-up from the TI6 review (efficiency, not correctness): the trigger feed's zero-write cursor-advance commits bump `required_sequence` without flowing through `MaterializedServingBackend::apply_commit`, so a query racing one forces a spurious warmed-table reload (results stay correct; the reload is wasted work — this production behavior WAS the reuse-test flake). Fix at the materialized-serving boundary: advance coverage through zero-write commits, or derive `required_sequence` from the document-applied head. Then assert warmed reuse with the trigger feed running. | `nimbus-engine/src/tenant/materialized_reads/`, `tenant/trigger_candidates.rs` | small | done (PR #162 merged cd3a77b95, 2026-07-08; zero-write coverage widening, floor-guarded fail-closed; reuse tests run with trigger feed live) |
| TI8 | Engine follow-up from the TI6 review (metadata quality, not correctness): a zero-write cursor-advance commit landing in the subscribe bootstrap→activation gap coalesces with the next real document commit, and coalescing intentionally drops per-commit identity (`engine/mutations/commit_processing.rs` "Coalesced batches intentionally omit...") — so a lone real commit can lose its `snapshot.commit` metadata to an internal empty commit. Exclude zero-write commits from coalescing identity so a single real commit keeps metadata. | `nimbus-engine/src/engine/mutations/commit_processing.rs`, `subscriptions/delivery.rs` | small | done (PR #162 merged cd3a77b95, 2026-07-08; kind-aware commit identity at provider catch-up; conservative queue merge; + bootstrap catch-up gap fix and pinned-latest retention from the 5-round review loop) |

### Band UI — nimbus-ui hygiene

| ID | Item | Where | Size | Status |
| --- | --- | --- | --- | --- |
| UI1 | Split the storage-table god-file (1,154 lines, 10 state slices, 9 inline sub-components): extract a `useTableDocuments` pagination hook + move drawers/panels/cells to `components/storage/`. | `routes/developer/storage_.$table.tsx` | medium | done (PR #143 merged 166706678, 2026-07-07; typed mutation client + storage decomposition (1154->368) + data-loading contract; 2 write-path fidelity fixes) |
| UI2 | Typed mutation client `lib/api-mutations.ts` returning `{ok,data}|{error}`; route the 7 raw-`fetch` write sites through it. | `storage_.$table.tsx:96-244,857`, `operator/machines.tsx:137-155`, `danger-zone.tsx`, `settings/hooks.ts`, `function-runner.tsx`, `shell/tenants-fetch.ts` | medium | done (PR #143 merged 166706678, 2026-07-07; typed mutation client + storage decomposition (1154->368) + data-loading contract; 2 write-path fidelity fixes) |
| UI3 | Delete the ~10 per-route `Loading`/`Empty` copies; standardize on the existing `components/empty-state.tsx` + `loading-cell.tsx` (reconcile `detail`→`body`). | ~10 route files | small | done (PR #137 merged 7144c6338, 2026-07-07) |
| UI4 | Extract `components/Slideover` (overlay + Escape + header) and a `JsonEditorForm`; callers pass only a submit handler (drawer/panel/JSON-textarea re-hand-rolled per route). | `storage_.$table.tsx:667-1108`, `compute_.$function.tsx` | small | done (PR #137 merged 7144c6338, 2026-07-07) |
| UI5 | Shared doc types: move duplicated `FunctionDoc`/`TableDoc` declarations to `lib/types/` or standardize on generated `Doc<"...">`. | `compute.tsx:31`, `compute_.$function.tsx:65`, `storage.tsx:21`, `storage_.$table.tsx:28`, `operator/tenants.tsx:23` | small | done (PR #137 merged 7144c6338, 2026-07-07) |
| UI6 | Document one data-loading default (reactive `useQuery`; `loader:` only where preloading matters) and migrate the `useEffect`+`fetch` outliers. | `storage_.$table.tsx:143`, `graph-view.tsx`, `compute_.$function.tsx:333-443` | medium | done (PR #143 merged 166706678, 2026-07-07; typed mutation client + storage decomposition (1154->368) + data-loading contract; 2 write-path fidelity fixes) |
| UI7 | Split `operator/machines.tsx` (666 lines): `MachineDetail` component + `useMachineActions` hook; drop local Loading/Empty per UI3. | `routes/operator/machines.tsx` | small | done (PR #137 merged 7144c6338, 2026-07-07) |

### Band DS — Docs and spec truth

| ID | Item | Where | Size | Status |
| --- | --- | --- | --- | --- |
| DS1 | Fix the ARCHITECTURE.md crate table: add the 7 missing crates (`nimbus`, `nimbus-cli`, `nimbus-fs`, `nimbus-kv`, `nimbus-object-storage`, `nimbus-s3`, `nimbus-workloads`), correct the `nimbus-bin` row (5-line entrypoint → `nimbus-cli`), or drop the "All workspace members" claim. | `ARCHITECTURE.md:13-64` | small | done (direct-to-main 7d60ca6c7, 2026-07-07; 9 missing crates added, 41==41, source-map.md stale citations fixed) |
| DS2 | Document the workload-identity ladder (AD2 table: `WorkloadId` / `WorkloadIdentity` / `TenantWorkloadId`, and why minting is admission-anchored) in ARCHITECTURE.md's tenancy/auth sections. | `ARCHITECTURE.md` | small | done (direct-to-main 7d60ca6c7, 2026-07-07; workload-identity ladder table in ARCHITECTURE.md) |
| DS3 | Reconcile `storage-seams-architecture.md` with shipped code: Seam B is sync capability-trait shape (spec says async RPITIT); BlobGc lacks §9b write-intent pins / seal-before-enumerate / seam-generic sweep — implement or amend the spec, one decision per seam. Seam E `VolumeProvider` EXISTS (`nimbus-sandbox/src/volume.rs:117`, `LocalDirVolume` :149) matching spec §16b — the first-pass "absent" finding was wrong (searched only fs/s3/blob); remaining Seam E work (parity confirmation, GC/placement wiring, further volume backends) is owned by `nimbus-sandbox-plan.md`. Also truth-up the spec's §16c crate inventory (blob/fs/s3 shipped; object plane crate is `nimbus-object-storage`, not `nimbus-objectfs`; NOS/NFS/NC archived) and §12 topology. | `docs/private/plans/storage-seams-architecture.md` §9b/§12/§16b/§16c, `nimbus-storage/src/traits/object_metadata.rs:551`, `nimbus-blob/src/gc.rs:54`, `nimbus-sandbox/src/volume.rs:117` | small | done (PR #141 merged 7b79aed10, 2026-07-07; storage-seams-architecture.md truthed up; BlobGc gap -> GR9) |
| DS5 | BPD verifier drift: `scripts/verify-binary-embedded-package-distribution.sh` fails conditions 10/15 on unmodified main — its file pins (`crates/nimbus-bin/src/{node,codegen}.rs`) went stale in an earlier crate-seam refactor (paths repaired to nimbus-cli in the small-sweeps PR) and its content checks (caret-range fixtures, diagnostic-only classification strings) no longer match shipped code. Re-derive the archived BPD plan's proof conditions against current code or retire the script with a note. (Surfaced by the small-sweeps review, 2026-07-07.) | `scripts/verify-binary-embedded-package-distribution.sh` | small | done (direct-to-main 7d60ca6c7, 2026-07-07; 16 stale path pins + macOS grep symlink quirk fixed, 17-fail->1; condition 22 is by-design BPD_FULL forward-gap) |
| DS4 | Feature-flag notes: `nimbus-fs fuse` (confirm CI exercises it or gate), `nimbus-blob cluster` (mark deferred so it isn't mistaken for dead), `nimbus-engine test-hooks` (verify it can't leak into release builds). | three Cargo.tomls + docs | small | done (direct-to-main 7d60ca6c7, 2026-07-07; fuse dormant, test-hooks dev-only, cluster noted) |

### Band RR — 2026-07-21 full-review remediation

This band owns the remediation of the private full-codebase review performed
against `main` at `9a40b60a4`. The report's 28 lane findings normalize to 25
unique items because three findings were independently reported by two lanes.
Every unique item remains visible here, including already-fixed and refuted
claims, so completion cannot hide work by omission. The implementation branch
was rebased onto updated `origin/main` at `c789db2fd`. RR26 records one additional
private-fence regression discovered by the required docs-site completion gate.
RR27 through RR29 record three release-readiness findings verified on
2026-07-23 after the original remediation branch landed.

RR30 owns the storage semantic-type residual routed from the 2026-08-19 IMV
and BLI review. RR31 owns the cross-adapter numeric query and index semantics
residual from the same review.

| ID | Item | Where | Severity / verdict | Status |
| --- | --- | --- | --- | --- |
| RR1 | Make DynamoDB single-item put/update/delete condition evaluation and read-modify-write atomic. | `nimbus-dynamodb/src/commands/{item,transact}.rs` | high / confirmed | done (`concurrent_add_updates_retry_from_a_fresh_snapshot_without_lost_writes` plus single-item stream rollback; workspace 4,674/4,674) |
| RR2 | Preserve Firestore Timestamp, GeoPoint, Bytes, Reference, and special-double values through write/read round trips. | `nimbus-firebase/src/serializer.rs` + REST/gRPC lowering | high / confirmed | done (Firebase 66/66 plus REST, gRPC, nested patch, persistence, and Mongo projection round trips) |
| RR3 | Authorize the outer HTTPS CONNECT at L4 and enforce method/path rules on the decrypted request without permitting splice bypass. | `nimbus-proxy/src/request.rs` + CONNECT classification/tests | high / confirmed | done (focused decrypted method/path interception and splice-denial tests; workspace 4,674/4,674) |
| RR4 | Alt-Svc/HTTP-3 forward-proxy bypass claim. | `nimbus-proxy/src/pingora_app.rs`, sandbox egress pin | info / refuted | no-action (sandbox netns default-drops UDP; QUIC cannot bypass the PEP) |
| RR5 | Mongo `_id` fast-path `$ne`/null divergence claim; separately correct shared Mongo missing-field compatibility semantics if confirmed. | `nimbus-mongodb/src/commands/crud/filter.rs`, engine matcher | info / fast-path claim refuted | done (fast-path claim refuted; confirmed missing/null gap fixed with end-to-end `$eq`/`$ne`/`$exists` coverage and `_id` fast-path regression test) |
| RR6 | Restore the complete workspace inventory by documenting `nimbus-compute`. | `ARCHITECTURE.md` | medium / confirmed | done (workspace inventory and compute-plane ownership corrected; both docs gates green) |
| RR7 | Keep internal diagnostic strings out of external error envelopes and retain server-side correlation. | HTTP and runtime-host error envelopes | low / confirmed | done (HTTP and tenant-visible runtime-host internal errors are redacted with correlation IDs; hosted Node canaries then exposed and now cover typed timeout and pending-promise-stall envelopes end to end without reopening generic diagnostics) |
| RR8 | Consolidate the duplicated Convex tenant/team-binding authorization preamble. | Convex registry/auth + httpAction dispatch | low / plausible | done (duplication confirmed; canonical registry authorization helper covers handler and httpAction dispatch paths; server suite green) |
| RR9 | Fail startup loudly when a default-on MongoDB, DynamoDB, or S3 listener cannot bind its conventional port. | `nimbus-cli/src/start/adapters/{mongodb,dynamodb,s3}.rs` | low / high confidence | done (`busy_default_listener_port_fails_boot` covers all three adapters; workspace 4,674/4,674) |
| RR10 | Make the SQLite invariant-bypassing insert helper unavailable to production code. | `nimbus-storage/src/sqlite/write.rs` | low / high confidence | done (helper restricted to tests; storage and workspace suites green) |
| RR11 | Cover every compiled vendored dependency in top-level attribution and its verification gate. | `NOTICE`, `third_party/`, attribution scripts | low / high confidence | done (attribution helper 11/11 and full attribution gate green) |
| RR12 | Make TypeScript typechecking a required local and hosted CI gate. | `Makefile`, `.github/workflows/ci.yml` | low / high confidence | done (required workspace typecheck wired into local/hosted CI; all workspace typechecks green) |
| RR13 | Compare KV credentials in constant time. | `nimbus-kv/src/server.rs` | low / high confidence | done (username/password comparisons scan all bindings in constant time; KV auth tests and workspace suite green) |
| RR14 | Bound in-flight native `/ws` subscription registration per connection. | `nimbus-server/src/ws/socket/session.rs` | low / high confidence | done (`websocket_bounds_pending_subscription_registration_tasks` proves the 32-task cap and retryable rejection) |
| RR15 | Remove Convex-prefixed wire IDs and error wording from the canonical Nimbus JS SDK. | `packages/nimbus/src/browser.ts` | low / high confidence | done (Nimbus-native connection/request IDs and wording; Nimbus/Convex selftests and workspace JS build/typecheck/test green) |
| RR16 | Supply the landing-page Sandboxes tab glyph. | `website/src/styles/custom.css` | low / high confidence | no-action (already fixed on current main by PR #223) |
| RR17 | Make the unsigned, honor-system license posture and expired-entitlement behavior internally consistent and explicit. | `nimbus-license`, `LICENSING.md` | low / high confidence | done (`expired_license_snapshots_do_not_report_active_entitlements`; honor-system enforcement and legal posture documented) |
| RR18 | Pin the cargo-deny version used by hosted CI. | `.github/workflows/ci.yml` | low / high confidence | done (hosted install pinned to 0.19.0; `cargo deny check` green) |
| RR19 | Remove archived plan entries and reconcile status vocabulary/current wording. | `docs/private/plans/README.md` | low / confirmed | done (archived entries removed and vocabulary/current-owner wording reconciled; docs gates green) |
| RR20 | Repoint stale `nimbus-bin` ownership paths to `nimbus-cli`. | `ARCHITECTURE.md`; private architecture index if present | low / confirmed in root doc | done (live architecture/plan references corrected; docs gates green) |
| RR21 | Replace the stale Mongo M9 module description with the implemented bound/unbound tenant modes. | `nimbus-mongodb/src/commands/tenant.rs` | low / high confidence | done (module contract now documents implemented bound/unbound modes; docs and workspace gates green) |
| RR22 | Correct the website comment from five to six published doc groups. | `website/astro.config.mjs` | low / high confidence | no-action (already fixed on current main by PR #223) |
| RR23 | Document and test the fresh durable-journal probe required by ambiguous-outcome classification. | engine durable outcome + storage trait/provider tests | low / confirmed | done (classifier uses authoritative `recover_durable_journal`; engine recovery/provider suites green) |
| RR24 | Make the crossbeam-epoch deny express the entire unsafe range promised by its comment. | `deny.toml` | info / confirmed | done (deny range covers the promised versions; `cargo deny check` green) |
| RR25 | Delete the pre-launch consumer-compat-only `EgressEnforcementMode::LaunchMetadata` path. | `nimbus-egress`, CLI label/tests | info / confirmed | done (`sandbox_egress_enforcement_plan_rejects_removed_launch_metadata_mode` and CLI boundary regression green) |
| RR26 | Restore the docs private fence after runtime evidence, review history, and Node proof artifacts were reintroduced at legacy public paths. | `docs/private/{architecture,code-review,plans}`, live Node tooling/references | low / confirmed by docs-site gate | done (legacy public trees removed, live references and generators repointed; the public Node support fallback now consumes only published projections; 108-page docs check, 17/17 site verifier, both generator checks, and the 9-pass/1-private-skip/0-fail public fallback gate are green) |
| RR27 | Bind Convex WebSocket reauthentication to the URL-selected silo verifier from the same deployment snapshot that admitted the socket. | `nimbus-convex/src/silo_auth.rs`, server Convex registry/socket-auth/subscription seams | high / confirmed by fail-before cross-silo bearer acceptance | done (`ConvexSiloAuthAuthority` and `ConvexSocketAdmission` make the selected trust domain a snapshot-bound capability; four silo-auth unit tests plus the cross-silo rejection and existing identity WebSocket tests are green) |
| RR28 | Restore the required Convex silo in the deploy-admin authorization fixture so the test reaches the security boundary it asserts. | `nimbus-server/src/tests/local_server_security.rs` | low / confirmed by fail-before 400 response | done (`deploy_admin_requires_local_admin_header_even_with_deploy_bearer` now reaches the valid deploy path and passes without weakening its authorization assertions) |
| RR29 | Bind the Cloud Functions runtime-owner conformance fixture to its explicit HTTP tenant. | `nimbus-server/src/tests/runtime_owner_conformance.rs` | low / confirmed by fail-before 409 response | done (`cloud_functions_passes_runtime_owner_lifecycle_conformance` exercises the lifecycle contract through a valid trusted tenant binding and passes unchanged assertions) |
| RR30 | Own IMVR1. After IMV1 and BLI3 merge, inventory storage durable outcomes and provider-capability types. Classify each type as an opaque validated value, a closed enum with no invalid state, or a repair. Preserve valid closed enums. Add a nonterminal repair row for every constructible invalid state before closing the audit. Acceptance: `rr30-storage-semantic-types.md` records every type, construction path, owner, verdict, and test or source-gate obligation with no unowned repair. | `crates/nimbus-engine/src/{engine/mutations/durable_outcome.rs,persistence/**}`, `crates/nimbus-storage/src/{async_storage/traits.rs,diagnostics.rs,traits/**}` | architecture / confirmed residual | deferred(IMV1 and BLI3 merged) |
| RR31 | Own IMVR2. After IMV1 merges, inventory numeric equality and ordering across Nimbus, Convex, Firestore, MongoDB, and DynamoDB query and index paths. Define one engine-owned query and index semantics contract. Do not infer stored index meaning from request transport. Preserve one universal encoding only if conformance proves the covered contracts identical. Add a nonterminal repair row for each incompatible path. Acceptance: `rr31-numeric-index-semantics.md` records every index creation path, query path, equality rule, ordering rule, large-integer bound, selected semantics, and test obligation. No adapter can silently use an incompatible index. | `crates/nimbus-core/src/{query.rs,schema.rs}`, `crates/nimbus-storage/src/index/**`, `crates/nimbus-engine/src/engine/queries/**`, Convex, Firestore, MongoDB, and DynamoDB adapter query tests | architecture / confirmed residual | deferred(IMV1 merged) |

Completion gate: every RR row is `done` or `no-action (reason)` with focused
evidence, `cargo fmt --all --check`, `make clippy`, `make ci`, both docs gates,
and the structured closeout review are green before the branch is pushed.

Closeout evidence (2026-07-21): the initial remediation state passed `make ci`.
After rebasing through the current `origin/main` at `c789db2fd`, the required
component set passed again, with the workspace lane bounded to two test threads
because other worktrees were concurrently loading the shared host: 493 runtime
tests, 4,674/4,674 runnable workspace tests (31 ignored; 4,705 inventoried),
381/381 storage tests (2 external-provider skips), the required verification
harness, all workspace JS build/typecheck/test lanes (51 UI files / 336 tests),
and proof helpers. The exact storage PITR performance regression also passed
independently; the final clock-integration conflict surfaces passed 71/71
Firebase and 16/16 atomic-write-batch focused tests.
`scripts/check-docs.sh` passed all 108 public pages and
`scripts/verify-nimbus-docs-site.sh` passed 17/17 conditions. The first Opus
4.8 code review found stale Firestore typed-value sidecars in unmasked
`MergeAll` writes; the shared typed field setter now clears replaced sidecars,
the focused regression passed, and the rerun was clean. The first Opus docs/CI
review found that the public Node support fallback still depended on private
artifacts, that the posture generator wrote the same private target twice, and
that the publication generator still read legacy public source paths; all three
were confirmed and fixed. Its follow-up found one alphabetical workspace-table
ordering defect, which is also fixed. The final Opus 4.8 docs/CI rerun was clean
with no accepted or actionable findings. Its two explicitly non-actionable
observations were also checked: `backup_api` no longer exists in the license
surface, and the narrower public Node projection intentionally makes its
Node24-specific zero-gap check overlap the preceding all-version assertion.

Release-readiness follow-up evidence (2026-07-23): RR27 through RR29 each
failed deterministically before repair and passed after repair. The final
`make ci` run passed 517 runtime tests (134 intentionally ignored), 4,853
workspace tests (36 skipped), the required deterministic verification harness,
all workspace JavaScript build/typecheck/test lanes (51 UI files / 336 tests),
and the release/install proof helpers. Workspace Clippy first exposed an
eight-argument socket entrypoint created by the security fix; the final design
collapsed the correlated values into the concept-owned `ConvexSocketAdmission`
capability and the rerun passed with warnings denied. The independent Opus 4.8
high-reasoning structured review found no accepted or actionable findings.

### Band CP — `nimbus-compute` extraction (AD1, staged)

| ID | Item | Where | Size | Status |
| --- | --- | --- | --- | --- |
| CP1 | Create `crates/nimbus-compute` and move the transport-free modules as-is: `execution/`, `artifact_verifier_effects`, `machine_lifecycle.rs`, the `service_manager` binding shim. Acceptance: crate has zero axum/http-transport deps (Cargo.toml proof); server re-exports compile; `make ci` green. | new crate + `nimbus-server/src/{execution/,artifact_verifier_effects*,machine_lifecycle.rs,service_manager.rs}` | medium | done (PR #157 merged 639994b39, 2026-07-08; nimbus-compute created, execution/artifact_verifier_effects/machine_lifecycle/service_manager moved, subscriptions.rs error seam -> nimbus_core::Error, acyclic by cargo metadata, autoreview clean 1st pass) |
| CP2 | Split `AppState`: `ComputeState` (engine + registries, zero axum types) lives in nimbus-compute; server `AppState` wraps it plus transport concerns. `construction.rs` splits the same way. | `nimbus-server/src/{state.rs,construction.rs}` | medium | done (PR #161 merged 76dc8fda5, 2026-07-08; ComputeError+ComputeState per amended spec, Deref zero-churn, autoreview clean; 1 CI red = harness V8 seeded-action flake, 3/3 local repro passes, cleared on rerun) |
| CP3 | Migrate the orchestration halves of the deploy/services/sandboxes/scheduling/machines handlers into compute-owned functions; axum handlers reduce to extract → call → respond. Coordinate with SR3/SR4 and CO2–CO4 (do the consolidations first or fold them in). Optionally point `nimbus-cli` start/dev wiring at nimbus-compute directly. | `nimbus-server/src/http/{deploy,services,sandboxes,sandbox_spec,scheduling,machines}.rs` | large | done (PR #163 merged 336e5c6a5, 2026-07-08; deploy 588->16, 23 compute fns across 5 handler files, no-shims sweep, 4-round review clean; 2 CI reds attributed to known flake families, both green on the rebased fresh run) |

### Band SA — 2026-08-25 storage adversarial review

This band owns the corrected ledger of the 2026-08-25 full storage adversarial
review against `main` at `9c807015a`: seven parallel review lanes (Nimbus
ground-truth map, post-SIC delta, celld v0.1.0→v0.2.1 delta, object-store
exemplars, durability exemplars, Turso + Convex deep dive, web research), a
direct-code verification pass, and two rounds of second-model adversarial
cross-adjudication (Codex gpt-5.6, clean worktree at the same commit) in which
claims in both directions were re-verified against the code before acceptance.
The full report — including exemplar validations of current Nimbus design, the
adopt/do-not-adopt lists, and the low/advisory set — is archived at
`proof/architecture-review-2026-07/storage-adversarial-audit-2026-08.html`.
Refuted claims stay visible here so they are not re-filed. Rows that propose
scope additions to IMV, BLI, or NKV are triage inputs; the owning plan accepts
or declines them, and this band tracks each item until an owner does.

The review's strongest scheduling signal is not a row: BLI is still `proposed`
while its highest-stakes predicted defect (BLIF1 shared-hash release data
loss) is live in shipped code. Promotion is a roadmap decision for
`plans/README.md`, recorded here as urgency evidence only.

| ID | Item | Where | Severity / verdict | Status |
| --- | --- | --- | --- | --- |
| SA1 | Fix the three confirmed nimbus-s3 wire defects and add an S3-conformance lane: ListObjectsV2 sets the continuation token to the first unemitted key but the next page skips keys `<=` token, silently losing one object per page boundary; DeleteObject ignores the If-Match condition family and deletes unconditionally; CompleteMultipartUpload persists the client-supplied CRC64NVME without verification and later replays it on checksum-mode GET. Conformance lane covers the `IsTruncated ⟺ token` biconditional, conditional delete (412 before 404), and checksum recompute-or-reject. Record the 400 on modern SDK default checksum headers as an explicit compatibility decision (stock AWS SDKs break today). Engine-side condition machinery is correct; these are adapter-layer translation defects. | `crates/nimbus-s3/src/service.rs:321-331,274-290,626,1031-1054` | high / confirmed | todo |
| SA2 | Close the two convex.rs object-surface mediums: a fresh ULID per store orphans a manifest when the same logical file is stored concurrently, and every operation performs an O(n) full-bucket list. | `crates/nimbus-s3/src/convex.rs:333-336,534-546` | medium / confirmed | todo |
| SA3 | Fence the embedded default deployment: exclusive advisory data-dir lock over the Engine persistence and control roots (fs2 is the workspace lock crate), including the encrypted redb backend, whose custom `EncryptedFileBackend` opens with plain `OpenOptions` plus an in-process mutex and so bypasses redb's native file lock exactly where encryption-at-rest is on. `requires_committer_lease()` is false for Redb and SQLite, so nothing else fences two processes on one data dir. `LocalPackStore` already takes an exclusive root flock — the pattern stops one directory short. Add a cross-process refusal test. Do not add lease machinery. | `nimbus-engine/src/engine/bootstrap.rs`, `nimbus-engine/src/persistence/tenant/committer_lease.rs:19-31`, encrypted redb backend | high / confirmed | todo |
| SA4 | Design commit-log and MVCC retention as its own pre-launch contract: the commit log is never truncated, `retention.rs` pruning is unit-tested (`retention.rs:528` pins hard-delete denial under the floor) but wired to no production lifecycle so `durable_journal_cursor_floor()` is always 0, and PITR replays from genesis so restore cost grows without bound. BLI is blob-plane only; this metadata half has no owner plan, so triage creates a new owner plan. Design template: Convex four-window retention, prune cutoff clamped to the dependent structure's confirmed-deleted checkpoint, durable bound conservative / memory bound aggressive, fail-closed on trimmed history, per-page validation after each read. Touches HS. | `nimbus-storage/src/retention.rs`, commit log, PITR paths | high / confirmed gap | todo |
| SA5 | Erasure parity byte-stability hardening: the exact parity bytes `reed-solomon-simd` emits are durable format (shard hash is storage key and manifest `ShardRef`; heal re-encodes and demands byte equality), but the `=3.1.0` pin carries no comment and the recorded rationale is supply-chain, not byte stability, and no golden parity vector exists (all tests re-encode in-process, so CI stays green across an upgrade that strands every pre-upgrade heal). Add the pin comment and a checked-in golden parity vector; a manifest codec-id field is optional under a rule that any codec change requires an NBLE2 format bump. Aggravator: upstream guarantees cross-version byte compatibility only for shard sizes divisible by 64, and Nimbus accepts other even final shards. | `crates/nimbus-blob/Cargo.toml:31`, `erasure/{store,heal,manifest}.rs` | medium (downgraded from high) / confirmed | todo |
| SA6 | Scrub/heal/rollback error classification: any `Err` from verification — including transient I/O — currently becomes a `HashMismatch` finding plus a quarantine push, heal reports "beyond repair" for any mismatch cause, and the erasure rollback preimage treats every read error as absence (a transient read failure can later unlink a committed replica). Rule: only successful-read-wrong-bytes is corruption; only `NotFound` means absent. Proposed to the BLI owner to land with sweep work — quarantine feeding GC decisions turns misclassification into a deletion hazard; scope addition needs owner acceptance. rustfs hit exactly this (`00519714`). | `nimbus-blob/src/scrub.rs:1375-1393`, `scrub/rebuild.rs`, `erasure/manifest.rs:204-207` | medium (downgraded from high) / confirmed | todo |
| SA7 | Pre-#303 archive restore diagnostics: restore of a v1-position archive fails safely but dies on a raw serde error before the container-version check, because the nested position decode runs first. Fix is header-first parsing (read and check the container version before decoding any payload), then an operator-legible message naming the digest-codec change. Diagnostic only; no unsafe restoration exists. Rides IMV. | backup archive + PITR bundle readers | medium / confirmed | todo |
| SA8 | Correct negative-zero digest normalization after merged IMV1: `write_f64` collapses −0.0 to +0.0 and the `SpecialDouble::NegativeZero` leaf digests identically to plain `0.0`, while the stored scalar model returns `"-0"` as a distinct client-visible sentinel. IMV-D7's "one coordinate" rule applies to GeoPoint. Decide scalar and GeoPoint treatment separately, then add separate tests. Coordinate with IMV7 without editing its dirty closeout worktree. | `nimbus-storage/src/materialized_position.rs:483`, IMV-D7 | medium / reopened | in_progress |
| SA9 | Compare `TenantEventRecord.version` against the supported version on read/replay — `validate_integrity` hashes the record's own stored version, so a future-versioned record verifies clean and replays as current; one comparison closes the silent-misreplay path. Related diagnostics rule: version skew must not report as Corruption for the formats that cross deployment boundaries (backup bundles, AEAD envelope). | `nimbus-storage` journal record validation, blob-plane format readers | medium / confirmed | todo |
| SA10 | Repair the vacuous server-side digest assertions (`SnapshotFingerprint` serializes `position`, not `digest`, so both asserts compare `Null == Null`; the engine-side twin was fixed with #303, the server-side twin was missed) and give the report-semantics consistency endpoint tracing, metrics, or alert integration — report-only is fine, unobserved is not. Rides IMV. | `nimbus-server/src/tests/core_http/documents_and_commits/consistency.rs:33-40`, `nimbus-engine/src/engine/queries/verification.rs` | low / confirmed | todo |
| SA11 | State the tenant-KV durability contract before nimbus-kv ships on it: `Engine::tenant_kv_*` is a deliberate journal-less write plane (no committer, lease, sequence, or journal; redb-only) that is invisible to PITR and replication. Contract documentation is NKV-owned; do not route it through the document committer without an NKV owner decision. | `nimbus-engine/src/engine/kv.rs` | contract work / owned | todo |
| SA12 | Admin-surface conditional auth (the gate no-ops when `local_server_security()` is `None`; admin handlers carry no independent auth; unreachable via first-party binaries, real for direct embedders of `serve()`) is a server security-contract item, not a storage finding. | `nimbus-server/src/local_server/middleware.rs:117-119` | routed | no-action(routed) |
| SA13 | Low/advisory set and design checklists — the M11 absent-marker rule (a replay-range consumer must distinguish "no user-visible change" from "a change you cannot replay"), the M12 one-lock publication rule (watermark bump and shared-metadata publish under one lock), `debug_assert`-only placement invariants, the blob `local.rs` trio, `x-forwarded-proto` handling, the 409→AlreadyExists tripwire, and the cheap exemplar-derived greps — are recorded in the archived artifact's Low and checklist sections; consult before touching those surfaces. Also records the M9 decision: streamed `EncryptedBlobStore::put_stream` uses a random per-object salt (`encrypted.rs:45-52`), a deliberate contract split from buffered puts' deterministic content-derived seeding. Retried streamed puts therefore defeat dedup and can leave reclaimable orphans for the BLI sweep. Any change to the seeding is a crypto-contract owner decision, not an audit patch — do not re-file this as a finding. | review artifact §Low, §M9, §M11/M12 | advisory | no-action(reference) |
| SA14 | Refuted claims, kept visible so they are not re-filed: PITR "acknowledgement ceiling" epoch seal (the durable journal tail is the correct export bound; the zombie-writer residual is SA3); external-S3 "false green" (the owning verifier already fails loudly on partial config or a missing `RUN_EXTERNAL_S3=1`; residual is the absent hosted live lane, an operational gap); schedule-only ambiguity exemption (classification lives one layer down in `persist_schedule_only_execution_unit` with full Committed/RolledBack/Ambiguous reconciliation and crash-and-replay escalation); libSQL silent refresh give-up (synchronous fallback propagates the error); commit-log replay non-idempotence (byte-compare plus gapless-prefix gates); BLIF7 backup lock race (refuted in the BLI plan). | — | refuted | no-action(refuted) |
| SA15 | Documentation truth-ups already owned elsewhere: the stale `TenantPointWrite` mutation-path claim is IMV4's (CLAUDE.md is a symlink to AGENTS.md — one file); storage-seams spec tracking is BLI5's red condition. | docs | owned | no-action(owned) |

Band completion gate: every SA row is `done` or `no-action(<reason>)` with
focused evidence; scope-addition rows record the owning plan's accept/decline
decision in the Status cell.

## Execution Order and Dependencies

1. **GR first** — guarantee repairs are independent of everything else and
   highest stakes.
2. **SR next** — SR7/SR8 require SR1 and an owner discussion (the enum
   dispatch is a documented deliberate choice); everything else in SR is
   independent.
3. **CO / DE / TI / UI / DS** — parallel-safe among themselves; CO2–CO5
   should land before CP3 so the handler migration moves consolidated code.
4. **CP last** — CP1 any time after GR; CP2 after CP1; CP3 after CP2 and
   ideally after SR3/SR4/CO2–CO4.

Sizing SWAG (net-new + moved, excluding tests): GR ~600 LoC, SR ~1,200,
CO ~1,800, DE ~1,500 (mostly moves), TI ~500, UI ~1,200, DS ~200,
CP ~2,500 (mostly moves). No single item should exceed its band's review
finding; if implementation reveals 2× scope, stop and re-scope in the
ledger.

## Verification Contract

- Per item: focused tests for the touched surface, then update the ledger
  Status with one evidence line (test names/counts or commit SHA).
- Per band closeout: `cargo fmt --all --check` + `make clippy` + the
  band-relevant suites; full `make ci` before any merge batch.
- Blast-radius rule: any fail-closed change (GR2, GR3, GR7) must run the
  full workspace suite, not a name-filtered subset.
- The plan ledger in this file is the single completion record; keep the
  Corrected/No-Action ledger updated when an item turns out wrong.

## Suggested Goal Prompts

Master (whole campaign):

```text
/goal Execute docs/private/plans/architecture-review-2026-07-plan.md band by band in the documented order (GR, SR, CO, DE, TI, UI, DS, then CP if its gates hold; SR7/SR8 need an owner decision first — if none is recorded, mark them blocked and move on). For each item: read the cited files, implement the fix to the item's acceptance line, run focused tests for the touched surface, and update the item's Status cell to done with one evidence line (test names/counts or commit SHA). Decide rather than ask; if an item is wrong or already fixed, mark it no-action with a one-line reason; if truly blocked, record the blocker in the ledger and continue. Fail-closed changes (GR2, GR3, GR7) require the full workspace suite. The goal is met when every GR/SR/CO/DE/TI/UI/DS item's Status is done, no-action(reason), or blocked(reason), and make ci is green on the final state — or stop after 80 turns.
```

Single band (substitute the band ID):

```text
/goal Complete Band GR of docs/private/plans/architecture-review-2026-07-plan.md: implement GR1-GR8 to each item's acceptance line, run focused tests per item plus the full workspace suite for GR2/GR3/GR7 (fail-closed blast radius), update each Status cell to done with one evidence line, and finish with cargo fmt --all --check, make clippy, and make ci green. Decide rather than ask; mark wrong items no-action with a reason and truly blocked items blocked with the blocker recorded. The goal is met when all eight GR ledger rows are done/no-action/blocked with evidence and make ci is green — or stop after 25 turns.
```

Storage adversarial phase:

```text
/goal Execute Band SA in docs/private/plans/architecture-review-2026-07-plan.md through completion. Resume from the SA ledger, execution log, and git state. Keep one SA task in_progress. Use a dedicated codex/sa-<id> branch and nimbus-worktrees/sa-<id> worktree for each code task. Keep plan edits on main. Capture fail-before evidence, implement at the owning seam, run focused checks, and record exact evidence before advancing. Do not change IMV, BLI, or NKV scope without the owning plan's recorded acceptance. The goal is met when every SA row is terminal, every accepted repair has focused evidence, and the final required repository gates pass.
```

Storage semantic-type residual:

```text
/goal Execute RR30 in docs/private/plans/architecture-review-2026-07-plan.md after IMV1 and BLI3 merge. Inventory every storage durable-outcome and provider-capability type plus each public construction path. Classify each type as an opaque validated value, a closed enum with no invalid state, or a repair. Preserve valid closed enums. Write proof/architecture-review-2026-07/rr30-storage-semantic-types.md with the type, owner, construction paths, verdict, and required test or source gate. Add one nonterminal child row for every repair before marking RR30 done. Stop when the inventory has no unowned repair and the docs gate passes.
```

Numeric query and index semantics residual:

```text
/goal Execute RR31 in docs/private/plans/architecture-review-2026-07-plan.md after IMV1 merges. Read docs/private/adapters/convex/ai-guidelines.md first. Inventory numeric equality and ordering across Nimbus, Convex, Firestore, MongoDB, and DynamoDB index creation and query paths. Define one engine-owned semantics contract and never infer stored index meaning from request transport. Write proof/architecture-review-2026-07/rr31-numeric-index-semantics.md with every path, equality rule, ordering rule, large-integer bound, selected semantics, and test obligation. Add one nonterminal child row for every incompatible path before marking RR31 done. Stop when no adapter can silently use an incompatible index and the docs gate passes.
```

## Execution log

| Date | Item | Action | Evidence |
| --- | --- | --- | --- |
| 2026-08-19 | RR30 | routed | Accepted IMVR1 from the IMV and BLI plan review. RR30 stays deferred until IMV1 and BLI3 merge. No implementation started. |
| 2026-08-19 | RR31 | routed | Accepted IMVR2 from the Fable plan review. RR31 stays deferred until IMV1 supplies the normalized logical value tree. No implementation started. |
| 2026-08-19 | meta | corrected | Named RR31 beside RR30 in the resume pointer and the baseline line. No implementation started. |
| 2026-08-20 | meta | rebased | Refreshed the RR30 and RR31 baseline to `137cc632a` after 17 merged commits. Neither residual changed status. No implementation started. |
| 2026-08-20 | RR31 | input-captured | IMV1 work `0da288204` preserved current numeric index bytes and recorded the integer-versus-float collision, large-integer risk, and adapter consequences in `rr31-numeric-index-semantics.md`. RR31 remains deferred until IMV1 merges. |
| 2026-08-25 | SA band | created | Accepted the corrected ledger of the 2026-08-25 storage adversarial review (main @ `9c807015a`; seven lanes, direct-code verification, two-round gpt-5.6 cross-adjudication). Report archived at `proof/architecture-review-2026-07/storage-adversarial-audit-2026-08.html`. SA1–SA15 recorded; no implementation started. |
| 2026-08-25 | SA13 / SA band | corrected | Added the M9 streamed-encryption salt decision to SA13 and normalized Band SA status cells to plan vocabulary (routing moved into item text). No implementation started. |
| 2026-08-25 | SA8 | started | Accepted SA8 as a correction to the merged IMV1 codec. The dirty IMV7 closeout worktree remains unchanged. SA8 is the only active Band SA task. |
