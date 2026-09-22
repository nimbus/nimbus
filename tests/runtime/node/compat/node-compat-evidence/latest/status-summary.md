# Node Compatibility Suite Status

Counts every official vendored lane-local test-* JS/CJS/MJS fixture, then compares that denominator to the documented manifested passed subset plus explicit lane classification catalogs. Supported lanes use non-ignored Rust tests that execute node-compat fixtures for the matching lane minus explicit expected-failure, known-gap, and skipped classifications as the passed numerator. Ignored watchpoints never count as passed. Expected failures, known gaps, and skipped/excluded entries are not pass claims; the remaining remainder is intentionally reported as unmanifested_or_unclassified, not as pass or fail. Supplementary, regression, canary, watchpoint, and diagnostic evidence is reported in separate evidence tiers and never changes official pass denominators.

## Lane Summary

| Lane | Role | Upstream | Vendored test files | Passed | Expected failure / known gap | Skipped / excluded | Classified total | Classified coverage count | Unclassified | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `legacy` | `v20.20.2` | 4248 | 938 | 3297 | 13 | 3310 | 4248 | 0 | 22.1% |
| `node22` | `supported` | `v22.23.2` | 4762 | 2364 | 2378 | 20 | 2398 | 4762 | 0 | 49.6% |
| `node24` | `default` | `v24.21.0` | 5729 | 2399 | 3281 | 49 | 3330 | 5729 | 0 | 41.9% |
| `node26` | `current` | `v26.10.0` | 6201 | 2093 | 4041 | 67 | 4108 | 6201 | 0 | 33.8% |

## Evidence Tiers

| Tier | Source | Primary count | Passed | Claims | Official denominator? | Notes |
| --- | --- | ---: | ---: | ---: | --- | --- |
| `official` | `vendored_official_fixture_corpus` | 20940 fixture_count | 7794 | - | yes | Byte-identical Node upstream test-* fixtures under lane-local nodeNN/test roots; pass percentages use only this denominator. |
| `supplementary` | `node_compat_manifest_test_tier` | 7 fixture_count | - | - | no | Nimbus-authored support fixtures that explain behavior beyond official Node corpus pass claims. |
| `regression` | `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/regression` | 30 fixture_count | - | - | no | Nimbus-authored or adapted regression fixtures separated from official lane roots. |
| `canary` | `tests/runtime/node/canary-registry.json` | 37 active_canary_count | - | 79 | no | Package and app probes that support developer-facing claims without changing official fixture denominators. |
| `watchpoint` | `tests/runtime/node/expectations/rust-watchpoints.json` | 129 catalog_entry_count | - | - | no | Ignored Rust watchpoints and expectation catalog entries used to preserve known failures and unexpected-pass diagnostics. |
| `diagnostic` | `tests/runtime/node/expectations/rust-watchpoints.json + tests/runtime/node/canary-registry.json` | 11 diagnostic_count | - | 11 | no | Expected-denial or host-owned evidence; these are explicit boundaries, not compatibility passes. |

## Lane Classification Catalogs

| Lane | Catalog | Expected failure / known gap | Skipped / excluded | Classified total | By expectation | By classification |
| --- | --- | ---: | ---: | ---: | --- | --- |
| `node20` | `tests/runtime/node/classifications/node20.json` | 3297 | 13 | 3310 | `{"Expected failure": 2, "Known gap": 3295, "Skipped / excluded": 13}` | `{"Requires Native Addon Harness": 24, "Requires Pseudo Tty Host Harness": 28, "Requires Pummel Stress Harness": 60, "Requires Sequential Host State Harness": 119, "Requires Unpromoted Node Surface": 3024, "Requires Wpt Harness": 20, "Rust Watchpoint Expected Failure": 2, "Support Fixture Not Top Level Test": 13, "Upstream Known Issue Or Platform Boundary": 20}` |
| `node22` | `tests/runtime/node/classifications/node22.json` | 2378 | 20 | 2398 | `{"Expected failure": 20, "Known gap": 2358, "Skipped / excluded": 20}` | `{"Requires Native Addon Harness": 29, "Requires Pseudo Tty Host Harness": 31, "Requires Pummel Stress Harness": 55, "Requires Sequential Host State Harness": 115, "Requires Unpromoted Node Surface": 2080, "Requires Wpt Harness": 22, "Rust Watchpoint Expected Failure": 20, "Support Fixture Not Top Level Test": 20, "Upstream Known Issue Or Platform Boundary": 26}` |
| `node24` | `tests/runtime/node/classifications/node24.json` | 3281 | 49 | 3330 | `{"Expected failure": 3, "Known gap": 3278, "Skipped / excluded": 49}` | `{"Requires Native Addon Harness": 33, "Requires Pseudo Tty Host Harness": 33, "Requires Pummel Stress Harness": 65, "Requires Sequential Host State Harness": 120, "Requires Unpromoted Node Surface": 2981, "Requires Wpt Harness": 23, "Rust Watchpoint Expected Failure": 3, "Support Fixture Not Top Level Test": 48, "Upstream Known Issue Or Platform Boundary": 24}` |
| `node26` | `tests/runtime/node/classifications/node26.json` | 4041 | 67 | 4108 | `{"Known gap": 4041, "Skipped / excluded": 67}` | `{"Requires Native Addon Harness": 41, "Requires Pseudo Tty Host Harness": 33, "Requires Pummel Stress Harness": 69, "Requires Sequential Host State Harness": 123, "Requires Unpromoted Node Surface": 3730, "Requires Wpt Harness": 26, "Support Fixture Not Top Level Test": 59, "Upstream Known Issue Or Platform Boundary": 27}` |

## Family Passed Denominator

| Family | node20 | node22 | node24 | node26 |
| --- | ---: | ---: | ---: | ---: |
| `core-semantics` | 115 | 121 | 122 | 118 |
| `loader-context` | 173 | 232 | 180 | 174 |
| `networking` | 264 | 270 | 268 | 264 |
| `process-and-timing` | 46 | 48 | 48 | 46 |
| `streams-and-local-io` | 312 | 317 | 306 | 290 |

## Rust Ignored Test Inventory

- ignored Rust node_compat tests: 129
- source: `crates/nimbus-runtime/src/runtime/tests/node/`

## Expectation Catalog

- catalog: `tests/runtime/node/expectations/rust-watchpoints.json`
- entries: 129
- by expectation: `{"Expected failure": 129}`
- by classification: `{"Watchpoint": 129}`
- unexpected passes: 0

## Warnings
- none
