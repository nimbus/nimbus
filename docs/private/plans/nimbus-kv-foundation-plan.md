# nimbus-kv Foundation Plan (NKV0)

`nimbus-kv` is a **monolithic, natively Redis/Valkey-compatible (RESP2/RESP3)
data-structure store** that **delegates durable persistence to `nimbus-storage`**
and owns the in-memory cache/tiering. Memcached and Cloudflare Workers KV become
thin adapters over it; Nimbus's own features (rate limiting, sessions, queues,
caching) plug into the same Redis contract.

This is **Phase 0 (Foundation)** of the `nimbus-kv` program. Architecture,
licensing, the iroh/cluster reality, the open-source conformance strategy, and
the full NKV0..NKV6 phase roadmap are in the design doc (the contract source of
truth — do not re-derive from memory):
`docs/private/plans/research/nimbus-kv-architecture-2026.md` (§6–§10).

Owner-ratified (2026-06-22): monolithic-RESP-native-on-`nimbus-storage`;
durable-by-default + configurable in-memory cache (higher/lower/no cache; no-disk
pure-memory mode); whole surface (Tier 0+1+2) planned as a phased program, one
plan per phase, gated by **open-source Valkey conformance tests**; Cloudflare
Workers KV **consolidates onto `nimbus-kv`** (the "consolidate now" sequencing).
The CFA KV wedge's hard prerequisite is the `TenantKvStore` persistence primitive
landed **here at NKV0 band F2**; fuller Workers-KV consolidation onto the RESP
surface is NKV1.

## Why this plan exists

Nimbus has no KV primitive. The owner's decision is to make `nimbus-kv` a
first-class, natively Redis/Valkey-compatible store that many adapters and
features sit on, rather than burying flat-KV semantics inside the Cloudflare
adapter. The research established the tractable path:

- **Reuse permissive open source, never Redis 7.4+** (SSPL/AGPL). References:
  Valkey (BSD-3, semantics + tests), Kvrocks (Apache-2.0, the durable-encoding
  blueprint). Wire: the `redis-protocol` crate (MIT/Apache, RESP2+3,
  server-capable). Storage: `redb` (already a dependency).
- **The hard "Redis extras" reuse existing Nimbus primitives** — Streams →
  changefeed, Pub/Sub → reactivity, MULTI/EXEC → engine transactions (later
  phases).
- **Verifiable per-phase criteria** come from the **Valkey TCL suite in
  external-server mode** + a **redis-rs-driven harness** (the Kvrocks `gocase`
  pattern), exactly the data-driven model the Node-compat plans use.

NKV0 builds the spine every later phase stands on: the crate, a working RESP
server, the durable persistence delegation, the cache/tiering scaffold, and the
conformance harness — proven by a real `GET/SET/DEL/EXPIRE/INCR` round-trip
green through RESP → cache → `nimbus-storage`, under both RESP2 and RESP3.

NKV0 does **not** implement Redis collections (hash/list/set/zset — NKV2),
streams/pub-sub (NKV3), transactions/scripting (NKV4), the Memcached/Workers-KV
adapters (NKV5), or clustering (NKV6, gated on HS5 + secret-management).

## Scope

In scope:

- `crates/nimbus-kv/` (new crate): RESP2/RESP3 wire server on the
  `redis-protocol` crate; connection loop; `PING`/`ECHO`/`HELLO`/`QUIT`/
  `COMMAND`(minimal); the binary/subcommand entrypoint; the cache/tiering layer
  + the Redis-like config surface (`maxmemory`-style cache sizing, no-disk
  pure-memory mode, no-cache mode); `GET`/`SET`/`DEL`/`EXPIRE`/`TTL`/`INCR`
  routed through cache → storage.
