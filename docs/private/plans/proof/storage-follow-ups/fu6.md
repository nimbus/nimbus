# FU6 — Object-Manifest Seam Inversion And Libsql Replica-Cache Relocation

Branch `codex/fu6-seams`, based on `origin/main` @ `22c5cdd62`.

| Commit | Scope |
| --- | --- |
| `81d0adada` | FU6a — invert the nimbus-fs object manifest seam |
| `4fc8b9ade` | FU6b — give the libsql replica cache its own sqlite module |
| `8575bb359` | review follow-up — move the store-coverage claim to a build-time pin |

---

## FU6a — The Unfenced Object-Write Seam

### What the seam was

`ObjectRwBackend` (`crates/nimbus-fs/src/object/mod.rs`) held
`Arc<dyn ObjectMetaStore + Send + Sync>` — the storage-level sync trait whose
write half (`put_object_manifest`, `delete_object_manifest`,
`put_multipart_upload`, `delete_multipart_upload`) reached a tenant store
directly. Each of those calls assigned a commit sequence outside the tenant
committer actor.

Since SUC2.2 the engine's `TenantObjectMeta` is the only fenced route to object
metadata: the sequence is assigned inside the committer actor under the
committer lease, the batch is persisted through the fenced provider path, the
durable and applied watermarks advance, and subscriptions fan out. A store-level
write does none of that, and lets two writers on the same key interleave.

The seam was dormant, not live: nothing in production constructs
`ObjectRwBackend`. It was still publicly reachable, which is what FU6 closes.

### Consumer inventory

Taken before any edit, across the whole workspace.

**Write half (`put_object_manifest`, `delete_object_manifest`,
`put_multipart_upload`, `delete_multipart_upload` on the storage trait):**

| Consumer | Nature |
| --- | --- |
| `nimbus-fs` `ObjectRwBackend` (`read.rs`, `range.rs`, `write.rs`) | manifest put/delete only; never multipart |
| `nimbus-storage` `src/tests/object_meta.rs` | the crate's own read-half coverage, seeding rows to read back |

Nothing else. Specifically:

- `nimbus-engine` `persistence/tenant/objects.rs` dispatches **reads only** —
  four `pub(crate)` methods over `match_tenant_persistence!`.
- `nimbus-s3` and `nimbus-server` do not use this trait at all. They use the
  **async `S3ObjectMeta` trait declared in `nimbus-s3`**, which `nimbus-server`
  implements over the engine. That is the same inversion, already in-tree, and
  is the precedent this change follows.
- `ObjectRwBackend` and `ExternalFuseObjectMount` have **zero constructors
  outside `nimbus-fs`**.
- `nimbus-storage` has no `tests/` integration directory, so there is no
  out-of-crate test consumer either.

Conclusion: the write half had no legitimate remaining consumer. Pre-launch, it
is deleted rather than deprecated.

### Decision 1 — nimbus-fs declares the capability it needs

`crates/nimbus-fs/src/object/manifests.rs` declares `ObjectManifestStore` with
exactly the four operations `ObjectRwBackend` performs:

```rust
pub trait ObjectManifestStore: Send + Sync {
    fn get_manifest(&self, bucket: &str, key: &str) -> Result<Option<ObjectManifest>>;
    fn list_manifests(&self, bucket: &str, prefix: &str, limit: usize) -> Result<Vec<ObjectManifest>>;
    fn put_manifest(&self, manifest: &ObjectManifest) -> Result<()>;
    fn delete_manifest(&self, bucket: &str, key: &str) -> Result<()>;
}
```

Whoever mounts the backend supplies the implementation. Tests implement it over
an in-memory map; future production wiring implements it over the engine's
`TenantObjectMeta` through a blocking adapter (the trait is synchronous because
`deno_fs::FileSystem` is). The trait's doc comment states that fencing contract
explicitly, names `TenantObjectMeta` as the required route, and says why a raw
store implementation is valid only in a test that owns its whole tenant.

**Dependency-graph check.** `nimbus-fs` depends on `nimbus-blob`,
`nimbus-core`, `nimbus-runtime`, and `nimbus-storage`. `nimbus-engine` does not
depend on `nimbus-fs`, so a `nimbus-fs → nimbus-engine` edge would not cycle —
but it would invert the Seam C layering, putting the filesystem crate above the
engine. Declaring the trait locally avoids that edge entirely: `nimbus-fs`
keeps `nimbus-storage` only for the manifest DTOs (`ObjectManifest`,
`ObjectManifestAttributes`), which stay shared. Only the capability is
inverted.

### Decision 2 — the storage trait loses its write half

`ObjectMetaStore` is renamed `ObjectMetaRead` and keeps only the four read
methods. The name now matches its siblings (`TenantPointRead`,
`TenantRangeScan`) and states the constraint in the type name. Its doc comment
records why there is no write half and no publicly reachable substitute.

Mechanical consequences:

