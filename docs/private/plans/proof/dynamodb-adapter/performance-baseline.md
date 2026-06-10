# DynamoDB Adapter — Performance Benchmark Baseline (D9.6)

Per-operation-family latency baseline, produced by the custom-harness benchmark
`crates/nimbus-dynamodb/benches/operations.rs`
(`cargo bench -p nimbus-dynamodb --bench operations`). Each family is driven
through the public `dispatch` against an in-process, tempdir-backed `Service`, so
the numbers isolate **adapter + engine** cost (no network or SDK overhead).

## Environment

| Field | Value |
| --- | --- |
| Host | Apple M2 Max, 12 cores, macOS (Darwin arm64) |
| Storage backend | engine default, tempdir-backed (`Service::new`) |
| Concurrency | 1 (sequential dispatch) |
| Iterations / family | 1000 |
| Dataset | `Bench` 1 item; `BenchRange` 20 items (1 partition, 20 sort keys); `BenchStream` 1 captured event |
| Item size | small (~tens of bytes; single `N`/`S` attribute) |
| Commit | `2238a64b` |

## Latency (microseconds)

| Operation family | p50 | p95 | p99 |
| --- | --- | --- | --- |
| PutItem | 741.8 | 1006.5 | 1228.8 |
| GetItem | 6.0 | 6.8 | 7.3 |
| UpdateItem | 728.2 | 1066.7 | 1507.4 |
| Query | 132.3 | 201.7 | 232.2 |
| Scan | 50.2 | 59.5 | 67.7 |
| BatchGetItem | 6.4 | 8.2 | 10.2 |
| BatchWriteItem | 738.1 | 899.8 | 1014.9 |
| TransactWriteItems | 31.0 | 38.3 | 51.7 |
| Streams GetRecords | 17.8 | 25.3 | 29.2 |

Throughput at this concurrency is the reciprocal of p50 (e.g. GetItem ≈ 167k
ops/s, PutItem ≈ 1.3k ops/s single-threaded). The write families (Put/Update/
BatchWrite) are dominated by the engine's durable commit (fsync) per write;
reads and the stream path are in the single-digit-to-low-hundreds µs range.

## Initial non-regression thresholds

These are the first baseline; a later run regresses if a family's **p99** exceeds
**2×** the value above (a generous guard that catches real regressions while
absorbing host noise). Suggested CI alarm thresholds (p99 µs):

| Family | p99 baseline | Regression alarm (2×) |
| --- | --- | --- |
| PutItem | 1228.8 | 2458 |
| GetItem | 7.3 | 15 |
| UpdateItem | 1507.4 | 3015 |
| Query | 232.2 | 465 |
| Scan | 67.7 | 135 |
| BatchGetItem | 10.2 | 20 |
| BatchWriteItem | 1014.9 | 2030 |
| TransactWriteItems | 51.7 | 103 |
| Streams GetRecords | 29.2 | 58 |

Re-run `cargo bench -p nimbus-dynamodb --bench operations` to refresh; the harness
prints a CSV (`operation,p50_us,p95_us,p99_us,iters`) for easy diffing.
