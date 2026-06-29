# Cloudflare Adapters Plan (CFA, Archived)

Nimbus already ships **inbound compatibility adapters** that impersonate an
external service so its client code runs unchanged on a single Nimbus binary:
MongoDB and DynamoDB (wire/HTTP listeners), Firestore and Cloud Functions
(HTTP + runtime shims), and Convex (HTTP/WS + `HostBridge`). This plan opens the
**Cloudflare** adapter family on the same seam — and, crucially, on the same
principle: **adapters are thin compatibility surfaces over first-class Nimbus
primitives, never the home of the implementation.**

Companion research (contract source of truth, do not re-derive from memory):
`docs/private/plans/research/cloudflare-adapters-2026.md`.

Owner-ratified scope (2026-06-16, re-architected 2026-06-22): **inbound first**,
**core five** researched (Workers runtime, KV, D1, R2, Durable Objects). The
build wedge is **Workers KV → Durable Objects**, with the proof bar raised to
**`env.NS` running end-to-end inside a real Worker** — which deliberately pulls
a *minimal* Workers-runtime slice forward into this plan. D1, R2, and the full
Workers-runtime surface are sequenced into named follow-on bands.

## Why this plan exists

A reviewer comparing Nimbus to "your cloud, one binary" sees adapters for
MongoDB, DynamoDB, Firestore, Convex — but nothing for the Cloudflare Workers
platform, the most common edge/serverless backend developers want to self-host
off. Three load-bearing facts (research + the 2026-06-22 storage-portfolio
review) make a Cloudflare adapter tractable now:

1. **`workerd`, `miniflare`, `workers-types`, and `lol-html` are
   Apache-2.0 / MIT / BSD-3** — the entire reference stack is freely
   incorporable under Nimbus's license posture
   ([[feedback_apache_license_posture]]).
2. **Primitives-first beats foreign-runtime-embedding for Nimbus.** Cloudflare's
   own Miniflare went reimplement → embed-`workerd` for parity; but embedding
   `workerd` locks Durable-Object *storage* to its on-disk SQLite with no host
   seam, breaking Nimbus's engine-owned-storage and tenant-isolation
   invariants, and adds a second V8 + a per-binding process boundary. The
   owner-ratified direction is therefore: **build the missing Nimbus primitives
   (a KV primitive, a durable-object primitive), then put thin Cloudflare
   surfaces over them, and reimplement the Workers runtime as a profile on
   `nimbus-runtime` whose bindings resolve to those primitives.**
3. **The primitives already have homes in the storage-seam architecture.** The
   2026-06-22 review (`docs/private/architecture/storage-seams-architecture.md`)
   places each one precisely — see the table below — so CFA extends the existing
   seams rather than inventing a parallel stack.

## Architecture: which Nimbus primitive each Cloudflare primitive sits on

| Cloudflare | Nimbus primitive | Home / seam | This plan |
| --- | --- | --- | --- |
| **Workers KV** | the **`nimbus-kv`** primitive (`TenantKvStore` capability seam in `nimbus-storage`) | Workers KV is a **thin adapter over `nimbus-kv`** — the `TenantKvStore` capability is built/owned by `nimbus-kv` **NKV0 band F2**, not CFA (the 2026-06-22 "consolidate now" decision) | **adapter only** (CFA3, rides NKV0 F2) |
| **Durable Objects** | **durable-object substrate** (single-instance actor) | `nimbus-services` catalog single-instance resource + engine serialized mutation + per-instance storage namespace + scheduler + WS | **builds it** (CFA6–8) |
| **Workers runtime** | **Workers execution profile** | a `RuntimeBackend`/V8 profile on `nimbus-runtime` (the `WorkerLoop`/`RuntimeBackend` seam, same shape as Bun/JSC); bindings via `HostBridge` | **minimal slice** (CFA4) |
| **R2** | the **NOS object-storage primitive** (`s3s::S3` over `BlobStore`+`ObjectMetaStore`) | thin S3/SigV4 + binding adapter over NOS | **deferred — gated on NOS Phase 3** |
| **D1** | existing **SQLite/libSQL storage family** | thin prepared-statement adapter | follow-on, independent |

> **KV primitive shape (RESOLVED 2026-06-22).** Two layers, no duplication: the
> **flat persistence seam — the `TenantKvStore` capability trait — lives in
> `nimbus-storage`** (key+value+metadata+TTL commit atomically on the metadata
> plane, macro-impl'd across the 5 backends; the invariant `nimbus-storage` never
> depends on `nimbus-blob` keeps it acyclic), and the **monolithic `nimbus-kv`
> crate is the consumer that owns RESP + data structures + encoding + cache/
> tiering** on top of it. The owner chose the monolithic-crate identity for
> DRY/consolidation and to hide storage/scaling complexity (research
> §6). `TenantKvStore` itself is built and owned by the `nimbus-kv` program at
> **NKV0 band F2** (`docs/private/plans/nimbus-kv-foundation-plan.md`); CFA3 is a
> thin adapter that rides it. NKV1 then layers the Tier-0 command surface
> (string/incr/expire/keyspace/scan) Workers KV and Memcached consume — useful
> but NOT a hard prerequisite for the redb-backed CFA wedge, which needs only the
> F2 trait.

