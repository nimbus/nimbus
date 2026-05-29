# Node Compatibility Suite Status

Counts every vendored lane-local test-* JS/CJS/MJS fixture, then compares that denominator to the documented manifested passed subset plus explicit lane classification catalogs. Supported lanes use non-ignored Rust fixture evidence minus explicit expected-failure, known-gap, and skipped classifications as the passed numerator. Ignored watchpoints never count as passed. Expected failures, known gaps, and skipped/excluded entries are not pass claims; the remaining remainder is intentionally reported as unmanifested_or_unclassified, not as pass or fail.

## Lane Summary

| Lane | Role | Upstream | Vendored test files | Passed | Expected failure / known gap | Skipped / excluded | Classified total | Classified coverage count | Unclassified | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `legacy` | `v20.20.2` | 1308 | 902 | 401 | 5 | 406 | 1308 | 0 | 69.0% |
| `node22` | `supported` | `v22.22.3` | 4748 | 1000 | 3728 | 20 | 3748 | 4748 | 0 | 21.1% |
| `node24` | `default` | `v24.16.0` | 5198 | 1002 | 4149 | 47 | 4196 | 5198 | 0 | 19.3% |
| `node26` | `current` | `v26.2.0` | 5578 | 0 | 5529 | 49 | 5578 | 5578 | 0 | 0.0% |

## Lane Classification Catalogs

| Lane | Catalog | Expected failure / known gap | Skipped / excluded | Classified total | By expectation | By classification |
| --- | --- | ---: | ---: | ---: | --- | --- |
| `node20` | `tests/runtime/node/classifications/node20.json` | 401 | 5 | 406 | `{"Expected failure": 38, "Known gap": 363, "Skipped / excluded": 5}` | `{"Requires Native Addon Harness": 1, "Requires Pseudo Tty Host Harness": 11, "Requires Pummel Stress Harness": 12, "Requires Sequential Host State Harness": 13, "Requires Unpromoted Node Surface": 322, "Requires Wpt Harness": 2, "Rust Watchpoint Expected Failure": 38, "Support Fixture Not Top Level Test": 3, "Upstream Known Issue Or Platform Boundary": 2, "Vendored Non Official Placeholder": 2}` |
| `node22` | `tests/runtime/node/classifications/node22.json` | 3728 | 20 | 3748 | `{"Expected failure": 34, "Known gap": 3694, "Skipped / excluded": 20}` | `{"Requires Native Addon Harness": 29, "Requires Pseudo Tty Host Harness": 30, "Requires Pummel Stress Harness": 55, "Requires Sequential Host State Harness": 115, "Requires Unpromoted Node Surface": 3417, "Requires Wpt Harness": 22, "Rust Watchpoint Expected Failure": 34, "Support Fixture Not Top Level Test": 20, "Upstream Known Issue Or Platform Boundary": 26}` |
| `node24` | `tests/runtime/node/classifications/node24.json` | 4149 | 47 | 4196 | `{"Expected failure": 33, "Known gap": 4116, "Skipped / excluded": 47}` | `{"Requires Native Addon Harness": 31, "Requires Pseudo Tty Host Harness": 31, "Requires Pummel Stress Harness": 64, "Requires Sequential Host State Harness": 119, "Requires Unpromoted Node Surface": 3825, "Requires Wpt Harness": 23, "Rust Watchpoint Expected Failure": 33, "Support Fixture Not Top Level Test": 47, "Upstream Known Issue Or Platform Boundary": 23}` |
| `node26` | `tests/runtime/node/classifications/node26.json` | 5529 | 49 | 5578 | `{"Known gap": 5529, "Skipped / excluded": 49}` | `{"Requires Native Addon Harness": 33, "Requires Pseudo Tty Host Harness": 31, "Requires Pummel Stress Harness": 65, "Requires Sequential Host State Harness": 119, "Requires Unpromoted Node Surface": 5233, "Requires Wpt Harness": 25, "Support Fixture Not Top Level Test": 49, "Upstream Known Issue Or Platform Boundary": 23}` |

## Family Passed Denominator

| Family | node20 | node22 | node24 | node26 |
| --- | ---: | ---: | ---: | ---: |
| `core-semantics` | 115 | 121 | 123 | 0 |
| `loader-context` | 161 | 235 | 235 | 0 |
| `networking` | 260 | 270 | 270 | 0 |
| `process-and-timing` | 46 | 48 | 48 | 0 |
| `streams-and-local-io` | 311 | 317 | 315 | 0 |

## Rust Ignored Test Inventory

- ignored Rust node_compat tests: 67
- source: `crates/nimbus-runtime/src/runtime/tests/node/`

## Expectation Catalog

- catalog: `tests/runtime/node/expectations/rust-watchpoints.json`
- entries: 67
- by expectation: `{"Diagnostic expected failure": 1, "Expected failure": 66}`
- by classification: `{"Local Patch Regression": 1, "Watchpoint": 66}`
- unexpected passes: 0

## Warnings
- none
