# Hot-Key Backpressure Validation

Date: 2026-07-28

Purpose: prove that the canonical N=256 hot-key workload treats expected
bounded-inbox pressure as measured client wait rather than panicking on
`CommitterFull`.

Verdict: **functional PASS; rejected as a performance baseline**. The exact
canonical N=1/32/256 protocol completed with exit 0, including all fifteen
measured N=256 rounds. N=32 CV was 12.1% and N=256 CV was 27.2%, so the whole
report is ineligible for throughput acceptance. SWT0 still owns the quiet-host
hot-key baseline.

Git head:
`f6fc2cd86ea742b8c8780b3af4a44f82449f36b0`

The worktree contained the intended uncommitted benchmark-only retry patch.

Benchmark binary SHA-256:
`da4040b8c8ac4807579f67e475a460631eb8a98965970bd278b827f27d7ac30c`

Raw report: `hotkey-backpressure-validation-raw.md`

Raw report SHA-256:
`9835ccc20097e361dc2c0d437ab6533932d7fa828aad675f22ebc43acd160236`

Command:

```bash
timeout 600 env \
  NIMBUS_CWB_WORKLOAD=hotkey \
  NIMBUS_CWB_LADDER=1,32,256 \
  NIMBUS_CWB_OPS_PER_WORKER=100 \
  NIMBUS_CWB_MAX_MUTATIONS_PER_ROUND=9000 \
  NIMBUS_CWB_MEASURE_ROUNDS=15 \
  NIMBUS_CWB_WARMUP_ROUNDS=3 \
  NIMBUS_CWB_SPLIT_PHASES=1 \
  NIMBUS_CWB_OUT=/tmp/nimbus-hotkey-backpressure-validation.md \
  /Users/jack/src/github.com/nimbus/nimbus/target/release/deps/concurrent_write_throughput-d0d97d4acf36a759
```

Result summary:

| N | Mean mut/s | 95% CI | CV | Completion |
| ---: | ---: | ---: | ---: | --- |
| 1 | 586 | 571–601 | 4.7% | PASS |
| 32 | 2,487 | 2,321–2,653 | 12.1% | PASS; noisy |
| 256 | 1,970 | 1,673–2,268 | 27.2% | PASS; noisy |

