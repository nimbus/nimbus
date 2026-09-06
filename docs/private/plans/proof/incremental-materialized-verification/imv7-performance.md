# IMV7 closeout performance

Date: 2026-08-25  
Host: macOS arm64, 32 GiB physical memory  
Accepted verdict: `MERKLE_REQUIRED`

## Source and method

The run used merged `main` at `80f845952`. Closeout documentation commit
`8448c42b5`, rebased as `ae732808a`, changes no measured code. The
benchmark-only correction in this pull request records a typed seed-capacity
failure instead of losing the matrix checkpoint. It does not change
production code, measured thresholds, or successful-rung behavior.

The closeout branch later rebased onto main `ae4a1b233`. The intervening
commits add an engine process fence and the storage metadata-retention
baseline. They do not change materialized verification, its benchmark, or the
measured workload. The measurement was not relabeled or repeated. On the
rebased head, the fixed verifier reports `Summary: 16 passed, 0 failed`.

SRR4 adds a focused production-candidate measurement based on `78ff3586a`.
The benchmark source has SHA-256
`4d7724e513137e4b185410f8718cee774125d6fa7d78393e8d09c6e17ba9a3fe`.
The new artifact has SHA-256
`a4040747788f0719f6b9c09417e3f5fd74fdcdbbf5750d9c14a7e3d23958b935`.
It measures the production `MaterializedVerificationIndex`, not the earlier
prototype treap.

The first command ran the complete matrix:

```bash
cargo bench -p nimbus-engine --bench materialized-verification -- \
  --output docs/private/plans/proof/incremental-materialized-verification/imv7-raw.json
```

It retained 32 coordinates before the 1,000,000-document, 8 KiB fixture
exhausted local disk capacity. The second command repeated only that missing
payload family:

```bash
cargo bench -p nimbus-engine --bench materialized-verification -- \
  --output docs/private/plans/proof/incremental-materialized-verification/imv7-tail-raw.json \
  --documents 1000000 --payload-bytes 8192
```

The second command exited 0. It retained the typed `ResourceExhausted` result
for all four churn coordinates. The final `imv7-raw.json` mechanically joins
the 32-row checkpoint and the four-row tail. It keeps the write-overhead arm
from the first command. The retained tail file makes the split reproducible.

The focused command measured the production candidate at both decisive rungs:

```bash
cargo bench -p nimbus-engine --bench materialized-verification -- \
  --candidate-only \
  --output docs/private/plans/proof/incremental-materialized-verification/imv7-candidate-raw.json
```

Each candidate sample applies 0.1% churn to the production index with 1 KiB
values and then reads the root. Each rung has 21 raw samples. The report reads
retained bytes from `MaterializedVerificationIndex::resident_bytes` after the
samples. It does not use a prototype node-size constant.

The final JSON contains 36 unique coordinates. They are the exact product of
three document counts, three payload sizes, and four churn levels. Twenty-seven
coordinates reached their requested churn state. Five retained bounded churn
setup results. Four retained `resource_limited_seed` results.

Twenty full verifier rows completed. Seven reached a sample resource limit.
Nine did not start because setup was incomplete. Both gate rungs reached their
exact requested states. Capacity-limited 8 KiB stress rows do not enter the
verdict.

Each completed full-verifier row has three fresh-process samples. The legacy
matrix candidate rows have 21 timer-amortized prototype root comparisons. They
remain historical diagnostics and do not satisfy the final candidate gate.
The full sample limit is 60 seconds below one million documents and 15 seconds
at one million documents. A timeout is a censored lower bound, not a measured
completion.

The absolute candidate limits use the approved service targets. The
100,000-document rung must finish within the 1-second full-verifier threshold.
The 1,000,000-document rung must finish within the 60-second repeated-check
interval. Each rung must retain at most 192 bytes per leaf in total production
index storage. A slow full verifier cannot make these limits pass.

## Ratified gate