## Why pull the Workers-runtime slice forward

The original plan deferred *all* Worker-code execution and proved KV only
through the REST API + an injectable binding mechanism. The owner raised the bar:
the wedge must prove **`env.NS.get()/put()` works inside an actual running
Worker**, because that is the real product promise. The key insight that keeps
this tractable: proving `env.NS` end-to-end needs only a **minimal** runtime
slice — ES-module load, `export default { fetch(request, env, ctx) }` dispatch,
and `env` injection — *not* the full Workers API surface (HTMLRewriter, Cache
API, `cf` object, full streams). That minimal slice is a Workers profile on the
existing `nimbus-runtime` V8 backend; the large surface remains a follow-on band
that grows the profile over time (and can reuse `lol-html`, BSD-3, for
HTMLRewriter).

## Scope

In scope:

- **KV primitive:** `crates/nimbus-storage/src/traits/` (new `TenantKvStore`
  capability trait) + macro impls across redb/SQLite/Postgres/MySQL/libSQL +
  `TenantKeyring` at-rest crypto + engine mutation-path atomicity.
- **Workers runtime slice:** `crates/nimbus-runtime/` Workers execution profile
  on the `WorkerLoop`/`RuntimeBackend` seam (module-worker load, `fetch`
  dispatch, `env` injection); `crates/nimbus-runtime/src/host.rs` new
  `HostCallOperation` variants for the KV (and later DO) bindings.
- **Durable-object substrate:** `crates/nimbus-services/src/catalog.rs`
  single-instance resource + per-instance storage namespace; engine serialized
  execution; `nimbus-engine` scheduler for alarms; `crates/nimbus-server/src/ws/`
  for hibernation.
- **Cloudflare adapter family:** `crates/nimbus-server/src/adapters/cloudflare/`
  (`mod.rs` + `CloudflareConfig`, `config.rs` wrangler-binding parser, `kv/`,
  `durable_objects/`); registration in `adapters/mod.rs`, `construction.rs`,
  `router.rs`, `start/adapters.rs` (`--cloudflare` / `--no-cloudflare`).
- Tests: KV-primitive + KV-contract + **`env.NS` real-Worker end-to-end** + DO
  contract conformance, under `crates/*/tests/` and beside the modules.
- `docs/private/operating/cloudflare-adapters.md`;
  `scripts/verify-cloudflare-adapters.sh`; routing in `AGENTS.md` +
  `docs/private/plans/README.md`; proof bundle under
  `docs/private/plans/proof/cloudflare-adapters/`.

Out of scope (named follow-on bands — promote a fresh active plan or band first):

- **R2 adapter** — thin S3/SigV4 + Workers-binding object store **over the NOS
  object-storage primitive**. Hard cross-lane dependency: **NOS3**
  (`nimbus-s3-object-storage` S3 surface; a.k.a. NOS-A3) must land first. (The NOS1
  byte-plane `nimbus-blob` already exists; R2 needs the **NOS3 S3 name-plane
  surface**, which does not yet — so R2 is blocked on that specific surface, not an
  entire unbuilt subsystem.)
- **D1 adapter** — inbound prepared-statement adapter over the existing
  SQLite/libSQL storage family (auto-commit-only, `batch()`-as-transaction,
  Sessions API). Independent of the rest.
- **Full Workers-runtime surface** — the long tail beyond the minimal slice:
  HTMLRewriter (reuse `lol-html`), Cache API (`caches.default`), `request.cf`
  synthesis, full WHATWG streams fidelity, `scheduled()`/Cron-trigger product
  surface, service bindings. Grows the CFA4 profile.
- **Cluster-scale single-instance DO routing** — "one live instance per id
  across a cluster" is owned by **HS5** (`horizontal-scaling`: leader placement +
  Raft + gossip). CFA6 is **single-node MVP** and records the handoff; it does
  not block on HS.
- **Outbound storage backends** (KV/D1/R2 *under* Nimbus) and **Cloudflare's
  edge topology** (anycast/PoPs) — out of scope by construction.

## Upstream API anchors

Verify signatures against these, not from memory (full list + the two overturned
assumptions in the research doc §10):

- **workerd / workers-types / miniflare / lol-html**
  (`github.com/cloudflare/{workerd,workers-sdk,lol-html}`) — authoritative TS
  shapes + the reference reimplementation; Apache-2.0 / MIT / BSD-3.
