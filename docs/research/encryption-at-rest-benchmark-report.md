# Encryption-at-Rest Benchmark Report

Generated from the repo-owned benchmark harness and evidence collector:

```bash
make collect-encryption-benchmark-evidence \
  OUTPUT_DIR=/tmp/neovex-encryption-benchmark-evidence
```

This report summarizes the representative local embedded-provider results from
that capture bundle plus the focused replica-connected SQLite local-cache
reopen and freshness drills. The bundle shape is owned by
`scripts/collect-encryption-benchmark-evidence.sh`, which records:

- `system-info.log`
- `embedded-plaintext-report.md`
- `embedded-encrypted-report.md`
- per-command logs
- `libsql-replica-encrypted-cache-report.md` when local libsql benchmark
  endpoints are configured; that report is intentionally scoped to the
  plan-owned local-cache reopen and freshness drills rather than the broader
  write-heavy embedded contrast lanes

Checked-in artifacts from the 2026-04-21 local evidence refresh:

- [Summary report](./encryption-at-rest-benchmark-report.md)
- [Embedded plaintext raw report](./encryption-at-rest-embedded-plaintext-benchmark-report.md)
- [Embedded encrypted raw report](./encryption-at-rest-embedded-encrypted-benchmark-report.md)
- [Replica-connected SQLite local-cache raw report](./encryption-at-rest-libsql-replica-encrypted-cache-benchmark-report.md)
- [Indexed-query post-optimization refresh](./encryption-at-rest-indexed-query-refresh.md)
- [Journal cold-open attribution follow-up](./encryption-at-rest-journal-cold-open-profile.md)

## Host Context

Captured from the `system-info.log` artifact in the 2026-04-21 evidence
bundle:

- captured at `2026-04-21T03:10:32Z`
- repo commit: `a3aa0135dbc3111868a760273d60dce3d73482a7`
- repo branch: `main`
- OS: macOS `15.7.2` (`24G325`)
- kernel: Darwin `24.6.0` on `arm64`
- Rust toolchain: `rustc 1.93.1 (01f6ddf75 2026-02-11)`
- Cargo: `1.93.1 (083ac5135 2025-12-15)`

The sandboxed capture environment did not allow the collector to read CPU brand
string or physical memory through `sysctl`, so those fields remain absent from
this bundle. That is a limitation of this local capture environment, not an
intentional omission from the benchmark flow.

## Methodology

- plaintext and encrypted runs both used the checked-in
  `embedded-provider-benchmarks` harness
- encrypted runs used `ENCRYPTION=temp-master-key-file`, which exercises the
  real manifest-backed startup path with a benchmark-only stable master key
  file per benchmark process
- steady-state lanes used `2` warmup rounds and `12` measured rounds
- cold-start lanes used `1` warmup round and `10` measured rounds
- the benchmark harness alternated backend order every round
- cold-start lanes cloned seeded datasets before each sample, then measured the
  fresh open plus first representative execution
- the numbers below use median per-operation latency taken from the generated
  markdown reports

## Embedded SQLite

| Workload | Plaintext steady | Encrypted steady | Plaintext cold | Encrypted cold |
| --- | ---: | ---: | ---: | ---: |
| CRUD throughput | `388.23 us` | `612.81 us` | `431.86 us` | `671.58 us` |
| Point read latency | `779.00 ns` | `785.00 ns` | `79.44 us` | `90.32 us` |
| Indexed query latency | `1.11 ms` | `1.14 ms` | `1.91 ms` | `4.82 ms` |
| Composite indexed query latency | `619.47 us` | `636.71 us` | `1.15 ms` | `1.34 ms` |
| Durable journal stream latency | `636.90 us` | `670.08 us` | `7.68 ms` | `13.02 ms` |
| Durable journal bootstrap latency | `694.06 us` | `686.29 us` | `10.52 ms` | `15.12 ms` |
| Subscription fan-out latency | `68.70 us` | `66.41 us` | `1.04 ms` | `2.79 ms` |
| Mixed multi-tenant load | `196.56 us` | `221.44 us` | `225.61 us` | `241.50 us` |

Key takeaways:

- SQLite remains decisively ahead of encrypted redb on the write-heavy and
  mixed service-path workloads even after encryption is enabled.
