# PPSC4 slice B performance follow-up

Date: 2026-07-16

This proof closes the hot-key retry-herd and CRUD regressions found after the
initial PPSC4 slice B implementation. The performance gates pass on the final
code: hot-key N=32 is unchanged from `main@faa9c865c` within overlapping 95%
confidence intervals, and CRUD is faster at every requested rung.

## Design correction

The serial-step recovery function is
`reprepare_single_document_from_window`. It is synchronous and reconstructs a
stale path-A/C single-document write solely from the PPSC2 full-image window.
`single_document_change_since` and `current_document_state` are in-memory
write-log lookups. The only `runtime.store` access in the recovery function is
the process-local sequence-authority capability check; the function contains no
await and performs no storage I/O.

| Operation or condition | Result |
| --- | --- |
| Single-document insert/create, latest image absent | Rebuild and reauthorize inline, then assign, stamp, stage, and append |
| Single-document update/patch, latest image present | Reapply the patch to the latest full image and rebuild inline |
| Single-document delete, latest image present | Rebuild the delete from the latest full image inline |
| Insert with a latest image, or update/delete without one | Return a typed conflict; the write precondition genuinely failed |
| Image outside retention/bootstrap, table-wide lifecycle change, or path-C dependency on an unapplied predecessor | Return caller-wait retryable conflict |
| Multi-document or execution-unit commit with read dependencies | Preserve caller wait and first-committer-wins validation |

Inline recovery increments `inline_reprepare_total`; the caller retry loop alone
increments `reprepare_total`. The hot-key criterion test requires the former to
be nonzero and the latter to remain zero.

## CRUD diagnosis and correction

Raising `NIMBUS_PREPARE_CONCURRENCY` from 4 to 32 was measured and rejected:
the diagnostic CRUD ladder reached only 6,084 mut/s at N=32 (95% CI
[5,671, 6,497]). Permit width was therefore not the root fix.

Path A/C single-document prepare now reads the published full image from the
window first and runs synchronously in the caller's async task. Storage and the
bounded blocking permit remain a fallback for bootstrap/retention gaps,
scheduled/provider cases, and other operations that cannot be prepared from the
window. The write log maintains O(1) published and pending per-document image
indexes, including the short published-ahead-of-applied interval. The final
release ladders recorded window/storage prepare counts of 500/0 and 800/0 for
hot-key, and 4,500/0, 120,000/0, and 119,040/0 for CRUD.

## Benchmark method

The baseline was built in an isolated clone detached at `faa9c865c`, with only
the benchmark harness commits `f0275ce3a` and `77e7c357f` cherry-picked. The
candidate used this branch's final code. Both used release mode, SQLite, split
phase accounting, one discarded warmup round, five measured rounds, and the
same workload-specific ladders and mutation caps. A contaminated shared target
was detected during setup and discarded; both reported binaries were rebuilt
cleanly before evidence was accepted.

### Hot-key throughput

| N | main mean mut/s | main 95% CI | main median | main CV | candidate mean mut/s | candidate 95% CI | candidate median | candidate CV | delta | verdict |
| --- | ---: | --- | ---: | ---: | ---: | --- | ---: | ---: | ---: | --- |
| 1 | 536 | [527, 544] | 539 | 1.2% | 488 | [452, 523] | 482 | 5.9% | -9.0% | Diagnostic rung; not the stated hot-key gate |
| 32 | 3,039 | [2,925, 3,153] | 3,007 | 3.0% | 3,037 | [2,916, 3,158] | 3,071 | 3.2% | -0.07% | Pass: confidence intervals overlap materially |

| N | side | p50 us | p95 us | p99 us | Little's Law N~X*R |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | main | 1,823.0 | 2,030.2 | 2,169.4 | 1.0 |
| 1 | candidate | 1,886.3 | 3,022.8 | 4,467.6 | 1.0 |
| 32 | main | 9,735.6 | 12,650.1 | 13,057.8 | 29.0 |
| 32 | candidate | 10,081.9 | 10,948.0 | 12,656.4 | 29.0 |

| N | side | avg batch | window/storage prepare | plan CPU | conflict check | apply | fsync/append | measured under-gate |
| --- | --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | main | 0.00 | n/a | 0.1% | 3.0% | 0.8% | 96.1% | 286.883 ms |
| 1 | candidate | 0.00 | 500/0 | 4.4% | 0.1% | 0.4% | 95.0% | 351.222 ms |
| 32 | main | 0.00 | n/a | 0.3% | 7.4% | 0.8% | 91.5% | 245.628 ms |
| 32 | candidate | 0.00 | 800/0 | 4.5% | 4.0% | 0.3% | 91.2% | 247.080 ms |

### CRUD throughput

