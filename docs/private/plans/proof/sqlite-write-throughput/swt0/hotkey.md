# SWT0.2 Accepted Hot-Key Baseline

Date: 2026-07-28. Binary `9e378942…d6f72` at `B_ref` `2a1853dab`; canonical
hot-key protocol (same ladder/rounds; every worker updates one shared
document); report `hotkey-raw-accepted.md`
(SHA-256 `7544a346d7ecc6035c09cf94a8a322860e5fb6febd17963d894294dcaf402fb7`).

Verdict: **accepted** — CVs 3.3 / 1.3 / 1.1%. This is the campaign's first
accepted hot-key baseline; the N=256 rung saturates the 128-slot committer
inbox by design and the SWT0.1-fixed harness charges that backpressure to
client latency instead of panicking.

| N | Mean mut/s | 95% CI | Median | CV | p50/p95/p99 µs |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | 559 | 549–570 | 562 | 3.3% | 1,773.0 / 1,972.1 / 2,086.0 |
| 32 | **3,150** | 3,128–3,173 | 3,169 | 1.3% | 10,080.8 / 10,838.1 / 11,457.1 |
| 256 | 2,639 | 2,623–2,655 | 2,631 | 1.1% | 60,260.5 / 234,241.8 / 360,181.3 |

The cross-cutting regression gate uses hot-key N=32 (no >5% regression).
