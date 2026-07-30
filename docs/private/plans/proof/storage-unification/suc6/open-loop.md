# SUC6.1 — Open-Loop Service-Latency Companion

## Harness

`benches/concurrent-write-throughput.rs` gains an env-gated open-loop mode
(`NIMBUS_CWB_OPEN_LOOP_RATES` — fractions of the top rung's measured
closed-loop capacity; `_SECONDS`, `_ROUNDS`). Arrivals follow a fixed
schedule (`arrival_i = start + i/rate`); each latency is measured from the
scheduled arrival, not dispatch, so a slow engine cannot thin the arrival
process (the coordinated-omission fix the closed-loop harness's own header
says it needs). The engine's mutation admission gate sheds bursts by design;
shed arrivals are counted per round and never timed, the verdict names them,
and percentiles are explicitly scoped to the admitted subset. A round whose
in-flight work exceeds a bound aborts as saturation-breached. Closed-loop
behavior is untouched when the env is absent.

First run iteration: the harness originally panicked on `Overloaded`; that
run itself demonstrated the shedding behavior and drove the shed-aware
design (commit trail on the branch).

## Run (minicloud, pinned Linux/KVM, 4 cores, rustc 1.96.1, sqlite, insert workload)

This is the "pinned minicloud/KVM box" follow-up the campaign research doc
named as the prerequisite for publishable figures. Machine idle; nothing else
running. Full raw report: `open-loop-minicloud-report.md` (same directory).

Calibration (closed-loop): N=1 = 4,124 mut/s (CV 4.1%); N=256 = 22,113 mut/s
(CV 2.2%) — both inside the CV≤10% gate.

| Rate | Target/s | p50 ms | p90 ms | p99 ms | p99.9 ms (range over 3 rounds) | Shed |
| ---: | ---: | ---: | ---: | ---: | --- | ---: |
| 25% | 5,528 | 1.72–1.76 | 2.36–2.39 | 2.93–2.95 | 4.7–36.5 | 0 |
| 50% | 11,057 | 2.11–2.23 | 2.77–2.93 | 3.54–3.82 | 4.3–80.7 | 664 in one round (0.2%) |
| 75% | 16,585 | 3.94–4.86 | 5.27–6.56 | 7.07–8.52 | 10.4–31.6 | 0 |

Achieved rate within 0.02% of target in every non-shed round.

## Supportable Latency Claims (and their limits)

- On this box, single-document-insert service latency below saturation is
  **p50 ≈ 1.7–4.9 ms and p99 ≈ 3–8.5 ms across 25–75% of closed-loop
  capacity**, coordinated-omission-free.
- **The tail above p99 is burst/checkpoint-sensitive**: 3 of 9 rounds show
  p99.9 spikes (36 ms at 25%, 81 ms at 50%, 32 ms at 75%) an order of
  magnitude above their round's p99.
- **The admission gate (capacity 256) sheds arrival bursts intermittently at
  ≥50% load** — one 50% round shed 0.2% of arrivals; an earlier run shed at
  50% as well. Open-loop sustainable rate with zero shedding is therefore
  below 50% of closed-loop capacity on this box; with rare-shed tolerance it
  extends through 75%.
- These are single-insert figures on a 4-core box; they do not transfer to
  the M2-class closed-loop campaign numbers or to CRUD mixes.

This closes the campaign's standing "open-loop companion" prerequisite: the
closed-loop percentiles in prior reports remain queue latencies; SLA-grade
claims should cite this lane.

## SUC6.2 — Resource-Binding Candidate: Reject Without Re-Measurement

The plan row asks to measure "the 2.3%-of-guarded candidate" and implement
only if ≥3% safe end-to-end. The candidate is the `binding` component of the
SWT4 ablation, already attributed under the accepted D17 run: 0.108 ms =
2.3% of guarded time — the smallest of three components whose COMBINED
end-to-end projection (3.8% point / 2.6% conservative) already failed the
≥3% positive-lower-bound gate. Alone it is well under 1% end-to-end, below
what the CV≤10% paired protocol can resolve (the reason the bar is 3%).
Post-campaign main is faster still, shrinking the share further. A fresh
measurement cannot produce an admissible ≥3% result; decision recorded as
reject (ledger row U7), evidence = D17 attribution arithmetic.
