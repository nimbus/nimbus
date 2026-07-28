# Rejected Layered Diagnostic

Date: 2026-07-27

Purpose: confirm runtime SQLite/SQLCipher identity after the benchmark gained
identity reporting.

Verdict: **not accepted as a throughput baseline**. The resident-current
control had CV 10.3%, above the plan's 10% limit. No samples from this run were
combined with the accepted baseline.

Runtime:

- SQLite 3.51.3;
- SQLCipher 4.14.0 community;
- source id
  `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`.

Report SHA-256:
`83aa26c5665c9fc7180d6c450ed82598c636973b63aa543140a11d147e96b45c`.
The byte-for-byte raw report is retained as
`layered-noisy-diagnostic-raw.md`; its SHA-256 matches this value.

| Lane | Mean logical mut/s | 95% CI | CV |
| --- | ---: | ---: | ---: |
| Raw | 297,568 | 288,737–306,399 | 4.7% |
| Current-loop resident | 47,149 | 44,065–50,232 | **10.3%** |
| Guarded prepared/hoisted | 152,785 | 150,040–155,530 | 2.8% |
| Nimbus-shaped lower bound | 172,792 | 170,747–174,838 | 1.9% |
| Production storage | 38,821 | 38,241–39,402 | 2.4% |

The production-storage result agrees closely with the 38,810 planning
reference, but the entire run remains diagnostic because acceptance is per
complete sample set, not cherry-picked per lane.
