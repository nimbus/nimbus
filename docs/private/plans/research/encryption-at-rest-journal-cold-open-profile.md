# Encryption-at-Rest Journal Cold-Open Profile

This note records the focused cold-open attribution pass we ran after the
broader embedded encryption benchmark bundle. The goal was not to replace the
full benchmark reports, but to isolate where the encrypted reopen penalty lives
on the durable-journal path before making any more architecture or crypto
changes.

## Why This Exists

The broader embedded report already showed that the largest encryption-at-rest
penalties cluster around cold reopen rather than steady-state reads. The
durable-journal stream lane was one of the clearest examples:

- embedded SQLite stayed relatively close to its plaintext reopen cost
- embedded redb paid a much larger encrypted cold-open penalty

To decide what to optimize next, we narrowed the measurement plan to one warmup
and one measured sample, turned on per-phase logging, and added redb-specific
profiling for:

- manifest read and unwrap
- SQLCipher open phases
- tenant lazy-load timing
- encrypted redb open timing
- redb durable-journal stream timing
- encrypted redb page-read counts plus file-read and decrypt time during open

## Diagnostic Command Shape

```bash
NIMBUS_BENCH_STEADY_WARMUP_ROUNDS=1 \
NIMBUS_BENCH_STEADY_MEASURE_ROUNDS=1 \
NIMBUS_BENCH_COLD_WARMUP_ROUNDS=1 \
NIMBUS_BENCH_COLD_MEASURE_ROUNDS=1 \
NIMBUS_BENCH_COLD_OPEN_BREAKDOWN=1 \
NIMBUS_ENCRYPTION_PROFILE=1 \
NIMBUS_SQLITE_OPEN_PROFILE=1 \
NIMBUS_REDB_OPEN_PROFILE=1 \
NIMBUS_REDB_JOURNAL_PROFILE=1 \
NIMBUS_REDB_DEP_OPEN_PROFILE=1 \
NIMBUS_PROFILE_ONLY_COLD_SAMPLES=1 \
cargo bench -p nimbus-engine --bench embedded-provider-benchmarks \
  -- --workload journal-stream --local-encryption temp-master-key-file
```

Those drills intentionally trade statistical confidence for phase visibility.
They should be used to choose the next optimization target, not to replace the
longer checked-in benchmark bundle.

## Measured Cold Sample

The most useful comparison here is the encrypted redb measured cold sample
before and after the backend switched to fixed-size slot buffers with a single
physical read/write per page:

| redb phase | Before slot I/O cleanup | After slot I/O cleanup |
| --- | ---: | ---: |
| manifest unwrap | `185.88 us` | `185.88 us` |
| database open | `10.47 ms` | `10.61 ms` |
| first journal stream op | `1.97 ms` | `1.95 ms` |
| cold total | `13.04 ms` | `13.72 ms` |

On the same post-change drill, encrypted SQLite still landed around the same
order of magnitude as the earlier reduced-round runs:

- manifest unwrap: `141.33 us`
- pooled encrypted open: `560.00 us`
- first journal stream op: `1.89 ms`
- cold total: `1.98 ms`

The one-round redb total did not show a clear win from the slot-I/O cleanup by
itself. That does not mean the change was useless; it means this particular
micro-attribution drill is noisy enough that the direct latency win is smaller
than the total cold-open variance.

We also tested a follow-on positional-I/O variant that replaced the per-page
`seek` plus `read_exact`/`write_all` path with platform-specific
`read_at`/`seek_read` and `write_at`/`seek_write` helpers. On the same
one-round drill, that variant did not improve cold open and sometimes came out
slightly worse, so it was intentionally reverted. We kept the simpler slot I/O
path and retained only the improved profiling.

## redb Open Attribution

The new redb open profile added page-read accounting for the encrypted backend.
On the final measured cold sample for the retained code path, encrypted redb
open looked like this:

- manifest read plus unwrap: `192.58 us`
- encrypted backend construction: `34.42 us`
- `redb::Database` open: `10.93 ms`
- encrypted page-read calls during open: `5`
- logical bytes requested during open: `541001`
- encrypted physical page reads during open: `135`
- file-access time during open: `886.58 us`
- decrypt time during open: `3.91 ms`

The warmup cold sample landed in the same shape:

- encrypted physical page reads during open: `135`
- file-access time during open: `1.06 ms`
- decrypt time during open: `3.93 ms`
- `redb::Database` open: `11.30 ms`

### Dependency-Level redb Open Split

To move beyond the outer `redb::Database` timer, we patched the exact pinned
`redb 2.6.3` dependency locally through Cargo's `[patch.crates-io]` override
path and added env-gated phase timing inside `Database::new()` and
`TransactionalMemory::new()`. On the retained encrypted cold `journal-stream`
samples for Nimbus's current redb file format, the open split was:

