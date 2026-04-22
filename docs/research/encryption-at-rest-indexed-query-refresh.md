# Encryption-at-Rest Indexed Query Refresh

This report records the post-optimization embedded `indexed-query` benchmark
rerun for encryption at rest. It exists alongside the broader embedded evidence
pack in [encryption-at-rest-benchmark-report.md](./encryption-at-rest-benchmark-report.md)
so the cold-open-sensitive workload we explicitly tuned has a repo-owned,
high-signal refresh.

## Why This Refresh Exists

After the first embedded evidence pack was captured, we landed several
cold-open-focused improvements:

- SQLCipher key verification now probes a single schema row instead of
  scanning the full catalog on encrypted open.
- The embedded redb control-plane usage store now opens lazily on first use.
- Tenant lazy-load now skips redundant durable-journal recovery when runtime
  journal progress is already clean.
- The encrypted redb backend now encrypts and decrypts pages in place with
  fixed-size buffers, removing per-page heap allocation from the hot file-open
  and metadata-read path.

The original repo summary already captured the directional diagnostic story.
This refresh reruns the same benchmark harness with the workload narrowed to the
single-field indexed query that best exposes reopen cost, using the harness's
standard sample plan rather than the one-round diagnostic drill.

## Commands

Plaintext:

```bash
make bench-embedded-providers \
  WORKLOAD=indexed-query \
  REPORT=/tmp/neovex-indexed-query-refresh-plaintext.md
```

Encrypted:

```bash
make bench-embedded-providers \
  WORKLOAD=indexed-query \
  ENCRYPTION=temp-master-key-file \
  REPORT=/tmp/neovex-indexed-query-refresh-encrypted.md
```

## Methodology

- benchmark harness: `embedded-provider-benchmarks`
- workload filter: `indexed-query`
- steady-state warmup rounds: `2`
- steady-state measured rounds: `12`
- cold-start warmup rounds: `1`
- cold-start measured rounds: `10`
- encrypted runs use the manifest-backed local-encryption startup path through a
  benchmark-only stable master key file
- cold-start runs clone seeded datasets before each measured reopen so the
  first-query path is isolated from seed writes

## Results

### SQLite

| Mode | Steady-State median per op | Cold-Start median per op |
| --- | ---: | ---: |
| Plaintext | `1.11 ms` | `1.52 ms` |
| Encrypted | `1.13 ms` | `1.56 ms` |

Resulting SQLite encryption overhead on this workload:

- steady-state: about `+0.02 ms` per op, roughly `+1.8%`
- cold-start: about `+0.04 ms` per op, roughly `+2.6%`

### redb

| Mode | Steady-State median per op | Cold-Start median per op |
| --- | ---: | ---: |
| Plaintext | `1.22 ms` | `2.25 ms` |
| Encrypted | `1.23 ms` | `2.60 ms` |

Resulting redb encryption overhead on this workload:

- steady-state: about `+0.01 ms` per op, roughly `+0.8%`
- cold-start: about `+0.35 ms` per op, roughly `+15.6%`

### Backend Comparison

| Lane | Plaintext SQLite vs redb | Encrypted SQLite vs redb |
| --- | ---: | ---: |
| Steady-State | `1.10x` faster | `1.09x` faster |
| Cold-Start | `1.48x` faster | `1.66x` faster |

## Comparison With The Earlier Embedded Bundle

These refreshed indexed-query numbers should be preferred when discussing the
optimized reopen path. Compared with the earlier checked-in embedded bundle:

- encrypted SQLite cold indexed-query improved from `4.82 ms` to `1.56 ms`
- encrypted redb cold indexed-query improved from `7.44 ms` to `2.60 ms`

That is a large enough shift that the earlier bundle is still useful for the
broader pre-optimization baseline, but not as the best representation of the
current indexed-query cold-open path.

## SQLite Cold-Open Attribution Follow-Up

We also reran the encrypted `indexed-query` workload with the existing repo
profile hooks enabled so the current SQLite cold path is attributed in the
same repo-owned evidence pack rather than inferred from older one-round drills.

Command:

```bash
NEOVEX_ENCRYPTION_PROFILE=1 \
NEOVEX_SQLITE_OPEN_PROFILE=1 \
NEOVEX_TENANT_LOAD_PROFILE=1 \
NEOVEX_QUERY_PROFILE=1 \
NEOVEX_PROFILE_ONLY_COLD_SAMPLES=1 \
NEOVEX_BENCH_COLD_MEASURE_ROUNDS=3 \
cargo bench -p neovex-engine --bench embedded-provider-benchmarks \
  -- --workload indexed-query --local-encryption temp-master-key-file
```

Across the cold SQLite samples in that profiled rerun, the reopen breakdown was
small and stable:

- manifest read plus unwrap: usually `0.21-0.22 ms`, with one `1.31 ms`
  filesystem-read outlier
- SQLite `Connection::open(...)`: about `0.24-0.31 ms`
- SQLCipher `apply_key`: about `0.04 ms`
- temp hardening PRAGMAs: about `0.00-0.01 ms`
- SQLCipher `verify_key`: about `0.27-0.62 ms`
- SQLite connection initialization: about `0.03 ms`
- pooled open plus schema load: about `0.85-1.36 ms`
- tenant `open_existing`: about `1.27-2.28 ms`, or `2.60 ms` on the manifest
  read outlier

The bigger remaining cost is after the reopen seam, not inside it:

