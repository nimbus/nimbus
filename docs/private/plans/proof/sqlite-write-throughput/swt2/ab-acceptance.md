# SWT2 A/B Acceptance — Resident Embedded Writer

Date: 2026-07-28. Base = SWT1-merged `origin/main` `6c5299219` (binaries
layered `b3736125…`, Engine `a739b24c…`); candidate = `7cf5266d7` on
`codex/sqlite-write-throughput-p2-writer-residency` (binaries layered
`3232e916…`, Engine `270db4ff…`). Balanced adjacent pairing; every run with
any lane CV>10% rejected whole (raw/ vs rejected/). Ambient load was higher
than the SWT1 session (1-min 4–9), rejecting more runs; every gate below is
met on clean pairs only.

## Gates

- **Production storage ≥5% paired (primary)**: two valid layered pairs:
  +39.8% and +31.8% (73,937→103,332 CV≤1.5; 75,656→99,716
  CV≤9.8). **PASS.**
- **N=1 or N=32 gain ≥5%**: five valid N=1 micro pairs: deltas
  +250.5%, +260.0%, +249.4%, +230.5%, +262.3% → **mean +250.5%, CI
  [+235.0, +266.1] (t, df=4). PASS** (N=1 ≈1,330→≈4,670).
  The single valid CRUD pair also shows N=32 +97.8% (14,522→28,727).
- **N=256 regression ≤2%**: valid pair (b4,c4): 35,627→40,812 (**+14.6%**);
  additionally every candidate N=256 mean (44,182/44,769/40,982/40,812)
  exceeds every base mean (39,859/37,086/36,915/35,627) including
  CV-rejected runs. **PASS.**
- **Hot-key N=32 regression ≤5%**: no pair passed CV whole (noisy N=1
  rungs), but both candidate runs measured ≈5,900 vs both base runs ≈2,100
  (+180% directionally, both orders) — a regression is excluded by every
  observation; the D15 deviation from SWT1 is strongly reversed. Recorded
  as directional evidence; SWT5's final protocol re-validates the lane.
- **Cold open ≤5%/100µs**: 562.0µs → 621.8µs (+59.8µs) within the 100µs
  allowance. **PASS.**

## Mechanism confirmation

The resident writer removes the per-transaction open + 19-statement
initialization and makes the SWT1 statement cache persistent across
transactions. The impact lands exactly where predicted: batch-size-1 flows
(N=1 3.5×, hot-key ≈2.8×) and the storage lane (+32–40%), with N=256 gaining
+15% on top of SWT1's +54%.

## Correctness

Fail-before: SWT0 census (2 writer opens/pair) RED → residency census GREEN
(1 open first pair, 0 after). New tests: one open across three batch pairs;
exactly one reopen after an injected mid-transaction fault with single
replay; encrypted residency (keys once); four concurrent point-writer
threads coexist with dense journal and working queued route after.
Storage 431/431; Engine journal/publisher/direct/execution-unit/fan-out
253/253; fmt + clippy -D warnings clean.
