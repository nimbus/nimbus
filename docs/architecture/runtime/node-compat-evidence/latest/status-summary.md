# Node Compatibility Suite Status

Counts every vendored lane-local test-* JS/CJS/MJS fixture, then compares that denominator to the documented manifested passed subset plus explicit lane classification catalogs. Supported lanes use non-ignored Rust tests that execute node-compat fixtures for the matching lane minus explicit expected-failure, known-gap, and skipped classifications as the passed numerator. Ignored watchpoints never count as passed. Expected failures, known gaps, and skipped/excluded entries are not pass claims; the remaining remainder is intentionally reported as unmanifested_or_unclassified, not as pass or fail.

## Lane Summary

| Lane | Role | Upstream | Vendored test files | Passed | Expected failure / known gap | Skipped / excluded | Classified total | Classified coverage count | Unclassified | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `legacy` | `v20.20.2` | 1308 | 899 | 403 | 6 | 409 | 1308 | 0 | 68.7% |
| `node22` | `supported` | `v22.22.3` | 4773 | 1024 | 3729 | 20 | 3749 | 4773 | 0 | 21.5% |
| `node24` | `default` | `v24.16.0` | 5198 | 276 | 4872 | 50 | 4922 | 5198 | 0 | 5.3% |
| `node26` | `current` | `v26.2.0` | 5578 | 0 | 5529 | 49 | 5578 | 5578 | 0 | 0.0% |

## Lane Classification Catalogs

| Lane | Catalog | Expected failure / known gap | Skipped / excluded | Classified total | By expectation | By classification |
| --- | --- | ---: | ---: | ---: | --- | --- |
| `node20` | `tests/runtime/node/classifications/node20.json` | 403 | 6 | 409 | `{"Expected failure": 20, "Known gap": 383, "Skipped / excluded": 6}` | `{"Requires Native Addon Harness": 1, "Requires Pseudo Tty Host Harness": 11, "Requires Pummel Stress Harness": 12, "Requires Sequential Host State Harness": 13, "Requires Unpromoted Node Surface": 342, "Requires Wpt Harness": 2, "Rust Watchpoint Expected Failure": 20, "Support Fixture Not Top Level Test": 3, "Upstream Known Issue Or Platform Boundary": 2, "Vendored Non Official Placeholder": 3}` |
| `node22` | `tests/runtime/node/classifications/node22.json` | 3729 | 20 | 3749 | `{"Expected failure": 33, "Known gap": 3696, "Skipped / excluded": 20}` | `{"Requires Native Addon Harness": 29, "Requires Pseudo Tty Host Harness": 30, "Requires Pummel Stress Harness": 55, "Requires Sequential Host State Harness": 115, "Requires Unpromoted Node Surface": 3419, "Requires Wpt Harness": 22, "Rust Watchpoint Expected Failure": 33, "Support Fixture Not Top Level Test": 20, "Upstream Known Issue Or Platform Boundary": 26}` |
| `node24` | `tests/runtime/node/classifications/node24.json` | 4872 | 50 | 4922 | `{"Expected failure": 23, "Known gap": 4849, "Skipped / excluded": 50}` | `{"Requires Native Addon Harness": 31, "Requires Pseudo Tty Host Harness": 31, "Requires Pummel Stress Harness": 64, "Requires Sequential Host State Harness": 119, "Requires Unpromoted Node Surface": 4558, "Requires Wpt Harness": 23, "Rust Watchpoint Expected Failure": 23, "Support Fixture Not Top Level Test": 49, "Upstream Known Issue Or Platform Boundary": 24}` |
| `node26` | `tests/runtime/node/classifications/node26.json` | 5529 | 49 | 5578 | `{"Known gap": 5529, "Skipped / excluded": 49}` | `{"Requires Native Addon Harness": 33, "Requires Pseudo Tty Host Harness": 31, "Requires Pummel Stress Harness": 65, "Requires Sequential Host State Harness": 119, "Requires Unpromoted Node Surface": 5233, "Requires Wpt Harness": 25, "Support Fixture Not Top Level Test": 49, "Upstream Known Issue Or Platform Boundary": 23}` |

## Family Passed Denominator

| Family | node20 | node22 | node24 | node26 |
| --- | ---: | ---: | ---: | ---: |
| `core-semantics` | 115 | 121 | 122 | 0 |
| `loader-context` | 160 | 234 | 139 | 0 |
| `networking` | 260 | 270 | 1 | 0 |
| `process-and-timing` | 46 | 48 | 1 | 0 |
| `streams-and-local-io` | 311 | 317 | 6 | 0 |

## Rust Ignored Test Inventory

- ignored Rust node_compat tests: 78
- source: `crates/nimbus-runtime/src/runtime/tests/node/`

## Expectation Catalog

- catalog: `tests/runtime/node/expectations/rust-watchpoints.json`
- entries: 78
- by expectation: `{"Expected failure": 78}`
- by classification: `{"Watchpoint": 78}`
- unexpected passes: 0

## Warnings
- none