- Encryption overhead is modest on warm point reads and indexed queries, but
  cold-start indexed-query and journal lanes pay a visibly larger reopen cost.
- The measured CRUD delta is meaningful but still leaves encrypted SQLite far
  ahead of encrypted redb for the representative service-path mix gathered here.

## Embedded redb

| Workload | Plaintext steady | Encrypted steady | Plaintext cold | Encrypted cold |
| --- | ---: | ---: | ---: | ---: |
| CRUD throughput | `8.99 ms` | `10.23 ms` | `8.94 ms` | `10.17 ms` |
| Point read latency | `786.00 ns` | `778.00 ns` | `116.22 us` | `161.27 us` |
| Indexed query latency | `1.25 ms` | `1.24 ms` | `2.47 ms` | `7.44 ms` |
| Composite indexed query latency | `682.83 us` | `689.38 us` | `1.56 ms` | `2.20 ms` |
| Durable journal stream latency | `658.94 us` | `651.17 us` | `12.79 ms` | `23.45 ms` |
| Durable journal bootstrap latency | `543.42 us` | `542.48 us` | `15.12 ms` | `26.09 ms` |
| Subscription fan-out latency | `385.58 us` | `424.96 us` | `2.08 ms` | `5.15 ms` |
| Mixed multi-tenant load | `2.98 ms` | `4.14 ms` | `2.96 ms` | `4.40 ms` |

Key takeaways:

- Encrypted redb keeps roughly the same warm indexed-read shape as plaintext,
  but the cold-start indexed-query and journal lanes grow materially.
- Warm CRUD and mixed-load deltas are noticeable but smaller than the
  corresponding cold-start penalties.
- Encrypted redb still retains the steady-state durable-journal-bootstrap edge
  over encrypted SQLite in this local run.

## Overall Readout

- The local encryption implementation preserves the broad embedded-provider
  ranking already seen in plaintext mode: SQLite remains the stronger default
  for the representative service-path mix, while redb keeps a narrower
  advantage on some journal-centric steady-state lanes.
- The largest encryption penalties in this run cluster around cold-start reopen
  paths rather than warm steady-state reads.
- No benchmark-specific crypto shortcut was used: the encrypted numbers came
  from the same manifest-backed startup and reopen flow used by runtime opens.

## Cold-Open Optimization Follow-Up

After the full evidence capture above, we ran reduced-round diagnostic drills
to isolate the cold indexed-query reopen path while landing two low-risk
optimizations:

- SQLCipher key verification now reads a single schema row instead of scanning
  the full catalog on every encrypted open.
- The embedded redb control-plane usage store now opens lazily on first use,
  so query-only cold reopens do not pay control-plane startup cost.
- Tenant lazy-load now skips redundant durable-journal recovery when the
  runtime's initial journal progress already reports `applied_head >= durable_head`.
- The encrypted redb backend now encrypts and decrypts pages in place with
  fixed-size buffers and single-slot physical I/O, removing per-page heap
  allocation and reducing syscall fan-out on the hot file-open and
  metadata-read path without changing nonce or AAD handling.

Representative diagnostic command shape:

```bash
NEOVEX_BENCH_COLD_OPEN_BREAKDOWN=1 \
NEOVEX_TENANT_LOAD_PROFILE=1 \
NEOVEX_QUERY_PROFILE=1 \
cargo bench -p neovex-engine --bench embedded-provider-benchmarks \
  -- --workload indexed-query --local-encryption <disabled|temp-master-key-file>
```

These first follow-up runs used `1` warmup and `1` measured round per lane.
They remain useful for phase attribution, but they no longer stand alone: the
repo now also carries a full-sample `indexed-query` refresh in
[encryption-at-rest-indexed-query-refresh.md](./encryption-at-rest-indexed-query-refresh.md).

### Indexed Query Cold Reopen

SQLite on the cold indexed-query lane is now much closer between plaintext and
encrypted mode:

| Mode | SQLite cold total | SQLite service bootstrap | SQLite first operation |
| --- | ---: | ---: | ---: |
| Plaintext | `38.01-38.64 ms` | `60-76 us` | `37.95-38.57 ms` |
| Encrypted | `38.76-39.30 ms` | `91-98 us` | `38.67-39.20 ms` |