- **Listener security (shared guard, not a sixth copy; tenant-bound, not
  `SELECT`-widenable).** nimbus-kv runs its own RESP listener **outside** the
  `WireProtocolAdapter` seam, so it cannot inherit that seam's `guard()`. It must
  still fail closed:
  - **Loopback bind guard — EXTRACT, do not re-implement.** Lift a shared
    `refuse_non_loopback_bind(SocketAddr) -> io::Result<()>` helper — one canonical
    definition of "loopback" (incl. unspecified-address and IPv4-mapped-IPv6
    handling) — into a common location (`nimbus-core` or a small `nimbus-net`) and
    have nimbus-kv call it, **not a sixth hand-rolled copy** of the most
    security-critical guard in the tree. (The four existing bind guards already
    delegate to stdlib `IpAddr::is_loopback()`; converge them on the shared helper
    rather than diverge.) The listener **refuses any non-loopback bind**.
  - **Tenant resolution — credential-bound, `SELECT` deliberately narrowed.** RESP
    authenticates a *connection* (`AUTH` / `HELLO 3 AUTH`); each generated
    credential **binds to exactly one `TenantId`** (the DynamoDB `AccessKeyRegistry`
    shape — reuse that type, never MongoDB's `$db`-name model). The wire token never
    widens the tenant: Nimbus deliberately **narrows Redis `SELECT n`** so
    cross-tenant keyspace access is a hard error. The canonical in-tree rule
    ("authentication decides the tenant; a wire-supplied name never does") binds
    here. (`SELECT`/numbered-DB selection is itself NKV1+; NKV0 exposes no `SELECT`,
    so the trap cannot open in this phase — but the credential→tenant binding is
    specified now so it never does. See the design doc §11 "RESP multi-tenancy and
    the `SELECT` trap".)
  - **Auth posture: dev vs production.** Generated dev-cred auth is a
    **loopback-only, single-tenant convenience**, NOT a production multi-tenant auth
    model. The precondition for ever binding a routable address is the
    per-credential→tenant binding *enforced* (the DynamoDB `AccessKeyRegistry`
    precondition, mirroring `nimbus-mongodb`'s loopback-lift constraint) **plus TLS
    termination** (RESP `AUTH`/SCRAM run plaintext). Production credential
    provisioning / rotation / revocation is a named follow-on
    (service-identity-provider-auth / secret-management lane), not NKV0.

  RESP-unauth-on-`0.0.0.0` is a standing CVE class. The completion gate asserts the
  refusal **and a positive cross-tenant isolation property** (a credential bound to
  tenant A cannot read tenant B's keys via any wire token) as passing `#[test]`s in
  the `nimbus-kv` crate. Behavioral enforcement runs in CI via `rust-workspace-tests`
  — the same path as the `mongodb`/`dynamodb` refusal tests
  (`listener_rejects_non_loopback_bind_address` / `*_unauthenticated_*`); the shell
  verifier only checks the named tests' presence as a `/goal` scaffold, since it
  cannot run the plan gate in CI (the plan lives under local-only `docs/private/`).
- `crates/nimbus-storage/src/traits/` (new flat KV capability — `TenantKvStore`:
  ordered keys, range/prefix scan, atomic batch, get/put/delete, value + metadata
  + TTL field) **behind a swappable storage-engine trait**, with the **`redb`
  impl** as the default embedded backend (full cross-backend macro impl is NKV1).
  At-rest crypto via `TenantKeyring`; writes atomic through the engine commit
  path; **RMW ops (`INCR`, conditional `SET`, expiry) go through the engine
  transaction, never a read-then-write race**.
- The **conformance harness**: a redis-rs-driven integration harness that spawns
  the `nimbus-kv` binary, plus a runner script invoking the **Valkey TCL suite in
  external mode** (`runtest --host --port --single unit/... --ignore-encoding`)
  with a documented `--skipfile` (curating internals-dependent `DEBUG`/
  `OBJECT ENCODING` tests out of scope), sliceable per phase, run under RESP2 and
  RESP3.
- `docs/private/operating/nimbus-kv.md` (config + conformance contract);
  `scripts/verify-nimbus-kv-foundation.sh`; routing in `AGENTS.md` +
  `docs/private/plans/README.md`; proof under
  `docs/private/plans/proof/nimbus-kv-foundation/`.

Out of scope (later NKV phases — one plan each):

- **NKV1** Tier 0 core completeness (full string/incr/expire/keyspace/scan +
  cross-backend `TenantKvStore` across all 5 storage backends) — and the point at
  which **Workers KV fully consolidates onto the `nimbus-kv` RESP surface**. (The
  CFA KV wedge's hard prerequisite — the `TenantKvStore` trait + redb impl —
  already lands at **NKV0 F2**; NKV1 is cross-backend completeness, optional for
  the redb-backed wedge.) **`scan`/`keyspace`/size surfaces apply the same
  skip-on-read logical-expiry filter as the read path** (a key past `expire_at` is
  invisible to `scan`, `get`, and count even if not yet swept), and pagination sets
  `list_complete` only on an exhausted cursor — a short/empty page is never "done".
- **NKV2** collections (hash/list/set/zset, Kvrocks encoding). **NKV2 is the first
  band to lift Kvrocks (Apache-2.0) encoding, so it must extend
  `scripts/verify-third-party-attribution.sh` scope to `crates/nimbus-kv` (or add
  per-file `Adapted from kvrocks@<sha>` headers + an NKV verifier condition).** NKV0
  lifts no third-party source — only references Valkey/Kvrocks semantics — so the
  attribution gate's current nimbus-guest/nimbus-libkrun scope is correct here.
  (**Threat model:** `deny.toml` is a *dependency-license* gate — it cannot see
  first-party source cribbing from Redis 7.4+; source-level provenance is enforced
  separately by the attribution-header gate, which is why extending it to
  `crates/nimbus-kv` at the first source lift — NKV2 — is load-bearing, not
  optional. The NKV2 plan must make that gate-extension a hard entry condition.)
- **NKV3** streams (→changefeed) + pub/sub (→reactivity); **NKV4** transactions
  (→engine txns) + Lua/EVAL + bitmap/geo/hll; **NKV5** Memcached adapter +
  post-wedge Workers-KV hardening/re-pointing if CFA needs a richer `nimbus-kv`
  internal surface; **NKV6** cluster (gated on HS5 + secret-management).
- Implementing the Redis `DEBUG`/`OBJECT ENCODING` introspection surfaces beyond
  the minimum the smoke subset needs (tracked via the skipfile; grown per phase).

## Conformance-harness contract

The gate that every phase reuses (NKV0 builds it, sized to a smoke subset):

- **Oracle:** the **Valkey** TCL suite (BSD-3), external mode —
  `runtest --host 127.0.0.1 --port <p> --single unit/type/<t> --skipfile
  tests/nimbus-kv-skip.txt`, run under RESP2 and RESP3 (HELLO). Never Redis 7.4+.
- **Driver harness:** a redis-rs integration harness that spawns the `nimbus-kv`
  binary (the `REDISRS_SERVER_BIN` / Kvrocks-`gocase` `-binPath` pattern) and
  asserts command behavior.
- **Honest, budgeted skip accounting:** the skipfile has two delimited sections —
  encoding/internals (`DEBUG`/`OBJECT ENCODING`/`INFO`, allowed) and behavioral
  (**capped, ~zero for NKV0**, each citing a tracking issue). The runner **asserts
  a minimum count of *passing* behavioral assertions**, so an all-skipped run
  cannot read green — the Node-compat false-green discipline, made
  machine-checkable.
- **Per-phase slicing:** each later phase's verifier asserts its Valkey
  `unit/type/<t>` subset green; NKV0 asserts only a smoke subset
  (`GET/SET/DEL/EXPIRE/INCR`) + the redis-rs round-trip.

## Ledger

| NKV0 | Description | Status |
|------|-------------|--------|
| F0 | Scaffold plan + verifier at `scripts/verify-nimbus-kv-foundation.sh` (9 conditions, mostly FAIL until later bands flip them); baseline proof at `docs/private/plans/proof/nimbus-kv-foundation/nkv0-baseline.md`; routing in `AGENTS.md` + `docs/private/plans/README.md` (program + this plan). Design doc `nimbus-kv-architecture-2026.md` is the program/architecture source of truth. | done |
| F1 | `crates/nimbus-kv/` crate + RESP2/RESP3 wire server on the `redis-protocol` crate: async connection loop, command frame parse + reply encode, `PING`/`ECHO`/`HELLO`(RESP3 upgrade)/`QUIT`/`COMMAND`(minimal). Entrypoint pinned: the crate exposes `run_listener`/`serve`, and `nimbus-bin` gets a thin subcommand calling it. The listener boots loopback-only with generated dev credentials — it **refuses any non-loopback bind** (via the **shared `refuse_non_loopback_bind` helper**, not a hand-rolled copy) and **requires auth**, each credential bound to exactly one `TenantId` (DynamoDB `AccessKeyRegistry` shape; `SELECT` cross-tenant access is a hard error). Named tests assert: a redis-rs client connects, `HELLO 3` negotiates RESP3, `PING`/`ECHO` round-trip, **a non-loopback bind is refused** (`listener_rejects_non_loopback_bind`, asserting `ErrorKind::InvalidInput`), **an unauthenticated command is rejected**, and **a credential bound to tenant A cannot read tenant B's keys**. These `#[test]`s run in CI (`rust-workspace-tests`), the real behavioral gate. | done |
| F2 | Flat KV capability in `nimbus-storage`. New `TenantKvStore` trait in `crates/nimbus-storage/src/traits/`: `kv_get`, `kv_put(value, metadata?, expire_at?)`, `kv_delete`, `kv_scan(prefix, cursor, limit)` (ordered range/prefix), and an atomic multi-key batch — **behind a swappable storage-engine trait** so the engine (redb / fjall / rust-rocksdb) is not hardwired. Exemplar review: every trusted comparable (TiKV, CockroachDB, Kvrocks) runs on an **LSM, not a B-tree**, and redb has **no native TTL** — so abstract the engine now and **benchmark redb-vs-fjall on the real write/TTL workload inside this foundation phase, gating before NKV2 collections commit** (the second engine + the benchmark stay in F2, not deferred). The benchmark **publishes a measured durable-write-throughput number as a gate artifact** (not just a redb-vs-fjall comparison), and the throughput analysis names **all three serialization points that compound on the durable write path** — redb's own `begin_write` lock, the per-tenant engine `sequence_gate`, and the storage write semaphore (`TENANT_WRITE_PARALLELISM = 1`) — so "Redis/Valkey-compatible" is never read as throughput-parity: the durable tier is ≈ tens of thousands of writes/sec/tenant, single-writer-serialized, NOT Redis cache parity (~133K req/s); cache-class latency is the no-disk / cache modes only. Default impl = **`redb`** (ordered B-tree + range API + atomic multi-table txn = the storage-atomicity invariant). **TTL has no native engine support: sweep via an expiry-ordered secondary index written in the *same* atomic txn as the value, plus skip-on-read, plus a bounded background pass** — the swept index must be maintained atomically with the value write (never a second un-transacted write). **The background sweep is a transactional compare-and-delete:** within the *deleting* write txn it re-reads the key's current `expire_at` from the value record and deletes ONLY if still `<= now`, dropping the matching expiry-index entry in that same txn — the index is a *hint*, the value record's `expire_at` is truth. So a key whose TTL is extended by a concurrent `SET … EX` / `PERSIST` racing the sweep is **never phantom-deleted** (the classic Redis-clone data-loss trap — the single-writer semaphore alone does NOT prevent it, because a decision-read in txn A and a delete in txn B can straddle the re-set), and a stale index entry can never trigger a second delete. An F2 completion-gate test asserts: a key whose expiry is extended by a racing `SET … EX` survives the sweep, and an expired key IS deleted with its index entry. **RMW atomicity is a hard requirement** (`INCR`, conditional `SET`, expiry, later `CAS`) — through the engine transaction, never a read-then-write race (the unsafe path that got ScyllaDB's Redis API deleted). At-rest encryption via `TenantKeyring` — the **net-new NC (nimbus-crypto) trait**, not pre-existing, reusing the existing `encrypted_redb`/`LocalKeyProvider` DEK path (see `nimbus-crypto-extraction-plan.md`; NC is the practical predecessor). (Cross-backend macro impl across SQLite/Postgres/MySQL/libSQL = NKV1.) On-disk format versioning + fail-closed validators are inherited from `nimbus-storage`'s `CURRENT_STORAGE_FORMAT_VERSION` regime (`format.rs`); the redb→fjall engine choice is gated by the F2 benchmark before any durable data is written in a release build, so it never strands customer data — pre-launch the posture is breaking/wipe-and-reload, no migration shim. | done |
| F3 | Cache/tiering scaffold in `nimbus-kv`. The durable-by-default model: an in-memory cache in front of the `TenantKvStore` durable tier, write-through by default, plus the Redis-like config surface — cache sizing (`maxmemory`-style), `no-disk` pure-in-memory mode (Redis-style, no `nimbus-storage` backing), and `no-cache` mode. Route `GET`/`SET`/`DEL`/`EXPIRE`/`TTL`/`INCR` through cache → storage with the configured tiering. **Cache-coherency rule: atomic RMW ops (`INCR`, conditional `SET`, expiry) execute against the engine-transactional tier (durable when configured; the in-memory redb transaction in no-disk mode) — never against the cache directly; the cache is updated or invalidated *from the committed result*, never from a pre-commit guess** — a cached value can never diverge from committed truth. The linearization point is the **single-writer serialization** (redb's `begin_write` lock + the 1-permit storage write semaphore), which holds identically in disk-backed and no-disk modes — so the no-disk mode (which has no "durable tier") still has a well-defined atomicity source. **Expiry coherency:** the cache entry carries the durable `expire_at` (not an independent cache TTL), a cache hit applies the **same skip-on-read logical-expiry check** as the storage read path, and `EXPIRE`/`PERSIST`/`SET … EX` invalidate or rewrite the cached entry's expiry from the committed result in the same step — so a logically-expired value is never served from cache (a security-adjacent guarantee for session/lease/rate-limit keys). Tests assert: durable+cache round-trip survives a restart; no-disk mode is volatile; no-cache mode reads straight through; **concurrent `INCR` through the cache stays consistent with the durable counter under BOTH disk-backed and no-disk modes**; and an extend-then-shorten-TTL sequence through the cache path honors the durable `expire_at` exactly at the boundary. (Eviction policy / `maxmemory-policy` is NKV5.) | done |
| F4 | The conformance harness. A redis-rs integration harness under `crates/nimbus-kv/tests/` that spawns the `nimbus-kv` binary (binary-path env, readiness via `PING`) and drives commands. Plus `scripts/nimbus-kv-conformance.sh` invoking the Valkey TCL suite in external mode against a spawned `nimbus-kv` (`runtest --host --port --single ... --skipfile tests/nimbus-kv-skip.txt`), under RESP2 and RESP3, sliceable per phase. `tests/nimbus-kv-skip.txt` is a **budgeted, machine-checked** artifact: **two delimited sections** — `# --- encoding ---` (allowed, `--ignore-encoding`-class `DEBUG`/`OBJECT ENCODING`/`INFO`) and `# --- behavioral ---` (**capped at a small committed integer — 0 for NKV0's smoke subset**), each behavioral skip citing a tracking issue, not a bare reason. The conformance runner **asserts a minimum count of *passing* behavioral assertions** (parsing the `runtest` output) so an empty/all-skipped run cannot read green, and the proof **pins the exact `valkey` checkout SHA + `unit/type/<t>` slice** for reproducibility — closing the forgeable-green failure mode the Node-compat plans warn about. **Wire a CI workflow** that installs `tclsh` + a pinned `valkey` checkout and runs the conformance script under RESP2/RESP3 (gated into the merge summary once green on-branch). Document how a phase adds its `unit/type/<t>` slice. | done |
| F5 | Smoke green + closeout. A real `GET`/`SET`/`DEL`/`EXPIRE`/`TTL`/`INCR` round-trip green through RESP → cache → `nimbus-storage`, via the redis-rs harness, under RESP2 and RESP3, plus the Valkey `unit/type/string` (or a documented smoke subset) green in external mode with the skipfile. Operator/dev doc `docs/private/operating/nimbus-kv.md` (config surface, conformance contract, the durable+cache+no-disk modes). Plus a minimal **observability surface** — per-command latency, cache hit/miss ratio, durable-write queue depth/latency (surfacing the single-writer ceiling), connected-clients, and a readiness probe distinct from `PING` (the HS3 "observability before the data plane" discipline). Flip every ledger row to `done`; append Execution Log; move plan to `docs/private/plans/archive/`; verifier `plan_file()` accepts both paths; update routing. **Push, verify, PR:** push the active branch, full branch CI green, verifier `9 passed, 0 failed`, and submit a PR to `main`. For the current combined branch, F5 is a checkpoint and the final submitted PR is the NKV/Cloudflare PR after CFA9. Never push directly to `main`. | done |

## Completion Gate

`bash scripts/verify-nimbus-kv-foundation.sh` exits 0 with summary line
`9 passed, 0 failed`. The 9 conditions:

1. Plan file exists (active or archived path).
2. Routing entries exist in both `CLAUDE.md` (= `AGENTS.md`) and
   `docs/private/plans/README.md` naming this plan.
3. F0: design doc `nimbus-kv-architecture-2026.md` and baseline proof
   `nkv0-baseline.md` exist.
4. F1: `crates/nimbus-kv/` exists, depends on `redis-protocol`, and contains a
   RESP server with a `PING` handler + a binary/subcommand entrypoint.
5. F2: a `TenantKvStore` trait exists in `nimbus-storage` with `kv_*` methods,
   behind a swappable storage-engine trait with a SECOND engine (`fjall`) compiled
   + a published redb-vs-fjall benchmark artifact, a `redb` impl, and a TTL sweep
   that is a transactional compare-and-delete (a key whose TTL is extended by a
   racing `SET … EX` survives the sweep; an expired key is deleted with its index
   entry).
6. F3: the cache/tiering scaffold + config surface (`maxmemory`/`no-disk`/
   `no-cache`) is present in `nimbus-kv`, with a behavioral tiering test (incl.
   concurrent-`INCR` coherency under BOTH disk-backed and no-disk modes) and an
   expiry-coherency test (a cache hit honors the durable `expire_at`; a
   logically-expired value is never served from cache).
7. F4: the conformance harness exists — a redis-rs spawn-the-binary integration
   test + `scripts/nimbus-kv-conformance.sh` (Valkey external mode) + a **two-section
   budgeted** `tests/nimbus-kv-skip.txt` (encoding allowed / behavioral capped),
   with the runner asserting a **minimum count of passing behavioral assertions**
   (so an all-skipped run cannot read green).
8. F5: a `GET/SET/DEL/EXPIRE/INCR` smoke test passes under RESP2+RESP3, operator
   doc exists, every ledger row is `done`, and the latest `ci.yml` run for the
   active branch is green and matches the current branch head
   (`NIMBUS_VERIFY_CI_BRANCH` may override the branch for explicit closeout
   checks).
9. F1 security: the RESP listener refuses a non-loopback bind (via the shared
   `refuse_non_loopback_bind` helper), requires generated dev-cred auth, and binds
   each credential to exactly one tenant (DynamoDB-style; `SELECT` cross-tenant is a
   hard error) — `nimbus-kv` is outside the `WireProtocolAdapter` guard seam, so the
   verifier requires named **assertion-bearing** tests to exist (a non-loopback bind
   is refused with `InvalidInput`, an unauthenticated command is rejected, and a
   tenant-A credential cannot read tenant-B keys), which `rust-workspace-tests` runs
   in CI. A green gate must never admit a no-auth or cross-tenant-readable RESP
   listener on `0.0.0.0`.

## Trust targets

- **Before NKV0**: no KV primitive.
- **After F1**: a real RESP2/RESP3 server boots and a stock Redis client
  (`redis-rs`) connects and round-trips `PING`/`ECHO`.
- **After F3**: `nimbus-kv` is durable-by-default on `nimbus-storage` with a
  configurable cache and a Redis-style no-disk mode — the persistence identity is
  real, not a stub.
- **After F5**: a stock Redis client runs `GET/SET/DEL/EXPIRE/INCR` against
  `nimbus-kv` and a slice of the **Valkey** test suite passes in external mode —
  the conformance gate every later phase reuses is proven. This is a credible
  (if minimal) durable Redis/Valkey-compatible store — **foundation-grade, not
  production-ready**: a GA "production-ready" claim additionally requires the
  **NKV-DR** backup/restore/DR phase (design doc §9) and the F5 observability
  surface above.
- **End-state honesty**: NKV0 ships strings + expiry only. Collections, streams,
  pub/sub, transactions, the Memcached/Workers-KV adapters, and clustering are
  later NKV phases. The CFA KV wedge rides the `TenantKvStore` primitive from
  **NKV0 F2**; fuller Workers-KV consolidation onto the RESP surface is **NKV1**.

## Proof directory

`docs/private/plans/proof/nimbus-kv-foundation/`:

- `nkv0-baseline.md` — starting state (no `nimbus-kv` crate, no KV capability),
  the ratified decisions, the consolidate-now sequencing, the references.
- `f1-resp-server.md` — RESP2/3 server, redis-rs connect + HELLO 3 evidence.
- `f2-kv-primitive.md` — the `TenantKvStore` trait + swappable storage-engine
  trait + redb impl, atomicity + TTL + RMW-via-engine-txn evidence, and the
  redb-vs-fjall write/TTL benchmark.
- `f3-cache-tiering.md` — durable+cache+no-disk+no-cache mode evidence (incl. the
  restart-survival + volatility tests).
- `f4-harness.md` — the redis-rs spawn harness + Valkey external-mode runner +
  skipfile rationale.
- `f5-closeout.md` — smoke green (RESP2+RESP3), Valkey subset green, retro,
  hand-off to NKV1.

## Execution Log

| NKV0 | Commit | Subject |
|------|--------|---------|
| F0 | `b9748aae2` | scaffold nimbus-kv Foundation plan + verifier + baseline |
| F1 | `d85dacd0f` | add authenticated RESP2/RESP3 nimbus-kv listener |
| F2 | `3222babfc` | add TenantKvStore trait, redb impl, fjall bench, TTL tests |
| F3 | `b892ff9bc` | add cache/tiering modes and route string commands |
| F4 | `0fb43652d` | add redis-rs spawned-binary harness + Valkey external-mode conformance |

## Notes on staging order

F0 first so the verifier exists. F1 (RESP server) and F2 (storage capability)
are near-parallel but F1 is listed first so there is a server to drive. F3
(cache/tiering) needs F2's durable tier to sit in front of. F4 (harness) needs
F1's binary to spawn. F5 proves the whole spine end-to-end and closes out. One
commit per band, one Execution Log entry.

## Branch, CI, and PR workflow

- **Isolation.** All NKV0 work lands on the `nimbus-kv` worktree branch (code
  plan → PR, per [[feedback_commit_workflow]]).
- **CI is verification.** Branch pushed; full suite green before the closeout PR.
  The Valkey-external-mode conformance lane is an **explicit F4/F5 deliverable**: a
  CI workflow that installs `tclsh` + a pinned `valkey` checkout and runs
  `scripts/nimbus-kv-conformance.sh` under RESP2 and RESP3. Gate it into the merge
  summary only once proven green on the branch (the Node-compat-lane discipline) —
  it is a deliverable to build, not a lane asserted as already running.
- **PR as the last step.** Verifier `9 passed, 0 failed` + branch CI green → open
  a PR to `main` for a standalone NKV0 branch. When NKV0 is executed as the first
  leg of the combined NKV/Cloudflare branch, F5 is a checkpoint: record the proof,
  keep the branch moving into CFA, and treat the submitted combined NKV/Cloudflare
  PR after CFA9 as the terminal closeout item. The plan is not complete until that
  PR exists. Never push directly to `main`.
- **Base-dependency caveat.** NKV0 adds a `nimbus-storage` trait under active
  churn (the storage-seam waves); rebase onto latest `main` before the closeout
  PR.
