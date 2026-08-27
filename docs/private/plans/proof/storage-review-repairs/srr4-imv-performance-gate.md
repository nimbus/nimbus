# SRR4 IMV Performance Gate

Date: 2026-08-26
Work commit: `132343e37`

## Outcome

IMV7 now requires measured production-candidate latency and resident bytes at
both decisive rungs. A slow or censored full-verifier baseline cannot make the
candidate pass.

## Fail-Before Evidence

The previous condition 15 ignored candidate status and raw sample count. A
fixture with `resource_limited` status and no candidate samples still passed
all old candidate expressions:

```text
invalid candidate status=resource_limited
invalid candidate sample_count=0
old condition-15 candidate arms accepted=True
```

## Repair

The benchmark has a focused `--candidate-only` mode. It measures the production
`MaterializedVerificationIndex` at 100,000 and 1,000,000 leaves. Each of 21
samples applies 0.1% churn with 1 KiB values and then reads the root.

The proof parser now requires:

- `measured` latency and resident-byte status.
- 21 positive raw samples and summaries recomputed with the declared
  percentile algorithm.
- p95 at most 1 second for 100,000 leaves and at most 60 seconds for one
  million leaves.
- measured production-index storage at most 192 bytes per leaf in total.
- an uncensored measured full-verifier result at the 100,000-document decision
  rung.
- write-throughput loss at most 5% and p99 commit-latency increase at most 5%.

The censored one-million-document full-verifier result remains diagnostic. It
does not decide the candidate verdict.

## Measurements

| Rung | Candidate p95 | Absolute limit | Resident bytes | Absolute total |
| --- | ---: | ---: | ---: | ---: |
| 100,000 leaves | 2,871,625 ns | 1,000,000,000 ns | 14,800,072 | 19,200,000 |
| 1,000,000 leaves | 35,753,750 ns | 60,000,000,000 ns | 148,000,072 | 192,000,000 |

Both production measurements retain 149 bytes per leaf. The full-verifier p95
is 13,537,241,000 ns, and its p95 extra RSS is 3,302,260,736 bytes. The retained
write changes are -1.517947% throughput and +0.693269% p99 latency.

## Negative Tests

The mutation helper passes nine checks. The accepted proof passes. Five invalid
mutations cover malformed JSON, empty samples, censored status, excessive
candidate latency, and excessive resident memory. Three more cover a truncated
decisive full sample set, the wrong full-matrix host class, and a missing
candidate host class. Each mutation fails with a concise error and no traceback. Fixture
generation also runs under `set -e`, so a generator error fails the helper
instead of becoming an accepted missing-file mutation.

## Verification

| Command or gate | Result |
| --- | --- |
| Focused candidate benchmark | Completed both rungs and wrote the retained JSON artifact. |
| IMV7 performance parser | Passed with the measurements above. |
| IMV7 mutation helper | `Summary: 9 passed, 0 failed`. |
| Complete IMV verifier | `Summary: 16 passed, 0 failed`. |
| Engine benchmark `cargo check` | Passed. |
| Strict engine benchmark Clippy | Passed; existing vendored warnings remain. |
| Rust format, Bash syntax, and ShellCheck | Passed. The existing `SC2016` exclusions in `verify.sh` remain. |
| Technical-writing lint | Passed for the changed IMV proof documents. |
| `git diff --check` | Passed. |

## Remaining Uncertainty

Absolute timing remains host-specific. The focused artifact measures the
production index and accepted service limits on the same macOS arm64 host
class. SRR5 owns the complete repository gates and final second-model review.