- `TransactionalMemory::new()`: `4.74-6.38 ms`
- `InMemoryState::from_bytes()` total: `4.50-6.16 ms`
- `Allocators::from_bytes(...)`: `4.49-6.15 ms`
- `begin_writable()`: `5.13-5.90 ms`
- savepoint restore and tracker setup after open: effectively noise (`~0-0.20 ms`)

That turns the earlier "redb internal metadata/validation work" bucket into a
more concrete two-part story:

- reconstructing allocator state from the v2 on-disk metadata dominates the
  read-side portion of open
- `begin_writable()` immediately burns another `~5-6 ms` to flip the database
  into writable mode and flush that state

The savepoint-restore path was not the culprit.

### Deeper v2 Subphase Split

We then patched the same local `redb 2.6.3` fork one level deeper so the
retained v2 path emitted timing from inside `Allocators::from_bytes(...)` and
`begin_writable()` itself. On the same reduced-round encrypted cold
`journal-stream` drill, the measured split was:

- `Allocators::from_bytes.read_region_tracker`: about `0.19 ms`
- `Allocators::from_bytes.decode_region_tracker`: about `0.01 ms`
- `Allocators::from_bytes.read_region_headers`: about `4.57 ms`
- `Allocators::from_bytes.deserialize_region_headers`: about `0.12 ms`
- `Allocators::from_bytes.total`: about `4.90 ms`
- `begin_writable.write_header`: effectively noise (`~0.00-0.02 ms`)
- `begin_writable.flush_write_buffer.file_write`: about `0.37 ms`
- `begin_writable.flush.sync_data`: about `4.7-4.8 ms`
- `begin_writable.total`: about `5.1 ms`

That is the clearest current explanation for the retained encrypted redb cold
reopen penalty:

- the allocator side is dominated by region-header reads, not by in-memory
  decode or region-tracker parsing
- the writable-open side is dominated by the durability barrier, not by header
  mutation or write-buffer promotion

A focused `encrypted_redb` reopen test on a differently shaped local database
produced much larger absolute allocator times, but the same qualitative split:
region-header reads dominated allocator reconstruction and `sync_data`
dominated `begin_writable()`. That makes the conclusion more credible than a
single noisy benchmark sample by itself.

### Rejected v3 File-Format Experiment

Because `redb 2.6.3` exposes `Builder::create_with_file_format_v3(true)`, we
tested the obvious product-side experiment: create Nimbus redb databases in v3
format and rerun the same reduced-round encrypted cold `journal-stream` drill.
That experiment made cold reopen worse, so it was intentionally reverted.

Compared with the retained v2-created databases:

| redb reopen phase | v2-created DBs | v3-created DBs |
| --- | ---: | ---: |
| `redb::Database` open total | `10.02-12.53 ms` | `13.95-16.01 ms` |
| encrypted page reads during open | `135` | `262` |
| open-time decrypt work | `4.03-4.04 ms` | `7.63-7.90 ms` |

The inner split explains why:

- v3 nearly eliminated `TransactionalMemory::new()` itself (`0.24-0.29 ms`)
- but `get_allocator_state_table()` plus `load_allocator_state()` cost
  `8.50-10.14 ms`
- `begin_writable()` still cost another `5.13-5.50 ms`

So the v3 allocator-state table path more than erased the header-side savings
we hoped to reclaim from `Allocators::from_bytes(...)`. In the current Nimbus
encrypted reopen path, switching new redb files to v3 is not a cold-open
optimization.

## What This Means

Three conclusions are stable across the focused journal-stream drills:

- Manifest work is small. It is not the cold-open bottleneck for either
  backend.
- SQLite encryption overhead is modest and concentrated in SQLCipher open and
  key-verification phases, not in manifest unwrap.
- Encrypted redb cold-open is dominated by database open, not by the first
  journal stream itself.
- Within that redb open cost, the dominant current v2 phases are allocator
  reconstruction plus `begin_writable()`, not savepoint restoration.

Within that redb open cost, the new page-read stats show a more precise split:

- encrypted slot access, including the per-page `seek`, is about `0.9-1.1 ms`
- page decryption is about `3.9-4.0 ms`
- the remaining roughly `6.0-6.5 ms` is redb internal open, metadata traversal,
  cache warmup, and validation work above the raw encrypted slot access

We also tested the one obvious supported builder knob,
`redb::Builder::set_cache_size`, on the same reduced-round encrypted
journal-stream drill. It did not improve the retained encrypted cold-open path:

| redb cache setting | Encrypted redb cold journal median | Readout |
| --- | ---: | --- |
| default | `12.79 ms` | baseline |
| `64 MiB` | `13.41 ms` | effectively unchanged |
| `16 MiB` | `14.39 ms` | slightly worse |
| `1 MiB` | `17.78 ms` | worse, with one `149 ms` `redb::Database` open outlier |

