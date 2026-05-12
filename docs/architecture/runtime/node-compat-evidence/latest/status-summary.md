# Node Compatibility Suite Status

Counts every vendored lane-local test-* JS/CJS/MJS fixture, then compares that denominator to the documented manifested passed subset plus explicit lane classification catalogs. Supported lanes use non-ignored Rust fixture evidence minus explicit expected-failure, known-gap, and skipped classifications as the passed numerator. Ignored watchpoints never count as passed. Expected failures, known gaps, and skipped/excluded entries are not pass claims; the remaining remainder is intentionally reported as unmanifested_or_unclassified, not as pass or fail.

## Lane Summary

| Lane | Role | Upstream | Vendored test files | Passed | Expected failure / known gap | Skipped / excluded | Classified total | Classified coverage count | Unclassified | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `supported` | `v20.20.2` | 1308 | 904 | 399 | 5 | 404 | 1308 | 0 | 69.1% |
| `node22` | `default` | `v22.15.0` | 1283 | 876 | 403 | 4 | 407 | 1283 | 0 | 68.3% |
| `node24` | `supported` | `v24.15.0` | 1495 | 925 | 567 | 3 | 570 | 1495 | 0 | 61.9% |

## Lane Classification Catalogs

| Lane | Catalog | Expected failure / known gap | Skipped / excluded | Classified total | By expectation | By classification |
| --- | --- | ---: | ---: | ---: | --- | --- |
| `node20` | `tests/runtime/node/classifications/node20.json` | 399 | 5 | 404 | `{"Expected failure": 36, "Known gap": 363, "Skipped / excluded": 5}` | `{"Requires Native Addon Harness": 1, "Requires Pseudo Tty Host Harness": 11, "Requires Pummel Stress Harness": 12, "Requires Sequential Host State Harness": 13, "Requires Unpromoted Node Surface": 322, "Requires Wpt Harness": 2, "Rust Watchpoint Expected Failure": 36, "Support Fixture Not Top Level Test": 3, "Upstream Known Issue Or Platform Boundary": 2, "Vendored Non Official Placeholder": 2}` |
| `node22` | `tests/runtime/node/classifications/node22.json` | 403 | 4 | 407 | `{"Expected failure": 25, "Known gap": 378, "Skipped / excluded": 4}` | `{"Requires Native Addon Harness": 1, "Requires Pseudo Tty Host Harness": 11, "Requires Pummel Stress Harness": 11, "Requires Sequential Host State Harness": 13, "Requires Unpromoted Node Surface": 338, "Requires Wpt Harness": 2, "Rust Watchpoint Expected Failure": 25, "Support Fixture Not Top Level Test": 3, "Upstream Known Issue Or Platform Boundary": 2, "Vendored Non Official Placeholder": 1}` |
| `node24` | `tests/runtime/node/classifications/node24.json` | 567 | 3 | 570 | `{"Expected failure": 29, "Known gap": 538, "Skipped / excluded": 3}` | `{"Requires Native Addon Harness": 1, "Requires Pseudo Tty Host Harness": 11, "Requires Pummel Stress Harness": 12, "Requires Sequential Host State Harness": 13, "Requires Unpromoted Node Surface": 491, "Requires Wpt Harness": 2, "Rust Watchpoint Expected Failure": 29, "Support Fixture Not Top Level Test": 3, "Upstream Known Issue Or Platform Boundary": 8}` |

## Family Passed Denominator

| Family | NLC | node20 | node22 | node24 |
| --- | --- | ---: | ---: | ---: |
| `core-semantics` | `NLC3` | 115 | 17 | 123 |
| `loader-context` | `NLC7` | 162 | 188 | 164 |
| `networking` | `NLC6` | 260 | 270 | 265 |
| `process-and-timing` | `NLC4` | 46 | 48 | 48 |
| `streams-and-local-io` | `NLC5` | 311 | 317 | 315 |

## Rust Ignored Test Inventory

- ignored Rust node_compat tests: 61
- source: `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`

## Expectation Catalog

- catalog: `tests/runtime/node/expectations/rust-watchpoints.json`
- entries: 61
- by expectation: `{"Diagnostic expected failure": 1, "Expected failure": 60}`
- by classification: `{"Local Patch Regression": 1, "Watchpoint": 60}`
- unexpected passes: 0

## Warnings
- none
