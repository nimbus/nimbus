# SWT0.2 Accepted Full-Engine CRUD Baseline

Date: 2026-07-28. Binary `9e378942…d6f72` at `B_ref` `2a1853dab`; canonical
protocol (N=1/32/256, 100 ops/worker, 9,000 cap, 15 measured rounds, 3
warmups, split phases); report `crud-raw-accepted.md`
(SHA-256 `e3bf3f98b4a1737e82f567a1a3f356f94675a52e9ed2f514208419c4e2182085`).

Verdict: **accepted** — CVs 1.7 / 1.5 / 4.3%.

| N | Mean mut/s | 95% CI | Median | CV | p50/p95/p99 µs | Avg batch |
| ---: | ---: | ---: | ---: | ---: | --- | ---: |
| 1 | 2,018 | 1,999–2,037 | 2,022 | 1.7% | 485.2 / 552.0 / 690.9 | 1.00 |
| 32 | 16,135 | 15,997–16,273 | 16,202 | 1.5% | 1,903.0 / 2,356.4 / 4,059.9 | 16.01 |
| 256 | **27,165** | 26,523–27,807 | 27,320 | 4.3% | 8,825.7 / 14,358.3 / 18,975.3 | 129.70 |

N=256 phase split: plan-CPU 20.7%, conflict 3.6%, apply+publish 57.4%,
first append 18.3%.

This reference run is 26.7% above the historical 21,433 observation on
identical production source, confirming the host-drift finding that motivated
the paired `F_ref`/`B_ref` acceptance design (D8). The final target remains
the contemporaneous paired ratio ≥1.40 plus the 30k/28k absolute floors.
