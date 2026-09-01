# Node Compatibility Trend Snapshot

- baseline available: `true`
- current status: `target/node-compat/status/status-summary.json`
- current dashboard: `target/node-compat/dashboard/dashboard-summary.json`

## Lane Trends

| Lane | Upstream | Passed | Passed Delta | Pass Rate | Pass Rate Delta Points | Unclassified | Unclassified Delta |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 919 | +0 | 21.6% | +0.0 | 0 | +0 |
| `node22` | `v22.23.2` | 2362 | -1 | 49.6% | -0.2 | 0 | +0 |
| `node24` | `v24.20.0` | 2397 | -3 | 42.3% | -3.9 | 0 | +0 |
| `node26` | `v26.8.1` | 2090 | -2 | 35.2% | -2.3 | 0 | +0 |

## Evidence Trends

| Metric | Current | Baseline | Delta |
| --- | ---: | ---: | ---: |
| `canary_report_count` | 0 | 2 | -2 |
| `expectation_catalog_entry_count` | 150 | 150 | +0 |
| `oracle_report_count` | 0 | 4 | -4 |
| `required_canary_gap_count` | 0 | 0 | +0 |
| `rust_ignore_count` | 150 | 150 | +0 |
| `slice_report_count` | 5 | 0 | +5 |
| `unexpected_pass_count` | 0 | 0 | +0 |
| `warning_count` | 0 | 0 | +0 |
