# IMV7 closeout performance

Date: 2026-08-25  
Host: macOS arm64, 32 GiB physical memory  
Accepted verdict: `MERKLE_REQUIRED`

## Source and method

The run used merged `main` at `80f845952`. Closeout documentation commit
`8448c42b5` changes no measured code. The benchmark-only correction in this
pull request records a typed seed-capacity failure instead of losing the
matrix checkpoint. It does not change production code, measured thresholds,
or successful-rung behavior.

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

The final JSON contains 36 unique coordinates. They are the exact product of
three document counts, three payload sizes, and four churn levels. Twenty-seven
coordinates reached their requested churn state. Five retained bounded churn
setup results. Four retained `resource_limited_seed` results.

Twenty full verifier rows completed. Seven reached a sample resource limit.
Nine did not start because setup was incomplete. Both gate rungs reached their
exact requested states. Capacity-limited 8 KiB stress rows do not enter the
verdict.

Each completed full-verifier row has three fresh-process samples. Each
candidate row has 21 root-comparison samples. The full sample limit is 60
seconds below one million documents and 15 seconds at one million documents.
A timeout is a censored lower bound, not a measured completion. The one-minute
verification interval and all IMV2 thresholds remain unchanged.

## Ratified gate

| Condition | Final measurement | Limit | Result |
|---|---:|---:|---|
| 1. Full verifier at 100,000 documents, 1 KiB, 0.1% churn | 13.537241 s p95; 3,149.28125 MiB p95 extra RSS | 1 s p95; 256 MiB | Pass: both limits are exceeded. |
| 2. Candidate speedup at 100,000 documents, 0.1% churn | 1 ns p95 root comparison; 13,537,241,000 times faster | At least 5 times | Pass. |
| 3. Candidate speedup at 1,000,000 documents, 0.1% churn | 1 ns p95 root comparison; full p95 is at least 15 s | At least 10 times | Pass with a 15,000,000,000-times censored lower bound. |
| 4. Active-session write effect | Throughput -1.517947%; p99 commit latency +0.693269% | Absolute change at most 5% for each | Pass. |
| 5. Resident index bytes per leaf | 160 prototype bytes | At most 192 bytes | Pass with 32 bytes of headroom. |

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

## Comparison with IMV2

The matched IMV2 decisive full-verifier p95 was 9.157025750 seconds. The final
value is 47.834476% higher. IMV2 p95 extra RSS was 2,430.40625 MiB. The final
value is 29.578388% higher. Both results remain far beyond condition 1.
Therefore, the continuation decision does not change.

The IMV2 prototype comparison was 4 ns p95 at the decisive rung. The final
comparison is 1 ns p95. These timer-amortized comparisons show the order of
magnitude only. They do not claim that an engine request completes in one
nanosecond. The required speedups are 5 times and 10 times, and both final
margins are more than one billion times.

The IMV2 write arm changed throughput by -1.435864% and p99 commit latency by
+1.711999%. The final changes are -1.517947% and +0.693269%. All four values
are within the ratified 5% bounds.

## Production memory evidence

The benchmark prototype reports 160 bytes per leaf: 144 bytes for one node
and 16 bytes of conservative allocator metadata. Production memory evidence
comes from the ignored generated-history test, not from that prototype:

```text
verification_root leaves=1000000 max_depth=55 \
resident_bytes_per_leaf=149 budgeted_bytes_per_leaf=164
test verification_root_million_leaf_depth_and_memory_meet_imv2_limits ... ok
```

The production measured value has 43 bytes of headroom to the 192-byte limit.
The production budget has 28 bytes of headroom. The maximum treap depth is 55.

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