That tells us the remaining redb reopen cost is not a simple cache-size policy
mistake inside repo-owned setup. The expensive work is still happening inside
`redb::Database` open and the metadata/validation it performs afterward.

We also rejected the next obvious repo-owned product lever,
`Builder::create_with_file_format_v3(true)`, because it increased encrypted
cold reopen cost instead of reducing it. That means the next useful
investigation should stay focused on the current v2 reopen path and the
`begin_writable()` behavior, not on flipping Nimbus to redb v3 by default.

There is also no obvious remaining Nimbus-side builder or startup knob hidden
behind the current retained redb seam. The local checks we ran after this drill
confirmed that the repo-owned open path is already down to `redb::Database`
construction itself, so the next meaningful cold-open optimization will likely
need one of these two moves:

- a redb-side or upstream-facing reduction in v2 allocator-header read cost
- an architectural change that avoids paying the immediate writable-open
  durability barrier on read-first opens

### Read-First Deferred-Writable Experiment

To test that second option directly, we patched the same temporary local
`redb 2.6.3` fork with a benchmark-only experiment that:

- skipped the eager `mem.begin_writable()` call during `Database::new()`
- skipped the immediate internal write transaction that restores persistent
  savepoint tracker state
- lazily called `begin_writable()` only when a later `begin_write()` happened

This was intentionally not a product patch. It is not semantically complete
enough for merge, because it changes when writable-mode and savepoint-tracker
work happen. The point was to measure the upside of a true read-first open.

On the same reduced-round encrypted cold `journal-stream` drill, the result was
material:

| redb reopen probe | Retained behavior | Deferred writable experiment |
| --- | ---: | ---: |
| cold journal median | `12.97 ms` | `7.96 ms` |
| cold first-operation sample | `12.88-13.94 ms` | `7.85-10.09 ms` |
| `redb::Database` open | `10.23-10.84 ms` | `4.76-5.65 ms` |
| open page reads | `135` | `133` |
| open decrypt work | `3.98-3.99 ms` | `3.88-3.90 ms` |

That is roughly a `~5 ms` win on the cold read-first path, or about `39%` off
the measured cold journal median in this drill.

The important nuance is that the cost is not magically gone. In the same
experiment logs, the deferred writable-state work showed up immediately after
the first read sample when later teardown or write-adjacent flows forced the
database into writable mode. So this experiment demonstrates a real
latency-shaping opportunity for read-first opens, not an end-to-end removal of
durability work.

That makes the next decision much clearer:

- if Nimbus cares most about first-read latency after reopen, a read-first open
  mode is promising
- if Nimbus needs the current eager writable guarantees on every open, the
  remaining win has to come from allocator reconstruction rather than from
  avoiding the sync barrier

The first cold `indexed-query` rerun looked scary, but it turned out to be a
poor basis for the roadmap because it only had one cold sample. That
single-sample pass showed redb moving from about `2.79 ms/op` to `7.20 ms/op`,
which initially looked like proof that the deferred-writable idea was
journal-only.

We then reran the same deferred-writable drill on `indexed-query` with three
cold samples per backend. That stronger pass did not reproduce the regression:

| redb reopen probe | Retained behavior | Deferred writable experiment |
| --- | ---: | ---: |
| cold indexed-query median | `3.00 ms/op` | `2.60 ms/op` |
| `redb::Database` open | `11.02-15.43 ms` | `5.09-5.25 ms` |
| tenant `open_existing` | `11.46-15.93 ms` | `5.43-5.69 ms` |
| first indexed-query total | `42.05-46.71 ms` | `31.77-32.14 ms` |
| first indexed-query execute | `27.89-29.70 ms` | `25.56-25.66 ms` |

That changes the interpretation in an important way. The journal-stream result
was not a fluke, and the deferred-writable experiment still looks promising on
the read-first redb cold paths we re-ran with enough samples to trust.

The caution is still real, just narrower. This experiment is not merge-ready
because it changes when writable-mode and savepoint-restore work happen, and
the deferred cost still reappears later when a write or teardown forces the
database into writable mode. So the useful follow-up is not "skip eager
writable mode everywhere." It is to explore an explicit read-first or read-only
open contract, with clearly defined warmup and durability semantics, as the
most promising remaining redb cold-open lever.

### Supported API Boundary Check

We also rechecked the live Nimbus storage seam and the exact pinned
`redb 2.6.3` source to answer the next obvious implementation question: can we
prototype that read-first behavior in-repo without carrying a dependency fork?

Today, the answer is no.