| N | main mean mut/s | main 95% CI | main median | main CV | candidate mean mut/s | candidate 95% CI | candidate median | candidate CV | delta | verdict |
| --- | ---: | --- | ---: | ---: | ---: | --- | ---: | ---: | ---: | --- |
| 1 | 1,795 | [1,748, 1,843] | 1,797 | 2.1% | 1,971 | [1,879, 2,062] | 2,009 | 3.7% | +9.8% | Pass: better |
| 32 | 11,286 | [10,973, 11,599] | 11,204 | 2.2% | 13,247 | [12,998, 13,496] | 13,274 | 1.5% | +17.4% | Pass: better |
| 256 | 17,880 | [17,627, 18,133] | 17,867 | 1.1% | 21,749 | [21,398, 22,100] | 21,623 | 1.3% | +21.6% | Pass: better |

| N | side | p50 us | p95 us | p99 us | Little's Law N~X*R |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | main | 528.1 | 653.2 | 798.5 | 1.0 |
| 1 | candidate | 486.5 | 604.3 | 812.8 | 1.0 |
| 32 | main | 2,546.2 | 3,828.1 | 13,591.2 | 31.9 |
| 32 | candidate | 2,344.3 | 3,020.2 | 4,644.0 | 31.9 |
| 256 | main | 12,094.5 | 32,733.9 | 40,343.9 | 252.3 |
| 256 | candidate | 12,114.1 | 17,115.4 | 19,412.7 | 252.0 |

| N | side | avg batch | window/storage prepare | plan CPU | conflict check | apply | fsync/append | measured under-gate |
| --- | --- | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | main | 1.00 | n/a | 4.3% | 0.2% | 52.4% | 43.1% | 2,396.487 ms |
| 1 | candidate | 1.00 | 4,500/0 | 1.4% | 0.2% | 53.8% | 44.6% | 2,164.038 ms |
| 32 | main | 17.79 | n/a | 16.3% | 0.7% | 52.7% | 30.3% | 10,151.678 ms |
| 32 | candidate | 16.11 | 120,000/0 | 11.6% | 2.1% | 57.3% | 29.0% | 8,493.305 ms |
| 256 | main | 185.42 | n/a | 25.1% | 0.3% | 52.2% | 22.4% | 6,239.730 ms |
| 256 | candidate | 140.54 | 119,040/0 | 20.8% | 3.6% | 60.4% | 15.2% | 4,977.336 ms |

Raw reports:

- `/private/tmp/cwb-p3-main-hotkey.md`
- `/private/tmp/cwb-p3-main-crud.md`
- `/private/tmp/cwb-p3-final-hotkey.md`
- `/private/tmp/cwb-p3-final-crud.md`

## Correctness and repository gates

| Gate | Result |
| --- | --- |
| `cargo fmt --all --check` | Pass |
| `python3 scripts/test-taxonomy.py check` | Pass (`test taxonomy ok`) |
| `cargo hakari generate --diff` | Pass |
| `cargo build --workspace` | Pass |
| core + storage + engine | 949 passed, 4 skipped; prior 946 baseline plus three new P1/P2 tests |
| server | 557 passed, 23 skipped; nextest classified two passing tests as leaky in the broad rerun, and the identified retry-test cluster reran 3/3 without a leak |
| PPSC4 per-path matrix | 9/9 passed |
| `hot_key_direct_writes_reprepare_inline_without_caller_retry` and `out_of_window_stale_prepare_falls_back_to_caller_wait` | 2/2 passed |
| `window_vs_storage_scan_differential` | 1/1 passed |
| Hermitage | 11/11 passed |
| live Postgres provider (`NIMBUS_TEST_POSTGRES_URL`, Homebrew PostgreSQL 17.9) | 12/12 passed, including resource-path binding coverage |
| `make verify-loom-handoff` | 3/3 passed; model unchanged because the handoff protocol did not change |
| `make clippy` | Pass with `-D warnings` |

The serial composition root remains below the modularity threshold:
`journal.rs` is 910 lines. No compatibility shim, migration path, feature flag,
or loom-model change was introduced.

## Deviations

- Hot-key N=1 is 9.0% below the contemporary baseline and its confidence
  interval ends 4 mut/s below the baseline interval. PPSC4's stated hot-key
  acceptance gate is N=32; that gate passes. CRUD N=1, the P2 sequential gate,
  is 9.8% faster than baseline.
- A structured external autoreview was attempted twice, but the nested Codex
  app server could not start in this sandbox (`Operation not permitted`). A
  manual diff audit found and fixed trigger-origin reconstruction, schema table
  ID cache invalidation, published-ahead-of-applied lookup, and the large enum
  payload flagged by clippy.
