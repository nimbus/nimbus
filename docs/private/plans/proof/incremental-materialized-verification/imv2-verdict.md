# IMV2 measurement verdict

Date: 2026-08-21  
Host: macOS arm64, 32 GiB physical memory  
Verdict: `MERKLE_REQUIRED`

## Method

Command:

```bash
cargo bench -p nimbus-engine --bench materialized-verification -- \
  --output docs/private/plans/proof/incremental-materialized-verification/imv2-raw.json
```

The command exited 0. The retained JSON contains all 36 document-count,
payload-size, and churn rungs. It records three isolated full-verifier samples
and 21 candidate samples per completed rung. Each full-verifier sample starts
in a fresh child process. This makes extra peak RSS relative to a clean process
baseline observable and prevents one resource-limited sample from taking down
the matrix parent.

The full-verifier sample limit is 60 seconds below one million documents and
15 seconds at one million documents. A timed-out sample is a censored lower
bound, never a measured completion. Each child has a 24 GiB address-space
limit. Churn setup has a 120-second budget checked between atomic batches. An
atomic batch can finish after the budget, so the raw data records actual setup
time and the exact requested and applied document counts.

The completed matrix has these states:

- 28 rungs reached their requested churn state. Eight larger churn rungs
  stopped at the setup budget and are `resource_limited_setup`.
- 23 full-verifier rungs completed exactly. Five reached the child time limit
  and retain censored lower-bound percentiles. Eight did not start because
  their requested churn state was not reached.
- The gate uses 100,000 and 1,000,000 documents with 1 KiB payloads and 0.1%
  churn. Both rungs reached their requested states. Thus, no resource-limited
  setup enters the five-condition decision.

`bytes_read` is the verified payload-byte lower bound across the three compared
materialized states. It excludes provider and envelope bytes. Each row labels
this scope. CPU time, allocations, allocated bytes, peak RSS, extra peak
RSS, state bytes, and raw latency samples remain in `imv2-raw.json`.

## Ratified gate

| Condition | Measurement | Limit | Result |
|---|---:|---:|---|
| 1. Full verifier at 100,000 documents, 1 KiB, 0.1% churn | 9.157025750 s p95; 2,430.40625 MiB p95 extra RSS | 1 s p95; 256 MiB | Pass: the full verifier exceeds both limits. |
| 2. Candidate speedup at 100,000 documents, 0.1% churn | 4 ns p95 root comparison; 2,289,256,437.5 times faster than full p95 | At least 5 times | Pass. |
| 3. Candidate speedup at 1,000,000 documents, 0.1% churn | 1 ns p95 root comparison; full p95 is at least 15 s, so speedup is at least 15,000,000,000 times | At least 10 times | Pass with a censored lower bound. |
| 4. Active-session write effect | Throughput -1.435864%; p99 commit latency +1.711999% | Absolute change at most 5% for each | Pass. |
| 5. Resident index bytes per leaf | 160 bytes | At most 192 bytes | Pass with 32 bytes of headroom. |

The condition 1 measured margin is 8.157025750 seconds, or 815.702575%, over
the latency limit. The p95 extra-RSS margin is 2,174.40625 MiB, or
849.377441%, over the memory limit.

The candidate root-comparison samples are timer-amortized O(1) comparisons.
They do not claim that a future engine API call will take one to four
nanoseconds. Conditions 2 and 3 need only 5-times and 10-times improvements,
and the measured margins are many orders of magnitude larger. Condition 4
separately measures the SHA-256 leaf update and treap-path update added to real
commits.

The 160-byte memory result is conservative. `size_of::<TreapNode>()` is 144
bytes. The calculation adds 16 bytes of allocator metadata per leaf even
though the prototype stores nodes in one `Vec` allocation.

## Write-overhead arm

The paired arm measured 1,000 real commits at 100,000 documents and 1 KiB
payloads. It added the measured candidate leaf-hash workload and treap update duration to
each matching commit sample. This removes the cache-order defect found in the
first sequential-arm attempt.

| Arm | Throughput | p50 commit | p95 commit | p99 commit |
|---|---:|---:|---:|---:|
| Baseline | 481.219508 writes/s | 0.537916 ms | 2.054875 ms | 3.193167 ms |
| Active session | 474.309852 writes/s | 0.567124 ms | 2.095083 ms | 3.247834 ms |

## Decision

The full verifier exceeds condition 1. The benchmark-only deterministic treap
meets conditions 2 through 5. The literal continuation verdict is therefore:

`MERKLE_REQUIRED`

IMV3 is eligible after the IMV2 pull request merges. The benchmark prototype
does not enter production. IMV3 must define the storage-owned versioned root,
prove batch and incremental equivalence, and screen maintained dependencies
before Nimbus retains custom tree code.
