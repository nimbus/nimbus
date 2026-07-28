# SWT1 A/B Acceptance — Prepared Statements + Batch-Invariant Apply Context

Date: 2026-07-28. Same-session paired A/B; base and candidate built once from
clean worktrees and alternated in balanced block order.

| Artifact | Commit | Layered binary | Engine binary |
| --- | --- | --- | --- |
| Base `B_ref` | `2a1853dab` | `288aec36…bbb2d8` | `4e2efa7c…7299ef` |
| Candidate SWT1 | `9a6d7b744` | `3f6852ef…1aca0b` | `b9e3276f…b6a8d8` |

Raw reports for every valid pair are retained byte-for-byte under `raw/`;
whole rejected runs (CV>10% in any lane) under `rejected/`. No samples were
merged across runs; noisy pairs were rejected whole.

## Primary gate — canonical CRUD, paired blocks B,C,C,B,B,C,C,B

Pair 1 was rejected whole (candidate run c1 had one collapsed N=32 round,
CV 22.8%). Three valid pairs remain:

| Pair | Base N=256 | Candidate N=256 | Delta |
| --- | ---: | ---: | ---: |
| (b2,c2) | 29,100 | 45,717 | +57.1% |
| (b3,c3) | 29,310 | 45,020 | +53.6% |
| (b4,c4) | 29,282 | 44,407 | +51.7% |

**N=256 paired mean +54.1%, 95% CI [+47.3, +61.0] (t, df=2) — the ≥5% gate
passes with the lower bound nine times above it.** Candidate absolute mean
45,048 mut/s. N=32 paired mean +19.9% [+15.7, +24.0]. Effective batch at
N=256: base ~125–127, candidate ~119–120 (−4.8%, inside the ≤5% batch gate;
faster drains form slightly smaller batches).

## N=1 cross-gate — six dedicated paired blocks

Deltas −3.05, −5.71, −1.51, −5.22, −4.40, −2.86 → **mean −3.79%,
CI [−5.46, −2.12] (t, df=5). Within the ≤5% regression gate.** Mechanism:
at batch size 1 the per-transaction context/cache setup amortizes over one
record; SWT2's resident writer targets exactly this territory.

## Hot-key N=32 cross-gate — six dedicated paired blocks — DEVIATION

Deltas −5.38, −5.13, −5.22, −5.21, −6.53, −5.29 → **mean −5.46%,
CI [−6.02, −4.91]. The ≤5% gate fails by ~0.5 points, confirmed and
reproducible.** The full hot-key runs show the same shape at N=256 (−6.7%).
No per-operation cost is visible on any other lane; the diff makes apply
turnaround ~2× faster, which shifts the OCC retry equilibrium on the
pathological single-document workload: the serial window turns around
faster, more retries fit per committed update, and retry work per success
rises. This is a system-equilibrium effect of the win, not hidden overhead.

**Owner decision (2026-07-28): accept with the deviation documented**
(decision D15). Retry-policy tuning is Engine-owned and out of SWT1's
storage scope; SWT2 re-measures every lane and changes small-batch
economics directly.

## Layered evidence

Valid pair (b1,c1): **production storage 43,333 → 91,324 logical mut/s
(+110.7%)**, both CVs ≤1.6%. Unchanged synthetic lanes moved ≤1.2%,
confirming session stability. The second pair was rejected whole (candidate
storage lane CV 26.8% under a load spike). Candidate storage now retains
~55% of the guarded SQL lane, up from ~26%.

## Fail-before evidence

- SWT1.1: `swt11-fail-before-red.txt` (cached-prepare bound RED on uncached
  tree), `swt11-fail-after-green.txt` (GREEN after conversion; execute
  counts byte-identical to the SWT0 census).
- SWT1.2: `swt12-fail-before-red.txt` (hoisted census RED pre-hoist:
  format/schema/identity checks 3 per record vs 1 per batch). Invalidation
  correctness: mid-batch schema change reloads the plan and opens the
  maintained-index interval only for the post-change write; multi-table
  batches check each distinct table once; every write keeps its preimage
  read (tests in `journal/observability.rs`).

## Correctness verification

- Storage suite: 428/428 (external-fixture opt-out documented).
- Engine journal/publisher/direct/execution-unit/fan-out groups: 253/253,
  including torn-tail replay, sequence-order-across-retry, fsync
  amortization, and ppsc differentials.
- `cargo fmt --all --check`; clippy `-D warnings` on storage+engine: clean.