- first cold indexed query total: about `13.84-15.92 ms`
- first cold indexed query execute phase alone: about `10.99-13.32 ms`
- later warm indexed queries in the same sample: roughly `0.90-1.08 ms`

That means the tuned SQLite cold indexed-query path is no longer primarily a
crypto-open problem. Manifest unwrap is small, `apply_key` is tiny, and even
the largest crypto-specific step (`verify_key`) stays well under a millisecond.
The next worthwhile SQLite cold-open optimization should therefore target the
first query after reopen, such as page-cache warmup or other indexed-read
startup effects, rather than weakening or retuning the encryption contract.

## SQLite Query Warmup Experiment

We then tested the most direct warmup hypothesis with a benchmark-only hook in
the embedded indexed-query cold lane. The hook is gated by
`NEOVEX_SQLITE_INDEX_QUERY_WARMUP` and runs one targeted SQLite query before
the measured cold batch, while still counting that work inside reopen/bootstrap
time so the end-to-end sample remains honest.

Two warmup modes were tested:

- `limit1`: run the same indexed query with `limit = 1`
- `full`: run the full indexed query once before the measured batch

Using the same reduced-round cold drill (`1` steady warmup, `1` steady sample,
`3` cold samples), the end-to-end cold medians did not improve:

| SQLite cold experiment | Median per op |
| --- | ---: |
| baseline | `1.57 ms` |
| warmup `limit1` | `1.66 ms` |
| warmup `full` | `1.65 ms` |

The profile logs explain why. Both warmup modes did make the next measured
query look warm:

- baseline first cold query total: about `9.78-10.38 ms`
- first measured query after `limit1` warmup: about `0.95-1.03 ms`
- first measured query after `full` warmup: about `0.94-0.98 ms`

But the warmup itself cost almost the same as the cold query it removed:

- `limit1` warmup: about `10.54-11.19 ms`
- `full` warmup: about `11.00-12.89 ms`

That gives us a useful constraint for the roadmap. A targeted app-level query
warmup is not a real optimization here; it mostly shifts the same page-touch
cost from the first user-visible query into reopen/bootstrap, and in this drill
it regressed the end-to-end cold sample slightly. The fact that even `limit1`
cost nearly as much as `full` also suggests the remaining penalty is dominated
by touching the index and row pages at all, not by returning many rows once the
path is hot.

So the next SQLite cold-start optimization should not be "run a tiny indexed
query on open." If we pursue this path further, it should be through a lower
level page-cache or file-read warmup strategy that is cheaper than simply
paying the cold query early.

## SQLite Raw Index Probe Experiment

We then tested the next lower-level hypothesis with a second benchmark-only
mode, `NEOVEX_SQLITE_INDEX_QUERY_WARMUP=raw-id-only`. Instead of going through
the service query path, this mode opens the cloned SQLite file through a
separate raw `rusqlite` connection, applies the manifest-resolved SQLCipher
key, and runs a cheaper covering-style probe:

```sql
SELECT id
FROM documents
WHERE table_name = ?1 AND json_extract(data_json, '$."status"') = ?2
ORDER BY id
LIMIT 1
```

The hope was that this would pre-touch the relevant index pages more cheaply
than a full document query. On the same reduced-round drill, the result was
decisively negative:

| SQLite cold experiment | Median per op |
| --- | ---: |
| baseline rerun | `1.63 ms` |
| raw probe `raw-id-only` | `4.79 ms` |

The lower-level probe did not just shift a few milliseconds around. It made
the cold path materially worse:

- the raw warmup itself cost about `5.55-10.21 ms`
- two measured SQLite cold samples then saw the first service query spike to
  about `69.18-78.68 ms` total
- on those same samples, tenant load/open-existing also rose to about
  `3.25-3.58 ms`, above the rerun baseline of about `1.21-1.83 ms`

The profile pattern suggests that a separate pre-touch connection is not a
cheap page-cache warmup for this path. It likely duplicates or perturbs the
same cold file work the real service query still needs to perform, and on this
setup it can actively amplify the first service query instead of smoothing it.

That closes out the obvious SQLite warmup branch for now:

- app-level warmup (`limit1` and `full`) is not worthwhile
- external raw SQLite probing (`raw-id-only`) is worse

So the next cold-open optimization should not be another SQLite pre-touch
experiment of this family. If we return to SQLite reopen work, it should be a
fundamentally different in-process strategy inside the real open path, not an
extra probe connection. Otherwise, the better next investment is to shift back
to redb read-first/open design and the remaining libsql plus release-proof
evidence.

That means the lower-level SQLite warmup branch is complete enough to close for
now. We tested the app-level query warmups, then the cheaper raw probe idea,
and neither produced a credible end-to-end win. There is no checked-in evidence
left suggesting that another external pre-touch or side-channel file-read probe
is likely to pay off on this path.

## Takeaways

- SQLite cold indexed-query reopen is now very close between plaintext and
  encrypted mode. Encryption overhead on the tuned path is small enough to be a
  product decision, not a blocker.
- redb also improved materially, especially on cold encrypted reopen, but still
  retains a more visible cold-start penalty than SQLite.
- For enterprise review, the honest current story is that encryption at rest is
  no longer dominated by service bootstrap on this workload; the remaining cost
  is mostly first-query reopen overhead, and it is modest for SQLite after the
  recent fixes.
- The most obvious SQLite warmup ideas are now exhausted. Both service-query
  warmups and a lower-level raw index probe were negative, so the roadmap
  should stop spending time on "pay the cold query early" variants and move to
  either a fundamentally different in-process open-path change or a different
  backend-evidence slice.