- **KV**: `getWithMetadata` returns `{ value, metadata }` (no `cacheStatus`);
  `put` `expiration` (abs epoch) vs `expirationTtl` (≥60 s); `list` paginates by
  `list_complete` + `cursor`; value ≤25 MiB, metadata ≤1024 B, 1 write/sec/key.
- **Workers runtime**: ES-module `export default { fetch(request, env, ctx) }`;
  bindings injected via `env`; `ctx.waitUntil`/`passThroughOnException`.
- **Durable Objects**: `idFromName` deterministic / `newUniqueId` non-det /
  `idFromString` 64-hex; lazy instantiation; one live instance per id;
  `ctx.storage.sql.exec` + sync/async KV + `transaction(Sync)`; legacy KV
  ≤128 KiB/value; alarms at-least-once, 6 retries, backoff from 2 s;
  `acceptWebSocket` + `serializeAttachment` ≤16 KiB; input/output gates; RPC via
  Structured Clone.

## Ledger

| CFA | Description | Status |
|-----|-------------|--------|
| CFA0 | Scaffold plan + verifier at `scripts/verify-cloudflare-adapters.sh` (12 conditions, mostly FAIL until later bands flip them); research doc at `docs/private/plans/research/cloudflare-adapters-2026.md`; baseline proof at `docs/private/plans/proof/cloudflare-adapters/cfa0-baseline.md`; routing in `AGENTS.md` + `docs/private/plans/README.md`. Records the ratified decisions: inbound-first, primitives-first (build-on-Nimbus, not embed-`workerd`), KV→DO wedge with `env.NS`-in-a-real-Worker as the bar, R2-over-NOS and DO-cluster-over-HS5 deferrals. | done |
| CFA1 | Cloudflare adapter skeleton + config + wiring. New `crates/nimbus-server/src/adapters/cloudflare/mod.rs` exposing `CloudflareConfig`; register `pub mod cloudflare;` in `adapters/mod.rs`; mount via `construction.rs` (`ServeOptions::with_cloudflare`) + `router.rs` (`build_cloudflare_router`); add `cloudflare: Option<CloudflareConfig>` to `AdapterEnablement` in `start/adapters.rs` with `--cloudflare` / `--no-cloudflare` (default-on, fail-closed dev creds like the other wire surfaces). `config.rs` parses `wrangler.jsonc`/`wrangler.toml` binding declarations (`kv_namespaces`, `durable_objects`, later `d1_databases`/`r2_buckets`) into a typed binding registry. No data behavior yet. Tests assert binding-registry parsing of a representative `wrangler.jsonc`. | done |
| CFA2 | **KV primitive — owned by `nimbus-kv` NKV0 band F2, do NOT rebuild here.** Per the 2026-06-22 "consolidate now" decision, the flat `TenantKvStore` capability in `crates/nimbus-storage/src/traits/` (`kv_get`/`kv_put`/`kv_delete`/`kv_scan` + atomic batch, macro-impl'd across the 5 backends, engine-atomic, `TenantKeyring` crypto, TTL) is built and owned by the `nimbus-kv` program at **NKV0 band F2**, not by CFA. (`getWithMetadata` and `list` pagination are CFA3-adapter concerns layered over `kv_scan`, not trait methods.) CFA2 is therefore a **prerequisite gate, not build work**: confirm `nimbus-kv` NKV0 has landed `TenantKvStore` (through F2) before CFA3 runs. No CFA code in this row. | done |
| CFA3 | **Workers KV adapter** (thin compat over the `nimbus-kv` `TenantKvStore` surface from NKV0 F2). `crates/nimbus-server/src/adapters/cloudflare/kv/` maps the Cloudflare KV contract onto `TenantKvStore`: `get` type coercion (`text`/`json`/`arrayBuffer`/`stream`), `getWithMetadata` → `{ value, metadata }` (no `cacheStatus`), `put` `expiration` vs `expirationTtl` (≥60 s, reject <60 s), `metadata` ≤1024 B, value ≤25 MiB, key ≤512 B, `list_complete`+`cursor` pagination (empty `keys` ≠ done). Two front doors: (a) the Cloudflare **KV REST API** (`wrangler kv` + REST clients, testable now); (b) `HostCallOperation::CfKv*` variants in `crates/nimbus-runtime/src/host.rs` + a Cloudflare `HostBridge` so the CFA4 runtime can inject `env.NS`. Document the **deliberate deviation**: Nimbus serves strongly-consistent reads — a compatible superset of KV's ≤60 s eventual consistency, never reproduced. Contract tests incl. negative paths (<60 s TTL rejected, oversize rejected, missing-key delete = success). | done |
| CFA4 | **Minimal Workers runtime slice.** A Workers execution profile on `crates/nimbus-runtime/` via the `WorkerLoop`/`RuntimeBackend` seam (reusing that seam for pooling/lifecycle — but the module-worker `fetch(request, env, ctx)` dispatch, the `env` bindings object, and the per-invocation `ctx` are **net-new**, NOT "same shape" as the Convex-shaped `InvocationKind` dispatch the Bun/JSC backends use): load an ES-module Worker, dispatch `export default { fetch(request, env, ctx) }`, build and inject the `env` object with binding stubs that resolve through `HostBridge` (the CFA3 `CfKv*` ops → the CFA2 primitive), support `ctx.waitUntil`/`passThroughOnException`. Minimal by design — NOT the full Workers surface (HTMLRewriter/Cache/`cf`/full streams are the follow-on band) — **and unimplemented Worker APIs (`caches.default`, `request.cf`, streaming bodies, `scheduled()`) raise a NAMED error, never a silent no-op or `undefined`**, so a migrating Worker gets a clear failure rather than a wrong result. Tests: a trivial Worker returns a `Response`; `env` is populated; `waitUntil` runs post-response; **a Worker referencing an unsupported API is rejected with that named error**. | done |
| CFA5 | **Prove `env.NS` end-to-end inside a real Worker** (the wedge bar). A real ES-module Worker that does `env.NS.put()/get()/getWithMetadata()/list()` runs on the CFA4 runtime, resolving through the CFA3 adapter mapping to the CFA2 KV primitive. Conformance test asserts the documented KV contract **from inside the Worker** (not just via REST): value round-trips, metadata round-trips, TTL behavior, list pagination. Proof `cfa5-env-ns-e2e.md` captures the Worker source + the passing run. This closes the **KV wedge**: a Worker whose handler is a `fetch` returning a `Response` and whose only binding I/O is Workers KV runs unchanged on Nimbus. (Workers using streaming bodies, `request.cf`, the Cache API, service bindings, or `scheduled()` triggers require the follow-on Workers-runtime-surface band — and hit the CFA4 fail-loud boundary, never a silent wrong result.) | done |
| CFA6 | **Nimbus durable-object primitive (substrate).** Design proof `cfa6-do-primitive.md` + the catalog seam: single-instance addressing (`idFromName` deterministic / `newUniqueId` / `idFromString` 64-hex) and the **one-live-instance-per-id routing guarantee** (single-node MVP — enforced via the engine serialized path + a catalog single-instance lease; `nimbus-services` `claim_activation` exists but is `(tenant_id, service_name)`-scoped, so DO addressing needs a finer per-id directory). **Key the `DurableObjectInstance` directory as `(tenant_id, do_namespace, do_id)` from day one** (NOT `service_name`) so the single-node key already matches the HS5 cluster key and no re-key rewrite is needed at scale. **The `tenant_id` lead component is the ISOLATION boundary, not just routing:** DO id resolution (`idFromName`/`newUniqueId`/`idFromString`) is confined to the calling Worker's authenticated `(tenant_id, do_namespace)` — a stub for another tenant's id is *unconstructable*, the per-instance storage handle is derived from the authenticated tenant (never a wire-supplied id alone, even a forged 64-hex `idFromString`), and the engine's `ensure_tenant_matches` invariant binds every DO storage access. CFA7's cross-tenant test proves tenant A cannot stub/RPC/read tenant B's DO id. The proof MUST record three exemplar-review decisions (CF research §11): **(a)** the **HS5 handoff requires PER-DO-ID placement/leasing beneath the tenant lease** (Akka two-level region=tenant→entity=DO / Orleans per-grain directory) — a per-tenant lease alone collapses a tenant's many DOs onto one node, defeating DO scatter; this is a hard requirement on `horizontal-scaling` **HS5 (or an HS follow-on)**, fenced at per-DO storage. **(b)** the **serialization granularity — DECIDED (owner, 2026-06-23): per-DO-id independent serialization lanes (DO-faithful).** The engine mutation journal serializes *per-tenant*; that lane must NOT be the DO write lane (it would collapse a tenant's many DOs onto one writer — non-viable for the coordination workloads DOs exist for). CFA7 gives each `(tenant_id, do_namespace, do_id)` its own serialization lane; the tenant-journal-shared branch is rejected. **(c)** the **transient-duplicate contract**: under ungraceful failover a brief duplicate-activation window is unavoidable on commodity infra (Orleans ~30 s; Akka pre-SBR), so correctness rests on epoch/ETag fencing rejecting the loser's writes, NOT on an absolute "one instance in the world." **The fence is concrete:** a per-`(tenant_id, do_namespace, do_id)` monotonically-increasing **lease epoch** stored in the DO's per-instance namespace; every DO write carries its activation's epoch and commits-and-validates **transactionally** (reject if the stored epoch has advanced) — a transactional compare inside the engine storage txn, not an advisory check. CFA7 gate test (iv) proves the loser's writes are rejected. Extend `crates/nimbus-services/src/catalog.rs` with the `DurableObjectInstance` resource + per-instance storage handle. Skeleton + decisions only; behavior in CFA7–8. | done |
| CFA7 | **Durable Objects storage + lifecycle + RPC + prove a real DO.** `crates/nimbus-server/src/adapters/cloudflare/durable_objects/`: per-instance SQLite-backed storage (`ctx.storage.sql.exec` → cursor with `columnNames`/`rowsRead`/`rowsWritten`/`next`/`toArray`/`one`/`raw`), sync + async KV storage, `transaction`/`transactionSync`, legacy KV storage (≤128 KiB/value, ≤128 keys/batch) on the per-instance namespace. **Per-DO-id serialization lane (CFA6 decision b): each `(tenant_id, do_namespace, do_id)` gets its own concurrency lane — replace the single per-tenant `sequence_gate: Mutex<()>` with a keyed lock map INSIDE the engine mutation path. Do NOT *share* the per-tenant `sequence_gate`; but every DO write still flows through `run_store_mutation`/`apply_mutation_with_mode` and terminates in ONE storage transaction (document + index + commit-log append together). The per-DO lane changes serialization *granularity* only, never the storage-transaction boundary** — a parallel writer that commits DO state outside the engine path is the rejected `workerd`-embed shape and would violate both the "no separate code path" and "single storage transaction" invariants (`AGENTS.md`). Lazy instantiation (constructor on first call/alarm), eviction/persistence (storage survives; constructor re-runs). **Input/output gates are NOT free from the journal** (CF research §11): the journal gives write durability-ordering only — implement the **input gate** (defer incoming events, incl. non-storage RPC/`fetch`, while a storage op is in flight) and the **output gate** (hold outgoing messages until writes flush; **on write failure, discard the queued outgoing messages and restart the object**) at **per-instance scope**, and make `blockConcurrencyWhile` compose with them. RPC: public DO-class methods callable on the stub via the CFA4 runtime + `HostBridge` (Structured-Clone discipline) + `stub.fetch(request)`. **Prove a real DO end-to-end**: a Worker calls a DO stub method that mutates per-instance storage; tests assert state transitions, single-instance-per-id, and four gate properties the verifier now checks individually: **(i) co-commit atomicity** — a DO write's value, index, and **commit-log entry land in ONE storage transaction** (a mid-transaction fault-injection rollback test mirroring the storage rollback test; a branch that commits DO state outside the engine path FAILS the gate, so "atomic coalesced writes" is proven, not asserted); **(ii) the output-gate write-failure path** — inject a per-instance storage write failure mid-transaction and assert queued outgoing messages are discarded, the object restarts, and **no peer observed a message reflecting the failed write**; **(iii) the per-DO concurrency lane** — **two DOs in the same tenant make independent write progress** (one stalled in `blockConcurrencyWhile` does not block the other), and the test exercises a DO stalled in *user code*, not mid-commit, proving the lane is real and not the shared tenant journal; **(iv) fenced single-activation** — two concurrent activations of the **same** DO id (winner bumps the lease epoch) and the loser's queued output-gate writes are **rejected transactionally** (stale epoch), discarded, the object restarts, and storage shows no loser mutation. | done |
| CFA8 | **Durable Objects alarms + WebSocket hibernation.** Alarms via the `nimbus-engine` scheduler keyed per instance: `setAlarm`/`getAlarm`/`deleteAlarm` + the `alarm(alarmInfo)` handler, at-least-once with exponential backoff from 2 s up to 6 retries, waking an evicted instance (constructor before `alarm()`). The alarm schedule is **part of the per-DO durable state and fenced to the current lease holder** (CF research §11) — a stale former owner must NOT also fire the alarm; in cluster mode the alarm fires only on the node holding the per-DO lease. WebSocket hibernation via `crates/nimbus-server/src/ws/`: negotiate the hibernation path, `acceptWebSocket(ws, tags?)`, handlers `webSocketMessage`/`webSocketClose`/`webSocketError`, `serializeAttachment`/`deserializeAttachment` (≤16 KiB) on the `SessionResource`, `getWebSockets(tag?)`, `setWebSocketAutoResponse` (≤2048 chars, served without waking). Tests: an alarm round-trip (incl. a retry) and a hibernate→wake socket round-trip preserving attachment state. | done |
| CFA9 | **Activation + docs + closeout + submitted PR.** Register the Workers runtime profile as a real `RuntimeBackend` kind. Operator doc `docs/private/operating/cloudflare-adapters.md`: toggle + dev creds; the **auth posture** (generated dev-creds are a loopback-only single-tenant convenience, NOT production multi-tenant auth — lifting the loopback refusal requires per-credential→tenant binding *enforced* + TLS for plaintext `AUTH`/SCRAM; production credential rotation/provisioning/revocation is a named follow-on, the service-identity / secret-management lane); the KV strong-vs-eventual deviation **and the latency-class deviation** (a durable strongly-consistent store, NOT a microsecond edge cache — do not present it as a drop-in for KV's read-latency profile); a supported / unsupported / **errors-loudly** Workers-API compatibility matrix (KV/DO supported; HTMLRewriter, `caches.default`, `request.cf`, full streams, `scheduled()` unsupported and **rejected with a named error**, never a silent wrong result); the durable-object single-instance + per-instance-storage model + the HS5 cluster-routing handoff (incl. the per-DO leasing requirement); the **honest transient-duplicate contract** (under ungraceful failover a DO may briefly observe a fenced-out predecessor; Nimbus on commodity infra cannot promise Cloudflare's "one instance in the world" — correctness rests on the per-DO storage fence, Orleans/Akka-style); the minimal-vs-full Workers-runtime boundary and which follow-on band closes it; the R2-needs-NOS dependency. Record the KV strong-vs-eventual and latency deviations as **tracked deviation-register entries** (the MongoDB launch-readiness-M9 style), not just plan prose. Refresh `docs/private/architecture/runtime/adapter-boundary.md`. Flip every ledger row to `done`; append the Execution Log; move the plan to `docs/private/plans/archive/`; verifier's `plan_file()` accepts both paths; update routing. **Push, verify, PR:** push the active branch, confirm full branch CI green, run the verifier to `12 passed, 0 failed`, and submit the PR to `main` (`codex/nkv-cloudflare-foundation → main` for the current combined worktree). Do not mark CFA9 or the combined plan complete until the PR exists. Never push CFA commits directly to `main`. | done |

## Completion Gate

`bash scripts/verify-cloudflare-adapters.sh` exits 0 with summary line
`12 passed, 0 failed`. The 12 conditions:

1. Plan file exists (active or archived path).
2. Routing entries exist in both `CLAUDE.md` (= `AGENTS.md`) and
   `docs/private/plans/README.md` naming this plan.
3. CFA0: research doc + baseline proof exist.
4. CFA1: `adapters/cloudflare/mod.rs` with `CloudflareConfig`; `adapters/mod.rs`
   declares `pub mod cloudflare;`; `config.rs` (wrangler parser) exists;
   `start/adapters.rs` carries a `cloudflare` toggle.
5. CFA2: the **KV primitive prerequisite** — a `TenantKvStore` trait + `kv_*`
   methods exist in `nimbus-storage` (built and owned by `nimbus-kv` NKV0 band
   F2, not CFA; this condition gates that NKV0 has landed `TenantKvStore` through
   F2 before CFA3 runs).
6. CFA3: the **Workers KV adapter** — `adapters/cloudflare/kv/` mapping over
   `TenantKvStore`, `CfKv*` `HostCallOperation` variants in
   `nimbus-runtime/src/host.rs`, a KV REST handler, and a KV contract test.
7. CFA4: a **Workers runtime profile** in `nimbus-runtime` (module-worker
   `fetch` dispatch + `env` injection) on the `RuntimeBackend`/`WorkerLoop` seam.
8. CFA5: an **`env.NS` real-Worker end-to-end** conformance test (a Worker
   calling `env.NS.*` resolving to the KV primitive) + `cfa5-env-ns-e2e.md`.
9. CFA6: the **durable-object primitive** — design proof `cfa6-do-primitive.md`
   + a single-instance `DurableObjectInstance` resource in
   `nimbus-services/src/catalog.rs`, keyed `(tenant_id, do_namespace, do_id)`
   (NOT `service_name`) so the single-node key already matches the HS5 cluster key
   and `tenant_id` is the isolation boundary.
10. CFA7+CFA8: a `adapters/cloudflare/durable_objects/` module with per-instance
    storage + lifecycle + RPC, alarms (scheduler-backed `setAlarm`/`alarm()`),
    and WebSocket hibernation (`acceptWebSocket` + `serializeAttachment`), plus
    a real-DO end-to-end test, an alarm/hibernate round-trip test, a
    **per-DO-lane independent-progress test** (two DOs in one tenant make
    independent write progress — proves the per-DO serialization lane, CFA6
    decision b, not the shared tenant journal), AND the gate tests that **a DO
    write co-commits its commit-log entry in one storage transaction** and that
    an **output-gate write-failure / fenced-loser (lease-epoch)** path is
    exercised (so a branch that bypasses the engine txn or skips the fence fails
    the gate, not just the happy path).
11. CFA9: operator doc at `docs/private/operating/cloudflare-adapters.md`
    exists, every ledger row is `done`, and the latest `ci.yml` run for the
    active branch is green and matches the current branch head
    (`NIMBUS_VERIFY_CI_BRANCH` may override the branch for explicit closeout
    checks).
12. **Security posture:** the Cloudflare KV REST + Workers + DO ingress surfaces
    fail closed — a non-loopback bind is refused (via the **shared
    `refuse_non_loopback_bind` helper**, not a copy) without explicit opt-in,
    requests require generated dev-cred auth, and tenant is resolved by
    DynamoDB-style credential→tenant binding (never MongoDB-style request-supplied
    namespace selection). Assertion-bearing tests assert a non-loopback bind is
    refused and an unauthenticated request is rejected, **and** that a Worker
    authenticated to tenant A cannot stub/RPC/read a DO id (incl. a forged 64-hex
    `idFromString`) belonging to tenant B.

## Trust targets

- **Before CFA**: no Cloudflare surface; no KV or durable-object primitive.
- **After CFA1**: the Cloudflare adapter family exists, is configurable, and
  parses `wrangler` bindings.
- **After CFA3**: Nimbus has a **first-class KV primitive** (`TenantKvStore`,
  reusable by any adapter), and the Workers KV REST surface runs against it.
- **After CFA5**: a **Worker whose handler is a `fetch` returning a `Response`
  and whose only binding I/O is Workers KV runs unchanged on Nimbus** —
  `env.NS.get()/put()` proven end-to-end through the runtime → adapter →
  primitive path. The KV wedge is closed against that scoped promise; the broad
  Worker surface (streams, `request.cf`, Cache API, `scheduled()`) is the
  follow-on band, and an unsupported API is rejected with a named error, not
  silently wrong.
- **After CFA8**: a Cloudflare **Durable Object** — single-instance per id,
  per-instance transactional storage, alarms, WebSocket hibernation — runs on
  Nimbus's serialized engine, with DO state **in Nimbus's own storage** (the
  invariant embedding `workerd` would have broken).
- **End-state honesty**: even after CFA9, only the **minimal** Workers-runtime
  surface ships — Workers using KV/DO run; Workers depending on the long tail
  (HTMLRewriter, Cache API, `cf` fidelity, full streams) need the follow-on
  Workers-runtime-surface band. **R2 cannot land until NOS Phase 3.** D1 is an
  independent follow-on. Cluster-scale single-instance DO routing is HS5's.

## Proof directory

`docs/private/plans/proof/cloudflare-adapters/`:

- `cfa0-baseline.md` — starting state, ratified decisions, overturned
  assumptions (also records the 2026-06-22 primitives-first re-architecture).
- `cfa1-skeleton.md` — module tree, config, wiring, wrangler parse sample.
- `cfa2-kv-primitive.md` — `TenantKvStore` trait + per-backend impls + crypto
  routing + cross-backend conformance evidence.
- `cfa3-kv-adapter.md` — CF contract mapping over the primitive, `CfKv*`
  variants, REST surface, strong-vs-eventual deviation, contract conformance.
- `cfa4-workers-runtime-slice.md` — the runtime profile, module-worker dispatch,
  `env` injection evidence.
- `cfa5-env-ns-e2e.md` — the real Worker source + the passing `env.NS`
  end-to-end run (the wedge proof).
- `cfa6-do-primitive.md` — DO→Nimbus mapping, single-instance routing + per-
  instance storage decisions, catalog extension, and the HS5 cluster-routing
  handoff with the three exemplar-review decisions: per-DO-id leasing beneath the
  tenant lease, serialization granularity, and the transient-duplicate contract.
- `cfa7-do-storage.md` — per-instance storage + lifecycle + RPC, real-DO test.
- `cfa8-do-alarms-ws.md` — alarm round-trip (with retry) + hibernate→wake.
- `cfa9-closeout.md` — final state, runtime-backend registration, retro,
  follow-on-band handoff (R2/NOS, D1, full Workers surface, HS5).

## Execution Log

| CFA | Commit | Subject |
|-----|--------|---------|
| CFA0 | _on disk (docs/private untracked)_ | scaffold + primitives-first re-architecture |
| CFA1-CFA9 | pending final commit | NKV/Cloudflare foundation branch closeout, including KV wedge, Workers runtime slice, DO substrate, operator docs, verifier green, and submitted PR |

## Notes on staging order

CFA0 first so the verifier exists. **CFA1 runs first** (adapter skeleton + config,
no external dependency). **CFA2 is a no-code prerequisite gate**, not parallel
build work: it is satisfiable only once `nimbus-kv` NKV0 (through band F2) has
landed `TenantKvStore` in `nimbus-storage`. **CFA2 (KV primitive gate) before
CFA3 (KV adapter)** because the adapter is a thin surface over the primitive. **CFA3 before CFA4** so the
runtime has a binding to inject; **CFA4 before CFA5** because the end-to-end
proof runs on the runtime slice. CFA5 closes the KV wedge before the DO band.
**CFA6 (DO primitive + decisions) before CFA7/8** because single-instance routing
and per-instance storage are net-new and must be decided before code depends on
them; CFA7 (storage/lifecycle/RPC) before CFA8 (alarms/WS) because alarms and
hibernation wake *into* the storage + lifecycle machinery. CFA9 last.

Within each band: one commit, one Execution Log entry.

## Cross-lane dependencies (portfolio)

CFA sits in **L4 / Phase 1** of the Execution Order. The **DO bands (CFA6–8) and
the Workers-runtime slice (CFA4)** depend only on already-landed substrate —
`nimbus-runtime` (the `RuntimeBackend` seam), `nimbus-engine` (serialized path +
scheduler), `nimbus-services` catalog, `nimbus-server` WS — and are genuinely
NKV-independent; **CFA1 skeleton** is too. **But the KV wedge (CFA3 Workers KV
adapter + CFA5 `env.NS` proof) hard-depends on `nimbus-kv` NKV0 through band F2**
(the consolidate-now decision — see the note below): NKV0 F2 builds the
`TenantKvStore` capability in `nimbus-storage` and CFA3 rides it. So **execute
NKV0 (through F2) before CFA3/CFA5**; do not run CFA's KV wedge and the
`nimbus-kv` foundation in parallel. **Critical-path shape (owner decision,
2026-06-23): foundation-first** — `env.NS` in a real Worker is gated behind
NKV0(F2)+CFA1–CFA5 as one critical path; the wedge rides the real `nimbus-kv`
primitive from the start, with no throwaway flat seam and no later re-point (the
wedge-first alternative — a throwaway flat seam re-pointed onto the primitive
later — was weighed against foundation-first in CFA research §9 and rejected). The deferred
bands add further cross-lane edges: **R2 → NOS3** (`nimbus-s3-object-storage` S3
surface, L1/Phase 3);
**cluster-scale single-instance DO routing → HS5** (`horizontal-scaling`,
Phase 5 — see the Durable Objects critique in
`docs/private/plans/research/cloudflare-adapters-2026.md` §11 for the per-DO
leasing gap); the **full Workers-runtime surface** may consume NFS (filesystem
grant) and PIR (profile-aware pool defaults) when it grows.

> **KV consolidation onto `nimbus-kv` (owner decision 2026-06-22, "consolidate
> now").** The KV primitive is being lifted into its own program —
> `docs/private/plans/nimbus-kv-foundation-plan.md` (NKV0) + the NKV0..NKV6
> roadmap in `docs/private/plans/research/nimbus-kv-architecture-2026.md`. Nimbus
> gets a monolithic, natively Redis/Valkey-compatible store; **Cloudflare Workers
> KV becomes a thin adapter over `nimbus-kv`**, not a parallel consumer of a
> standalone flat seam. Consequence for CFA2/CFA3: the flat ordered-KV capability
> (`TenantKvStore`) lands in `nimbus-storage` as part of `nimbus-kv` **NKV0 band
> F2**, and the Workers KV adapter (CFA3) + the `env.NS`-in-a-real-Worker proof
> (CFA5) ride it. **CFA's KV wedge therefore sequences after `nimbus-kv` NKV0
> (through F2).** NKV1 adds the Tier-0 command surface but is not a hard
> prerequisite for the redb-backed wedge. When CFA2/CFA3 are executed, re-point
> them at the `nimbus-kv` surface rather than re-implementing a flat seam here.
> The Durable Objects bands (CFA6–CFA8) and the Workers-runtime slice (CFA4) are
> unaffected.

## Branch, CI, and PR workflow

- **Isolation.** All CFA work lands on the `cloudflare-adapters` worktree branch
  (code plan → PR, per [[feedback_commit_workflow]]).
- **CI is verification, not local compile.** Green local `make check`/`clippy` is
  necessary but not sufficient; the branch is pushed so the full suite runs green
  before the closeout PR.
- **PR as the terminal phase.** After CFA1–CFA9, the verifier reads
  `12 passed, 0 failed`, branch CI is green, and the final task is submitting the
  PR for the active branch (`cloudflare-adapters → main` when run standalone, or
  `codex/nkv-cloudflare-foundation → main` for the current combined worktree). Do
  not mark the plan complete until the PR exists. Never push directly to `main`.
- **Base-dependency caveat.** CFA touches `nimbus-storage` traits and
  `nimbus-services` catalog under active churn (NSR, the storage-seam waves);
  rebase onto latest `main` before opening the closeout PR to keep the diff
  reviewable.
