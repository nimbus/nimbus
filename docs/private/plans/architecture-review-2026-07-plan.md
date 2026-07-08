# Architecture Review 2026-07 — Improvement Control Plane

Status: `active`
Owner: this plan
Provenance: full-workspace architecture review, 2026-07-06 (six parallel
subsystem mappings + second-pass gap/hygiene sweeps + extraction/identity
inventory). Review artifact:
`proof/architecture-review-2026-07/nimbus-architecture-review.html`.

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

- `nimbus-egress/src/policy.rs` split — **no action**: 1,486 raw lines but
  tests start at :888; production size is under threshold.
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

Status values: `todo` | `in_progress` | `done (evidence)` |
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
| SR8 | Deepen or delete `EmbeddedPersistenceProvider` (abstracts almost nothing; own doc admits the migration contract lives on the concrete store). Resolve together with SR7. | `nimbus-storage/src/async_storage/traits.rs:16-20` | small | todo (AD3: delete EmbeddedPersistenceProvider; sequenced after the storage-lane SR1 PR merges) |

### Band CO — Consolidations and shared primitives

| ID | Item | Where | Size | Status |
| --- | --- | --- | --- | --- |
| CO1 | Extract a generic `BackgroundWorker<T>`/`WorkQueue<T>` seam; subscription delivery, trigger candidates, and trigger execution supply only per-item closures (three hand-rolled `Mutex<VecDeque>+Condvar` scaffolds today). | `nimbus-engine/src/tenant/{subscription_delivery/{worker,queue}.rs, trigger_candidates.rs, trigger_execution.rs}` | large | done (PR #142 merged, 2026-07-07; dual-reviewed 3 rounds, 2 P1s + P2-follow-on + P3 fixed pre-merge) |
| CO2 | One operator-route authorization helper: collapse the triplicated `authorize_operator_*_route` flow + shared scope matchers + one parameterized audit recorder into `http/authz.rs`. | `nimbus-server/src/resource_control/{sessions.rs:272,445,463, services.rs:276,359,383, sandboxes.rs:128,193,224}` | medium | done (PR #144 merged, 2026-07-07; authz core + verb seam + HTTP-adapter mount seam; 1 test-quality fix) |
| CO3 | Generic `paginate_by_key(...) -> (page, PageMeta)` + one shared collection-metadata struct (three identical clamp/truncate/next-token blocks). | `sessions.rs:263`, `services.rs:74`, `sandboxes.rs:211` | small | done (PR #138 merged efbf56a30, 2026-07-07; http/pagination.rs shared helper) |
| CO4 | One private helper for the four near-identical service-lifecycle handlers (get/start/stop/restart differ only in manager verb + surface string). | `nimbus-server/src/http/services.rs:477-632` | small | done (PR #138 merged efbf56a30, 2026-07-07; ServiceLifecycleVerb helper) |
| CO5 | Callable adapter uses the canonical `StructuredHttpError::status()` mapping instead of re-matching every `Error` variant. | `adapters/cloud_functions/http/callable.rs:147-204` vs `error_envelope.rs:508` | small | done (PR #138 merged efbf56a30, 2026-07-07; canonical StructuredHttpError mapping + documented 6-variant legacy override) |
| CO6 | Split the mysql/postgres `write.rs` twins (1,499/1,486 prod lines of parallel logic): statement-building vs txn orchestration, sharing a SQL-write helper. | `nimbus-storage/src/{mysql,postgres}/write.rs` | medium | done (PR #141 merged 7b79aed10, 2026-07-07; sql/write_core.rs shared 6 txn methods + sql/row.rs; dialect-divergent kept per-backend) |
| CO7 | `now_millis()` + injectable `Clock` in `nimbus-core`; route the 27 inlined `duration_since(UNIX_EPOCH)` sites through it (unlocks deterministic time in tests; today only nimbus-services has a test clock). | 27 files across blob/bridge/cli/convex/core/crypto/dynamodb/fs/kv/license/s3/runtime | medium | done (PR #154 merged 92d84ffc8, 2026-07-08; Clock moved to nimbus-core (re-export shim), 17 sites swept, 5 census corrections) |
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
| TI6 | Stabilize the nimbus-engine timing/concurrency test-flake family. Across the 2026-07 campaign these flaked CI intermittently while passing 3-8/8 locally on clean rebuilds (PRs that touch ZERO engine files still hit them): `materialized_serving::concurrency::{concurrent_first_load_only_publishes_caught_up_newest_materialized_table, paused_first_load_catches_up_before_publication}`, `subscriptions::basic::subscription_snapshots_expose_covered_sequence_and_commit_timestamp_metadata`, `cooperative_*_multiple_parked_runtimes`, `engine_update_and_delete_drive_subscription_updates`. Root-cause the shared timing assumption (likely a fixed sleep/deadline in `nimbus-testing/eventual.rs` too tight under CI shard load, or a publish/catch-up race in the materialized-serving harness) and make them deterministic (barrier/await on observable state, not wall-clock). | `nimbus-engine/src/tests/{materialized_serving,subscriptions}`, `nimbus-testing/src/eventual.rs` | medium | in_progress (branch ti6-flake-stabilization; root cause = trigger-feed cursor-advance commits share the document sequence space; settle/pause barriers, 40/40 stress; PR pending) |
| TI7 | Engine follow-up from the TI6 review (efficiency, not correctness): the trigger feed's zero-write cursor-advance commits bump `required_sequence` without flowing through `MaterializedServingBackend::apply_commit`, so a query racing one forces a spurious warmed-table reload (results stay correct; the reload is wasted work — this production behavior WAS the reuse-test flake). Fix at the materialized-serving boundary: advance coverage through zero-write commits, or derive `required_sequence` from the document-applied head. Then assert warmed reuse with the trigger feed running. | `nimbus-engine/src/tenant/materialized_reads/`, `tenant/trigger_candidates.rs` | small | todo |
| TI8 | Engine follow-up from the TI6 review (metadata quality, not correctness): a zero-write cursor-advance commit landing in the subscribe bootstrap→activation gap coalesces with the next real document commit, and coalescing intentionally drops per-commit identity (`engine/mutations/commit_processing.rs` "Coalesced batches intentionally omit...") — so a lone real commit can lose its `snapshot.commit` metadata to an internal empty commit. Exclude zero-write commits from coalescing identity so a single real commit keeps metadata. | `nimbus-engine/src/engine/mutations/commit_processing.rs`, `subscriptions/delivery.rs` | small | todo |

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

### Band CP — `nimbus-compute` extraction (AD1, staged)

| ID | Item | Where | Size | Status |
| --- | --- | --- | --- | --- |
| CP1 | Create `crates/nimbus-compute` and move the transport-free modules as-is: `execution/`, `artifact_verifier_effects`, `machine_lifecycle.rs`, the `service_manager` binding shim. Acceptance: crate has zero axum/http-transport deps (Cargo.toml proof); server re-exports compile; `make ci` green. | new crate + `nimbus-server/src/{execution/,artifact_verifier_effects*,machine_lifecycle.rs,service_manager.rs}` | medium | todo |
| CP2 | Split `AppState`: `ComputeState` (engine + registries, zero axum types) lives in nimbus-compute; server `AppState` wraps it plus transport concerns. `construction.rs` splits the same way. | `nimbus-server/src/{state.rs,construction.rs}` | medium | todo |
| CP3 | Migrate the orchestration halves of the deploy/services/sandboxes/scheduling/machines handlers into compute-owned functions; axum handlers reduce to extract → call → respond. Coordinate with SR3/SR4 and CO2–CO4 (do the consolidations first or fold them in). Optionally point `nimbus-cli` start/dev wiring at nimbus-compute directly. | `nimbus-server/src/http/{deploy,services,sandboxes,sandbox_spec,scheduling,machines}.rs` | large | todo |

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