| Condition | Final measurement | Limit | Result |
|---|---:|---:|---|
| 1. Full verifier at 100,000 documents, 1 KiB, 0.1% churn | 13.537241 s p95; 3,149.28125 MiB p95 extra RSS | 1 s p95; 256 MiB | Pass: both limits are exceeded. |
| 2. Production candidate at 100,000 documents, 1 KiB, 0.1% churn | 2.871625 ms p95 | At most 1 s p95 | Pass with 997.128375 ms of headroom. |
| 3. Production candidate at 1,000,000 documents, 1 KiB, 0.1% churn | 35.753750 ms p95 | At most 60 s p95 | Pass with 59.964246250 s of headroom. |
| 4. Active-session write effect | Throughput -1.517947%; p99 commit latency +0.693269% | Throughput loss at most 5%; p99 increase at most 5% | Pass. |
| 5. Production index resident bytes | 14,800,072 at 100,000 leaves; 148,000,072 at 1,000,000 leaves | 19,200,000; 192,000,000 | Pass with 4,399,928 and 43,999,928 bytes of headroom. |

The condition 1 latency measured margin is 12.537241 seconds, or 1,253.7241%,
over the limit. The p95 extra-RSS measured margin is 2,893.28125 MiB, or
1,130.187988%, over the limit.

The write arm measured 1,000 matching commits per side. Baseline throughput
was 620.662894 writes per second. Active-session throughput was 611.241560
writes per second. Baseline p99 commit latency was 3.131250 ms.
Active-session p99 was 3.152958 ms.

The throughput result has 3.482053 percentage points of headroom to its
absolute limit. The p99 latency result has 4.306731 percentage points of
headroom.

## Diagnostic comparison with IMV2

The matched IMV2 decisive full-verifier p95 was 9.157025750 seconds. The final
value is 47.834476% higher. IMV2 p95 extra RSS was 2,430.40625 MiB. The final
value is 29.578388% higher. Both results remain far beyond condition 1.
Therefore, the continuation decision does not change.

The measured production candidate is approximately 4,714 times faster at
100,000 documents. The 15-second censored lower bound is approximately 419
times the candidate p95 at one million documents. These relative values are
diagnostic only. The gate uses the absolute candidate limits above.

The IMV2 write arm changed throughput by -1.435864% and p99 commit latency by
+1.711999%. The final changes are -1.517947% and +0.693269%. All four values
are within the ratified 5% bounds.

## Production memory evidence

The focused artifact reports 14,800,072 resident bytes at 100,000 leaves and
148,000,072 bytes at one million leaves. Both values round to 149 bytes per
leaf. The measurement uses the production index's retained vector capacities.
The ignored generated-history test independently reports the same million-leaf
value:

```text
verification_root leaves=1000000 max_depth=55 \
resident_bytes_per_leaf=149 budgeted_bytes_per_leaf=164
test verification_root_million_leaf_depth_and_memory_meet_imv2_limits ... ok
```

The focused production measurement has 43 bytes per leaf of headroom. The
production budget has 28 bytes of headroom. The focused index depth is 51 at
one million leaves. The generated-history depth is 55.

## Assurance and remaining uncertainty

An incremental root match checks the engine-owned apply result at an exact
applied sequence. It is fast and can reuse a bounded process-local session. It
does not prove provider contents when corruption preserves that sequence. A
full scrub reads the provider state and supplies that stronger assurance.

Nimbus forces a full scrub for a missing or invalid session. It also forces a
scrub for a retention gap, a root mismatch, an explicit operator request, and
the other unsafe states in the operating guide. Nimbus does not schedule
periodic verification today.

Absolute timing and RSS depend on this host. The four 1,000,000-document,
8 KiB rows prove only that this host could not construct that stress fixture
with the available disk. They do not weaken or replace either exact decision
rung. IMV6 owns provider qualification. Its corrected hosted PostgreSQL,
MySQL, and libSQL lanes are green.

## Decision

The full verifier still exceeds condition 1. The final implementation meets
conditions 2 through 5 with the measured margins above. The accepted verdict
remains:

`MERKLE_REQUIRED`