That leaves SQLite with only about `0.69-1.29 ms` of residual cold indexed-query
overhead in these focused runs, roughly `1.8%-3.4%` over plaintext.

Before the bootstrap and SQLCipher-open optimizations, the same reduced-round
encrypted SQLite drill was roughly `50.06-51.11 ms`, so the cold indexed-query
reopen path improved by about `11-12 ms` overall.

### Phase Breakdown

The first cold SQLite indexed query now breaks down roughly as:

- plaintext tenant load: `1.10-1.36 ms`
- encrypted tenant load: `1.48-1.56 ms`
- plaintext first indexed execution: `8.41-8.68 ms`
- encrypted first indexed execution: `8.74-9.12 ms`
- encrypted SQLite pooled open inside tenant load: `0.87-0.98 ms`
- encrypted manifest unwrap inside tenant load: `0.21-0.22 ms`
- redundant journal recovery on clean reopens: `0 ns` after the recovery-skip change

The remaining SQLite encryption cost is therefore a small blend of:

- tenant open work, primarily manifest unwrap plus SQLCipher setup
- a modest first-query execution delta on the first indexed read

redb still shows a noticeably larger cold reopen penalty on the same lane:

- plaintext redb cold total: `56.25-56.87 ms`
- encrypted redb cold total: `67.62-69.83 ms`
- plaintext redb tenant load: `6.90-7.26 ms`
- encrypted redb tenant load: `11.74-13.02 ms`

A subsequent reduced-round rerun after the in-place redb buffer change reported
`2.69 ms/op` on the encrypted redb cold indexed-query lane, or about `64.6 ms`
for the benchmark's `24` queries. Against the earlier `67.62-69.83 ms`
diagnostic totals above, that suggests roughly `3-5 ms` of reclaimed cold
reopen time. The next full evidence capture should confirm that delta under the
same longer measurement plan used for the checked-in baseline tables.

The practical takeaway is that the cold-open problem is no longer a
service-bootstrap problem for SQLite. The remaining gap is first-operation
bound and small enough that further tuning should be justified by product
requirements or a fresh full benchmark capture, rather than assumed.

The follow-up note now closes out the most obvious SQLite warmup branch as
well. Benchmark-only service-query warmups (`limit1` and `full`) slightly
regressed end-to-end cold latency, and the next lower-level raw probe
(`NEOVEX_SQLITE_INDEX_QUERY_WARMUP=raw-id-only`) regressed it sharply: on the
reduced-round rerun, SQLite cold median moved from `1.63 ms/op` to
`4.79 ms/op`, and two measured cold samples saw the first service query spike
into the `69-79 ms` range. That means the next cold-open step should not be
"pay the indexed query early" in another form. If SQLite gets another reopen
pass, it should be a fundamentally different in-process change inside the real
open path; otherwise the better next investment is redb read-first/open work
and the remaining libsql plus hosted proof evidence.

### Journal Stream Attribution

We also ran a focused post-change journal-stream cold-open attribution pass and
checked it into
[encryption-at-rest-journal-cold-open-profile.md](./encryption-at-rest-journal-cold-open-profile.md).
That follow-up does not replace the broader benchmark bundle; it exists to show
where encrypted reopen time is actually spent.

The important result from that drill is that manifest work is small, SQLite
remains relatively modest on the cold journal path, and encrypted redb reopen
is still dominated by `redb::Database` open rather than raw file I/O. On the
final retained-code cold sample, encrypted redb open read `135` encrypted
pages, spent about `0.9-1.1 ms` in encrypted slot access including per-page
`seek`, about `3.9-4.0 ms` in page decryption, and still spent another roughly
`6.0-6.5 ms` above that inside redb open-time metadata and validation work. We
also tested a positional-I/O variant and rejected it because it did not improve
the cold-open sample. That means the next cold-open optimization target should
be the redb reopen path itself, not the manifest contract or SQLCipher policy.

We then tested the one supported repo-owned `redb` builder knob,
`redb::Builder::set_cache_size`, on the same reduced-round encrypted
journal-stream drill. That sweep also failed to move the reopen bottleneck in a
useful direction:

| redb cache setting | Encrypted redb cold journal median | Result |
| --- | ---: | --- |
| default | `12.79 ms` | baseline |
| `64 MiB` | `13.41 ms` | effectively flat |
| `16 MiB` | `14.39 ms` | worse |
| `1 MiB` | `17.78 ms` | worse, plus a `149 ms` `redb::Database` open outlier |

That result is strong enough to reject cache-size tuning as the next product
lever. We removed the temporary benchmark-only override instead of carrying a
knob that does not address the real cold-open cost center.

We then patched the exact pinned `redb 2.6.3` dependency locally and added
phase timing inside `Database::new()` and `TransactionalMemory::new()`. That
dependency-level split turned the remaining v2 reopen cost into something more
concrete:

- `Allocators::from_bytes(...)` alone costs about `4.5-6.2 ms`
- `begin_writable()` costs another `5.1-5.9 ms`
- savepoint restoration after open is negligible

That means the current retained encrypted reopen path is not mostly paying for
manifest unwrap, repo-owned redb read helpers, or savepoint bookkeeping. It is
paying for allocator reconstruction plus the immediate writable-state flip
inside `redb::Database` open.

We then instrumented the retained v2 path one level deeper. On the same
reduced-round encrypted cold `journal-stream` drill:

- `Allocators::from_bytes(...)` was dominated by region-header reads
  (`~4.57 ms`) rather than tracker parsing (`~0.19 ms`) or deserialization
  (`~0.12 ms`)
- `begin_writable()` was dominated by the durability barrier
  (`sync_data ~= 4.7-4.8 ms`) rather than header mutation
  (`~0.00-0.02 ms`) or buffered file write (`~0.37 ms`)

That is the strongest current explanation for the remaining encrypted redb
cold-open cost. The bottleneck is no longer our manifest flow or the retained
encrypted slot I/O path; it is redb's allocator-header reconstruction plus the
immediate writable-open sync step.

We also tested the obvious product-side format experiment,
`Builder::create_with_file_format_v3(true)`, on the real Neovex redb store
surfaces. That change was intentionally reverted because it made encrypted cold
reopen worse rather than better:

| redb reopen probe | v2-created DBs | v3-created DBs |
| --- | ---: | ---: |
| `redb::Database` open total | `10.02-12.53 ms` | `13.95-16.01 ms` |
| encrypted page reads during open | `135` | `262` |
| open-time decrypt work | `4.03-4.04 ms` | `7.63-7.90 ms` |

That matters for prioritization. We have now checked the obvious repo-owned
levers on this path:

- manifest unwrap and SQLCipher tuning were never the redb bottleneck
- encrypted slot I/O cleanup helped the backend but did not eliminate the main
  reopen cost center
- `redb::Builder::set_cache_size(...)` did not improve the retained cold path
- `Builder::create_with_file_format_v3(true)` made the real Neovex reopen path
  worse

The next cold-open optimization should therefore be framed as either upstream
redb work on allocator reconstruction, or an architectural change that avoids
paying the immediate writable-open durability barrier on read-first opens.

We then tested that architectural hypothesis directly in the same temporary
local `redb 2.6.3` fork by deferring `begin_writable()` until the first real
write transaction instead of paying it eagerly during `Database::new()`. That
benchmark-only experiment is not merge-ready, but it is very informative:

- encrypted redb cold `journal-stream` median dropped from `12.97 ms` to
  `7.96 ms`
- `redb::Database` open dropped from about `10.23-10.84 ms` to
  `4.76-5.65 ms`
- open-time decrypt work stayed almost unchanged at about `3.88-3.99 ms`

That means the eager writable-open path is a real contributor to first-read
latency, not just a theoretical suspicion. The follow-up logs also showed the
deferred work reappearing later when teardown or a later write forced the
database into writable mode, so this is best understood as a latency-shaping
opportunity for read-first opens rather than a free elimination of durability
cost.

The first cold `indexed-query` rerun looked worse, but it only had one cold
sample and turned out to be too noisy to anchor the roadmap by itself. We
reran that lane with three cold samples per backend, and the stronger pass did
not reproduce the regression:

- cold indexed-query median moved from `3.00 ms/op` to `2.60 ms/op`
- `redb::Database` open dropped from about `11.02-15.43 ms` to about
  `5.09-5.25 ms`
- tenant `open_existing` dropped from about `11.46-15.93 ms` to about
  `5.43-5.69 ms`
- first indexed query total dropped from about `42.05-46.71 ms` to about
  `31.77-32.14 ms`

So the stronger evidence now points in one direction: the eager writable-open
path is a real contributor to redb cold-read latency, and a read-first open
mode remains a promising optimization lever.

The caution is that this is still a latency-shaping experiment, not a
production-ready fix. The deferred writable/savepoint work still shows up later
when a write or teardown forces the database into writable mode, so the next
follow-up should be an explicit read-first or read-only open contract with
clear warmup and durability semantics, not a blind "skip eager writable mode"
patch.

We also rechecked the live Neovex seam and the pinned `redb 2.6.3` API after
that experiment. The repo-owned open path is already down to
`TenantStore::open_* -> redb::Database::builder().create(...)`, and upstream
`Database::new()` still eagerly calls `mem.begin_writable()` and then restores
persistent savepoint state through an immediate write transaction. In other
words, there is no supported repo-only builder or page-cache knob left that
delivers this win. The next serious step is either an upstream/local `redb`
open-mode change or a deliberate decision to stop here and shift effort to the
remaining libsql and release-proof evidence.

For the current roadmap, Neovex is choosing the second path. We are not
pursuing a deliberate redb read-first/open contract via local patch or upstream
work right now, so this remains a documented future lever rather than active
implementation scope.

Under v3, `TransactionalMemory::new()` itself became tiny, but
`get_allocator_state_table()` plus `load_allocator_state()` consumed
`8.5-10.1 ms`, which more than erased the v2 header-load savings. So the next
redb cold-open optimization target should stay on the current v2 reopen path
and `begin_writable()` behavior, not on flipping Neovex to redb v3 by default.

### Journal Bootstrap Attribution

We also ran a focused cold `journal-bootstrap` drill with the same manifest,
SQLCipher, tenant-load, and redb profile hooks enabled. That pass separated
tenant reopen from the bootstrap export itself.

This workload is product-real, but it is not the common tenant reopen path. In
the checked-in service graph, `export_durable_journal_bootstrap_async()` is used
for replica/bootstrap surfaces such as [EmbeddedReplica bootstrap](/Users/jack/src/github.com/agentstation/neovex/crates/neovex-engine/src/replica.rs:37),
[shadow materializer verification](/Users/jack/src/github.com/agentstation/neovex/crates/neovex-engine/src/service/queries/verification.rs:17),
and the HTTP [journal bootstrap endpoint](/Users/jack/src/github.com/agentstation/neovex/crates/neovex-server/src/http/queries.rs:76).
Ordinary tenant lazy-load and query reopen do not export the full bootstrap
snapshot.

On the encrypted redb cold sample:

- manifest read plus unwrap: `0.20-0.28 ms`
- encrypted redb open: `11.42-11.55 ms`
- tenant runtime init: `0.37-0.38 ms`
- bootstrap export total: `3.04-3.25 ms`
- overall first operation: `15.50-15.76 ms`

The new tenant runtime subphase logging shows that runtime init is not the
problem:

- schema load during runtime init: `0.25-0.27 ms`
- journal progress during runtime init: `0.09-0.10 ms`

The bootstrap export profile then shows where the repo-owned first-operation
cost actually lands:

- journal progress inside export: `0.01 ms`
- schema reload inside export: `0.01 ms`
- document export: `3.75-3.83 ms`
- scheduled execution scan: effectively `0 ms`

After correcting the document-export split to measure iterator `next()` work
outside the loop body, that document-export cost turned out to be dominated by
redb traversal on the cold path rather than by MessagePack decode:

- cold `documents()` total: `3.70-3.77 ms`
- cold iterator `next()` plus value fetch: `3.15-3.19 ms`
- cold `Document::from_msgpack(...)`: `0.35-0.37 ms`

For comparison, the warm steady-state bootstrap export stayed decode-heavy:

- warm `documents()` total: `0.50-0.51 ms`
- warm iterator `next()` plus value fetch: `0.05-0.06 ms`
- warm `Document::from_msgpack(...)`: `0.38-0.39 ms`

That means the redb cold bootstrap path is now a clear two-part story:

1. `redb::Database` open still dominates the reopen penalty.
2. Once reopen finishes, the only meaningful repo-owned first-operation cost is
   cold document snapshot traversal and export for bootstrap-heavy workloads.

The prioritization implication is straightforward: if we are optimizing the
common embedded service reopen path, redb open-time metadata/validation remains
the main target. The bootstrap export work matters for replica/bootstrap APIs,
but it should not displace the primary reopen investigation unless those
surfaces become product-critical.

### Full-Sample Indexed-Query Refresh

After the targeted cold-open fixes landed, we reran the embedded benchmark
harness with the workload narrowed to `indexed-query` but otherwise keeping the
standard sample plan (`2` steady warmups, `12` steady measured rounds, `1`
cold-start warmup, `10` cold-start measured rounds). Those refreshed numbers
should be treated as the best current evidence for the tuned reopen path:

| Backend | Mode | Steady-State median per op | Cold-Start median per op |
| --- | --- | ---: | ---: |
| SQLite | Plaintext | `1.11 ms` | `1.52 ms` |
| SQLite | Encrypted | `1.13 ms` | `1.56 ms` |
| redb | Plaintext | `1.22 ms` | `2.25 ms` |
| redb | Encrypted | `1.23 ms` | `2.60 ms` |

That leaves current indexed-query encryption overhead at roughly:

- SQLite steady-state: about `+1.8%`
- SQLite cold-start: about `+2.6%`
- redb steady-state: about `+0.8%`
- redb cold-start: about `+15.6%`

Compared with the earlier broad embedded evidence pack at the top of this
document, the refreshed indexed-query cold path is materially better:

- encrypted SQLite cold indexed-query dropped from `4.82 ms` to `1.56 ms`
- encrypted redb cold indexed-query dropped from `7.44 ms` to `2.60 ms`

This is the most accurate current answer to "what does encryption at rest cost
on the cold indexed-query reopen path?" in the checked-in repo evidence.

## Replica-Connected SQLite Local Cache

The focused local-cache reopen and freshness report is now checked in at
[Replica-connected SQLite local-cache raw report](./encryption-at-rest-libsql-replica-encrypted-cache-benchmark-report.md).
That report intentionally measures the plan-owned replica drills rather than
the full write-heavy embedded contrast suite.

Representative medians from the refreshed encrypted local-cache run:

| Drill | Embedded sqlite / baseline | Libsql replica with encrypted local cache |
| --- | ---: | ---: |
| Point read steady-state | `989.00 ns/op` | `1.01 us/op` |
| Point read cold-start | `23.59 us/op` | `681.54 us/op` |
| Indexed query steady-state | `304.94 us/op` | `302.52 us/op` |
| Indexed query cold-start | `409.72 us/op` | `8.59 ms/op` |
| Composite indexed query steady-state | `194.52 us/op` | `183.02 us/op` |
| Composite indexed query cold-start | `294.09 us/op` | `8.19 ms/op` |
| Same-service barrier refresh | n/a | `2.91 ms` |
| Peer catch-up visibility | n/a | `539.39 ms` |

Key takeaways:

- Warm local-cache reads are close to embedded SQLite once the cache is hot:
  the steady-state point-read and indexed-query lanes stay within a few percent
  of embedded SQLite, and the composite indexed-query lane slightly favors the
  replica-backed path on this local fixture.
- Cold local-cache reopen remains materially slower than embedded SQLite
  because these drills include replica-provider reopen and cache-refresh work,
  not just local-at-rest crypto. Read the replica cold-start numbers as
  representative provider-path latency, not as a pure encryption-overhead
  delta.
- The shipped freshness contract looks operationally reasonable on the local
  single-node fixture: same-service barrier refresh stays in the low single
  digit milliseconds, while peer catch-up through the poll worker lands around
  half a second.

## Related Open Work

- The hosted SQLCipher proof artifacts from the release and Linux-package
  workflows remain a separate EAR4 closeout input, not an EAR9 benchmark gap.
