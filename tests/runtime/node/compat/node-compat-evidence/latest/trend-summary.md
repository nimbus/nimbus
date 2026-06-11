# Node Compatibility Trend Snapshot

- baseline available: `true`
- current status: `target/node-compat/status/status-summary.json`
- current dashboard: `target/node-compat/dashboard/dashboard-summary.json`

## Lane Trends

| Lane | Upstream | Passed | Passed Delta | Pass Rate | Pass Rate Delta Points | Unclassified | Unclassified Delta |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 916 | +14 | 21.6% | -47.4 | 0 | +0 |
| `node22` | `v22.22.3` | 2297 | +1297 | 48.4% | +27.3 | 0 | +0 |
| `node24` | `v24.16.0` | 2327 | +1325 | 44.8% | +25.5 | 0 | +0 |
| `node26` | `v26.2.0` | 1008 | +1008 | 18.1% | +18.1 | 0 | +0 |

## Evidence Trends

| Metric | Current | Baseline | Delta |
| --- | ---: | ---: | ---: |
| `canary_report_count` | 0 | 2 | -2 |
| `expectation_catalog_entry_count` | 135 | 67 | +68 |
| `oracle_report_count` | 0 | 2 | -2 |
| `required_canary_gap_count` | 0 | 0 | +0 |
| `rust_ignore_count` | 135 | 67 | +68 |
| `slice_report_count` | 0 | 8 | -8 |
| `unexpected_pass_count` | 0 | 0 | +0 |
| `warning_count` | 0 | 0 | +0 |