- Nimbus's repo-owned redb seam is already thin:
  `TenantStore::open_encrypted_with_simulation(...)` constructs the encrypted
  backend and immediately calls `redb::Database::builder().create_with_backend(...)`.
- In the pinned `redb 2.6.3` source, `Database::new()` still eagerly calls
  `mem.begin_writable()?` before returning the `Database`, then opens an
  immediate internal write transaction to restore persistent savepoint tracker
  state.
- The supported builder surface exposes knobs like cache size and file-format
  selection, but it does not expose a read-only or deferred-writable open mode.

That closes an important branch in the roadmap. There is no hidden repo-only
page-cache, file-read, or builder-level knob left that can deliver the
read-first win we measured in the temporary dependency patch. If Nimbus wants
that improvement in the real product path, it now needs one of two deliberate
moves:

- carry a local `redb` patch long enough to prove a real read-first contract
- upstream a supported `redb` open mode and then adapt the engine/storage seam
  around that contract

So this investigation is complete in one useful sense: the remaining redb
cold-open opportunity is real, but it is no longer a repo-only tuning problem.
For the current roadmap, Nimbus is intentionally not pursuing a local `redb`
patch or upstream read-first-open contract. The practical next step is to
leave this as a documented future lever and shift execution effort to the
remaining libsql benchmark evidence and hosted SQLCipher proof work.

## Journal Bootstrap Follow-Up

We then ran the cold `journal-bootstrap` workload with the same profile hooks
enabled, plus new tenant runtime subphase timing. That drill separates three
different pieces of the first operation:

- encrypted redb reopen
- tenant runtime initialization after reopen
- bootstrap export from the reopened tenant

That bootstrap export is not the ordinary tenant reopen path. In the checked-in
service graph it is used by replica/bootstrap surfaces, including
[EmbeddedReplica bootstrap](/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-engine/src/replica.rs:37),
[consistency/shadow materializer verification](/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-engine/src/service/queries/verification.rs:17),
and the HTTP [journal bootstrap endpoint](/Users/jack/src/github.com/nimbus/nimbus/crates/nimbus-server/src/http/queries.rs:76).
That matters for prioritization: this benchmark is measuring a real surface, but
not the common lazy-load query reopen path.

On the retained encrypted redb cold samples, the shape was stable:

- manifest read plus unwrap: `0.20-0.28 ms`
- encrypted redb open: `11.42-11.55 ms`
- tenant runtime init: `0.37-0.38 ms`
- bootstrap export total: `3.04-3.25 ms`
- overall first operation: `15.50-15.76 ms`

The new tenant runtime breakdown is especially helpful because it lets us rule
out another tempting suspect. Runtime init was small:

- schema load during runtime init: `0.25-0.27 ms`
- journal progress during runtime init: `0.09-0.10 ms`

So the post-open repo-owned cost is not runtime state wiring. It is the
bootstrap export itself. Within that bootstrap export:

- `journal_progress()`: about `0.01 ms`
- `load_schema()`: about `0.01 ms`
- `documents()`: about `3.70-3.77 ms`
- scheduled execution id scan: effectively `0 ms`

After fixing the document-export split to time iterator `next()` correctly, the
cold `documents()` path is mostly traversal/value fetch rather than decode:

- cold iterator `next()` plus value fetch: `3.15-3.19 ms`
- cold `Document::from_msgpack(...)`: `0.35-0.37 ms`

For contrast, the warm steady-state path is still decode-led:

- warm iterator `next()` plus value fetch: `0.05-0.06 ms`
- warm `Document::from_msgpack(...)`: `0.38-0.39 ms`

That means the bootstrap workload has a secondary repo-owned cost center after
reopen, but it is not primarily MessagePack decode on the cold path. It is
pulling the full document snapshot back through redb immediately after reopen.

That is the important architectural answer. The next optimization target should
not be crypto policy, manifest format, or SQLCipher tuning. It should be the
redb reopen path itself, especially:

- redb open-time metadata traversal and validation
- cold bootstrap document traversal/export cost after open, when the workload
  is snapshot-heavy

The second bullet is a replica/bootstrap concern. The first bullet still owns
the broader cold-reopen experience for embedded service workloads.

## Recommended Next Step

Treat the encrypted redb cold-open path as two separate problems:

1. `redb::Database` open still owns most of the cost.
2. Cold bootstrap document traversal/export is secondary but still meaningful on
   snapshot-heavy workloads.

The safest next investigation is to keep profiling at the redb seam:

- profile `redb::Database` open-time metadata traversal and validation more
  directly instead of trying more builder-level cache tuning or a file-format
  switch
- if bootstrap workloads become the next bottleneck, target how much document
  data the cold bootstrap needs to pull after reopen before considering
  MessagePack-specific micro-optimizations
- confirm any future change with a full-sample benchmark refresh before
  treating it as a landed performance improvement