- `provider_impls.rs`: the macro is `impl_object_meta_read!`, carrying the four
  read methods, invoked for `TenantStore`, `SqliteTenantStore`,
  `MemoryTenantStore`, and (feature-gated) `PostgresTenantStore`,
  `MySqlTenantStore`, `LibsqlReplicaTenantStore`.
- `StorageEngine`'s supertrait list ends `+ ObjectMetaRead`.
- The four write helpers become `#[cfg(test)] pub(crate)` free functions named
  `*_direct` (`put_object_manifest_direct` and siblings), re-exported
  `pub(crate)` from `traits/mod.rs` under `#[cfg(test)]`. They are `cfg(test)`
  so a non-test build cannot reach them and so they do not trip `-D warnings`
  as dead code; `CommitEntry` and `TenantPointWrite` imports moved behind the
  same gate for that reason.
- `src/tests/object_meta.rs` seeds through those free functions. Its eight
  behavioral tests are otherwise unchanged and still assert commit sequences,
  written table names, prefix ordering, bucket isolation, and sqlite reopen
  persistence.
- `docs/private/architecture/runtime/adapter-boundary.md` and the
  `SqliteTenantStore::writer_slot` comment lose their stale references to
  store-level object-manifest writes.

**Acceptance:** no publicly reachable unfenced object-write API remains in
`nimbus-storage`. `ObjectMetaRead` is read-only; the write helpers are
`cfg(test) pub(crate)`; no other store-level object-write entry point exists.

---

## FU6b — Libsql Replica-Cache Relocation

Three cfg'd functions moved out of the sqlite modules into
`crates/nimbus-storage/src/sqlite/replica_cache.rs`:

| Function | Was | Old gate |
| --- | --- | --- |
| `SqliteTenantStore::reconcile_replica_durable_records_batch` | `sqlite/journal.rs` | `#[cfg(any(test, feature = "libsql"))]` |
| `rebuild_sqlite_indexes_from_loaded_schema` | `sqlite/schema.rs` | `#[cfg(feature = "libsql")]` |
| its `pub(crate) use` re-export | `sqlite.rs` | `#[cfg(feature = "libsql")]` |

Both bodies moved verbatim (verified by diff, below). `sqlite/journal.rs` and
`sqlite/schema.rs` now carry no libsql-conditional code at all, and `sqlite.rs`
has no cfg'd re-export: `libsql.rs` imports
`crate::sqlite::replica_cache::rebuild_sqlite_indexes_from_loaded_schema`
directly from the owning module.

### Why `sqlite/replica_cache.rs` and not `src/libsql/`

`src/libsql/` is declared `#[cfg(feature = "libsql")] pub mod libsql;`, so
nothing needed by the provider-free `cfg(test)` build can live there.
`crate::tests::sqlite_foundation::journal` exercises
`reconcile_replica_durable_records_batch` directly, in an **ungated** test
module, and the brief binds those tests to stay unchanged. A new top-level
module would instead force widening several sqlite internals
(`acquire_writer_connection`, `release_writer_connection`,
`latest_sequence_in_conn`, `put_metadata_in_conn`, the private `path` and
`fault_injector` fields, the `observe_sqlite_*` hooks) from `pub(super)`/private
to `pub(crate)`.

A child module of `sqlite` needs none of that: items visible in `crate::sqlite`
are visible in its descendants. So the relocation is one concept-owned module,
one gate at the module declaration, and no widened visibility anywhere.

Two libsql cfg attributes remain in the sqlite tree, both now in libsql-owned
positions:

1. `#[cfg(any(test, feature = "libsql"))] pub(crate) mod replica_cache;` — the
   module gate, carrying `test` for the sqlite-foundation coverage.
2. `#[cfg(feature = "libsql")]` on `rebuild_sqlite_indexes_from_loaded_schema`
   itself — no test consumes it, so a provider-free test build would otherwise
   see it as dead code under `-D warnings`.

### Behavior invariance

Both moved bodies are byte-identical to their previous form (indentation aside
for the method, which changed nesting depth by zero — it stayed inside an
`impl SqliteTenantStore` block):

```
$ git show HEAD:...sqlite/journal.rs | sed -n '293,368p' | sed 's/^    //' > old
$ sed -n '/pub(crate) fn reconcile.../,/^    }$/p' ...replica_cache.rs | sed 's/^    //' > new
$ diff old new
RECONCILE BODY IDENTICAL (modulo indentation)

$ git show HEAD:...sqlite/schema.rs | sed -n '88,94p' > old
$ sed -n '/^pub(crate) fn rebuild.../,/^}$/p' ...replica_cache.rs > new
$ diff old new
REBUILD BODY IDENTICAL
```

The sqlite suite is unchanged and green: `-E 'test(sqlite_foundation)'` →
**68 run, 68 passed**.

---

## Review Follow-Up — The Store-Coverage Claim Is a Build-Time Pin

Review raised one accepted P2 against FU6a: the surviving
`object_meta_read_trait_covers_all_tenant_stores` was a `#[test]` whose body
only instantiated an empty generic function. That is a compilation check
wearing a test's clothes, and it violates the repo's "tests verify behavior"
contract — it can never fail at runtime, and a green tick from it says nothing
about what any store returns.

