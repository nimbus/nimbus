# FU7 — SUC6.2 Literal Measurement (U7 Override Exercised)

Owner directed the literal "measure on current main" gate be run rather than
closed by attribution arithmetic. Method: the retained SWT4.1 attribution
instrumentation (`NIMBUS_SWO_ATTRIBUTION=1`, canonical 12 rounds), run on the
idle minicloud box (rustc 1.96.1) against current main `22c5cdd62`. Raw
report: `fu7-attribution-raw.md` (all 8 lanes CV ≤ 0.6% — well inside the
accepted-run discipline).

## Measured binding component on current main

| Quantity | Value |
| --- | ---: |
| guarded lane | 6.701 ms |
| guarded minus binding cleanup | 6.606 ms |
| **binding component** | **0.095 ms** |
| share of guarded | **1.42%** |
| production storage lane | 10.612 ms |
| **binding share of the storage lane** | **0.90%** |

(D17's M2-box measurement was 0.108 ms / 2.3% of guarded — same order,
consistent.)

## Verdict: REJECT, now by measurement

End-to-end impact is strictly smaller than the storage-lane share (the engine
adds non-storage work; retention < 1), so the candidate's ceiling on current
main is **< 0.9% end-to-end** — a fortiori below the ≥ 3% safe
positive-lower-bound bar, with no need for the engine-retention factor. The
resource-binding cleanup stays rejected. This measured verdict supersedes
U7's attribution-only rationale; the owner override is exercised and
resolved (recorded as U10 in the archived plan's decision log).
