# NKV0 (Foundation) — Baseline Proof

Starting state for the nimbus-kv Foundation plan
(`docs/private/plans/nimbus-kv-foundation-plan.md`), captured 2026-06-22.

## Starting state

- **No `nimbus-kv` crate.** `crates/` has no `nimbus-kv`; no RESP server exists.
- **No KV capability in `nimbus-storage`.** No `OrderedKvStore`/`TenantKvStore`
  trait; the storage traits today are document/object/metadata oriented
  (`TenantPointRead/Write`, `ObjectMetaStore`, the SEQ historical/changefeed
  surface).
- **No verifier / operator doc / proof bundle** for nimbus-kv prior to NKV0.
- `redb` already exists as a `nimbus-storage` embedded backend (the chosen
  default durable engine for the KV capability).

## Ratified decisions (owner, 2026-06-22)

1. **Monolithic `nimbus-kv`, natively RESP/Redis-Valkey, on `nimbus-storage`.**
   One crate owns RESP + data structures + encoding + cache/tiering; persistence
   delegates to a flat ordered-KV capability in `nimbus-storage`. Reconciles with
   the earlier "`TenantKvStore` in nimbus-storage, not a standalone crate" — the
   flat persistence stays in `nimbus-storage`; `nimbus-kv` is a consumer crate.
2. **RESP is native; Memcached + Cloudflare Workers KV are adapters** (NKV5).
3. **Durable-by-default + configurable cache** (higher/lower/no cache; no-disk
   pure-memory mode) — a Redis-like config surface owned by `nimbus-kv`.
4. **Whole surface (Tier 0+1+2) planned as a phased program**, one plan per phase
   (NKV0..NKV6), gated by the **Valkey TCL suite (external mode) + a
   redis-rs-driven harness** (the Kvrocks `gocase` precedent). Full program in
   `docs/private/plans/research/nimbus-kv-architecture-2026.md` §6–§10.
5. **Cloudflare sequencing = consolidate now:** the Cloudflare Workers KV wedge
   rides the `TenantKvStore` seam from **NKV0 F2** (one KV substrate from day one;
   zero duplication). Fuller Workers-KV consolidation onto the RESP/native
   `nimbus-kv` surface is **NKV1**.

## References (permissive only; never Redis 7.4+ SSPL/AGPL)

- Semantics + tests: **Valkey** (BSD-3). Durable-encoding blueprint: **Apache
  Kvrocks** (Apache-2.0). RESP wire crate: **`redis-protocol`** (MIT/Apache).
  Storage engine: **`redb`** (already a dependency). Conformance driver:
  **`redis-rs`** (MIT, `REDISRS_SERVER_BIN`).

## iroh / cluster reality (recorded so it isn't re-assumed)

iroh does **not** provide a usable KV "for free" (iroh-blobs = content-addressed;
iroh-docs CRDT = out of scope). Cluster KV is **Raft/openraft (strong), not
CRDT**, and is explicit deferred work across HS5 + secret-management — sequenced
as **NKV6**, gated, not automatic. Single-node `nimbus-kv` on `nimbus-storage`
gets strong consistency + at-rest crypto for free.

## Verifier baseline

`bash scripts/verify-nimbus-kv-foundation.sh` at F0 is expected to pass
conditions 1–3 (plan, routing, design doc + this baseline) and fail 4–9 until
the corresponding bands land — `3 passed, 6 failed` of 9. Condition 9 is the
F1 security gate (the RESP listener refuses a non-loopback bind + requires
dev-cred auth, re-implemented because `nimbus-kv` is outside the
`WireProtocolAdapter` guard seam). That FAIL-until-built state is the correct
day-one baseline (the verifier ships in F0 so `/goal` is checkable from the
start).

Verified 2026-06-27 on branch `codex/nkv-cloudflare-foundation`:

- `bash -n scripts/verify-nimbus-kv-foundation.sh` — syntax clean.
- `bash scripts/verify-nimbus-kv-foundation.sh` — `3 passed, 6 failed`.
- `bash scripts/verify-cloudflare-adapters.sh` — `3 passed, 9 failed`
  (expected CFA0 baseline with the added security-posture gate).
- `bash docs/private/plans/proof/storage-seams/verify-storage-seams.sh` —
  `14 passed, 0 failed`.