The claim itself is worth keeping, so it moved rather than disappeared. It now
lives as a `const _: () = { ... }` item at the foot of
`crates/nimbus-storage/src/traits/provider_impls.rs`, directly beneath the
`impl_object_meta_read!` invocations it guards:

- **It is checked in every build shape**, not only under `cfg(test)` — the
  release build now carries the same guarantee the test used to claim.
- **It sits with the impls**, so a dropped macro line and its pin are one
  screen apart instead of one crate apart.
- **The remote stores stay feature-gated**, so the claim still tracks the
  build rather than silently narrowing to the embedded stores.

Fail-before evidence: dropping `SqliteTenantStore` from
`impl_object_meta_read!(TenantStore, SqliteTenantStore, MemoryTenantStore)`
turns a clean `cargo check -p nimbus-storage` into

```
error[E0277]: the trait bound `sqlite::SqliteTenantStore: ObjectMetaRead` is not satisfied
   --> crates/nimbus-storage/src/traits/provider_impls.rs:767:28
    |
767 |     pin_object_meta_read::<SqliteTenantStore>();
```

with `note: required by a bound in pin_object_meta_read`. The file was restored
from a saved copy, not `git checkout`, and re-checked clean.

The behavioral verification of the read plane is the other eight
`tests::object_meta::*` tests, which seed manifests and multipart uploads
through the `*_direct` helpers and assert concrete commit sequences, written
table names, prefix ordering and limits, bucket isolation, delete results, and
sqlite reopen persistence. A comment at the top of the test module points at the
pin, and the pin's comment points back at the tests.

---

## Verification

All commands run in `/Users/jack/src/github.com/nimbus/nimbus-fu6-seams`.
Rows marked ✱ were re-run at the review-follow-up commit; the rest are from
`4fc8b9ade` and are untouched by it.

| Command | Result |
| --- | --- |
| `cargo check -p nimbus-storage --all-targets` ✱ | clean |
| `cargo check -p nimbus-storage --all-targets --features libsql,mysql,postgres` ✱ | clean |
| `cargo check -p nimbus-storage --features libsql,test-hooks` | clean |
| `cargo check -p nimbus-fs -p nimbus-engine --all-targets` | clean |
| `cargo nextest run -p nimbus-fs` | **62 run, 62 passed** |
| `cargo nextest run -p nimbus-storage` (provider-free) ✱ | **296 run, 296 passed, 2 skipped** |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-storage --features libsql,mysql,postgres` ✱ | **438 run, 438 passed, 2 skipped** |
| `... -E 'test(object_meta)'`, both shapes ✱ | **8 run, 8 passed** each |
| `... -E 'test(u4_)'` (U4 gates) ✱ | **2 run, 2 passed** |
| `... -E 'test(sqlite_foundation)'` | **68 run, 68 passed** |
| `cargo clippy -p nimbus-storage -p nimbus-fs --all-targets -- -D warnings` ✱ | clean |
| `cargo clippy -p nimbus-storage --all-targets --features libsql,mysql,postgres -- -D warnings` ✱ | clean |
| `cargo fmt --all --check` ✱ | clean |

Both suite counts drop by one against the earlier run because the compile-only
test is gone, not because anything was skipped: 297 → 296 provider-free and
439 → 438 featured. The load-sensitive PITR budget test described below passed
in both of these runs.

### A load-sensitive budget test failed on the earlier runs

`tests::crud_and_journal::redb_storage_engine_quality_performance_budget_covers_latest_historical_cdc_pitr_and_gc`
failed on the `4fc8b9ade` runs of both suites, on the **SEQ13 PITR
export/import wall-clock budget** (measured `3.116790292s > 1s`) under
full-suite parallelism. In isolation it passed with `403ms`, well inside the
budget.

Confirmed pre-existing at the time by stashing the entire FU6 change set and
rerunning the provider-free suite on an unmodified `22c5cdd62` worktree: same
test, same assertion, same failure (296 passed / 1 failed / 2 skipped). It is a
load-sensitive timing budget, and that checkout was sharing the machine with
four sibling FU worktrees building concurrently. Nothing in FU6 touches the
redb PITR path. It passed in both review-follow-up runs, once the machine was
quiet — which is the same diagnosis from the other direction.

### Featured-run note

A first featured run without `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1`
reported 104 failures. Every one was
`provider_test_fixtures.rs:287` refusing to run without a live external
provider (`missing non-empty environment variable(s): NIMBUS_LIBSQL_URL, ...`)
— the postgres/mysql/libsql provider lanes, which need
`make test-external-provider`. Those lanes fail rather than skip by design.
With the documented opt-out set, the featured build's own 438 tests pass.
Live-provider evidence for those lanes remains hosted CI's.

### Machine note

The run hit `No space left on device` partway through (262 MiB free of 926 GiB).
Reclaimed by deleting `nimbus-network-architecture-audit/target` — 104 GiB of
regenerable build output in a worktree idle since 2026-07-29 18:37 with no live
processes. Freed 110 GiB; no source or git state touched.
