# SWT0.2 Accepted Layered Baseline

Date: 2026-07-28. Binary `1e7320c0…cf501` at `B_ref` `2a1853dab`; canonical
12 rounds x 60 repetitions; report `layered-raw-accepted.md`
(SHA-256 `065ffc11a53c4e17a83e461887714ec7cc86366f548b75142c759448b20ee890`).

Verdict: **accepted** — every lane CV at or below 10%.

| Lane | Logical mut/s | 95% CI | CV | Mean elapsed |
| --- | ---: | ---: | ---: | ---: |
| Raw row mutation | 404,859 | 390,916–418,802 | 5.4% | 1.903 ms |
| Current-loop SQL, resident connection | 54,169 | 53,785–54,552 | 1.1% | 14.180 ms |
| Guarded prepared/hoisted SQL | 169,972 | 168,498–171,447 | 1.4% | 4.519 ms |
| Nimbus-shaped SQL lower bound | 185,940 | 182,725–189,156 | 2.7% | 4.133 ms |
| Production storage append+apply | 42,806 | 42,482–43,129 | 1.2% | 17.944 ms |

Raw samples, byte shape, statement models, CPU serialization, and connection
costs are in the retained raw report. Quiet-host observations versus the
CTRL0 planning reference (which remains ranking-only evidence):

- production storage 42,806 vs 38,810 (+10.3%); guarded 169,972 vs 151,485
  (+12.2%); raw 404,859 vs 305,895 (+32.4%) — host state moves the raw
  device lane hardest;
- the guarded-to-lower-bound (forward-apply) delta is 0.386 ms = **8.5%** of
  guarded elapsed here, inside the documented 7–11.5% cross-run range (D11);
- WAL bytes/frames are bit-identical to CTRL0 on all Nimbus-shaped lanes.
