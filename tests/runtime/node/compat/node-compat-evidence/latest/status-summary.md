# Node Compatibility Suite Status

Counts every official vendored lane-local test-* JS/CJS/MJS fixture, then compares that denominator to the documented manifested passed subset plus explicit lane classification catalogs. Supported lanes use non-ignored Rust tests that execute node-compat fixtures for the matching lane minus explicit expected-failure, known-gap, and skipped classifications as the passed numerator. Ignored watchpoints never count as passed. Expected failures, known gaps, and skipped/excluded entries are not pass claims; the remaining remainder is intentionally reported as unmanifested_or_unclassified, not as pass or fail. Supplementary, regression, canary, watchpoint, and diagnostic evidence is reported in separate evidence tiers and never changes official pass denominators.

## Lane Summary

| Lane | Role | Upstream | Vendored test files | Passed | Expected failure / known gap | Skipped / excluded | Classified total | Classified coverage count | Unclassified | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `legacy` | `v20.20.2` | 4248 | 919 | 3316 | 13 | 3329 | 4248 | 0 | 21.6% |
| `node22` | `supported` | `v22.23.2` | 4762 | 2362 | 2380 | 20 | 2400 | 4762 | 0 | 49.6% |
| `node24` | `default` | `v24.20.0` | 5671 | 2397 | 3226 | 48 | 3274 | 5671 | 0 | 42.3% |
| `node26` | `current` | `v26.8.1` | 5940 | 2090 | 3795 | 55 | 3850 | 5940 | 0 | 35.2% |

## Evidence Tiers

| Tier | Source | Primary count | Passed | Claims | Official denominator? | Notes |
| --- | --- | ---: | ---: | ---: | --- | --- |
| `official` | `vendored_official_fixture_corpus` | 20621 fixture_count | 7768 | - | yes | Byte-identical Node upstream test-* fixtures under lane-local nodeNN/test roots; pass percentages use only this denominator. |
| `supplementary` | `node_compat_manifest_test_tier` | 7 fixture_count | - | - | no | Nimbus-authored support fixtures that explain behavior beyond official Node corpus pass claims. |
| `regression` | `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/regression` | 26 fixture_count | - | - | no | Nimbus-authored or adapted regression fixtures separated from official lane roots. |
| `canary` | `tests/runtime/node/canary-registry.json` | 37 active_canary_count | - | 79 | no | Package and app probes that support developer-facing claims without changing official fixture denominators. |
| `watchpoint` | `tests/runtime/node/expectations/rust-watchpoints.json` | 150 catalog_entry_count | - | - | no | Ignored Rust watchpoints and expectation catalog entries used to preserve known failures and unexpected-pass diagnostics. |
| `diagnostic` | `tests/runtime/node/expectations/rust-watchpoints.json + tests/runtime/node/canary-registry.json` | 11 diagnostic_count | - | 11 | no | Expected-denial or host-owned evidence; these are explicit boundaries, not compatibility passes. |

## Lane Classification Catalogs

| Lane | Catalog | Expected failure / known gap | Skipped / excluded | Classified total | By expectation | By classification |
| --- | --- | ---: | ---: | ---: | --- | --- |
| `node20` | `tests/runtime/node/classifications/node20.json` | 3316 | 13 | 3329 | `{"Expected failure": 20, "Known gap": 3296, "Skipped / excluded": 13}` | `{"Requires Native Addon Harness": 24, "Requires Pseudo Tty Host Harness": 28, "Requires Pummel Stress Harness": 60, "Requires Sequential Host State Harness": 119, "Requires Unpromoted Node Surface": 3025, "Requires Wpt Harness": 20, "Rust Watchpoint Expected Failure": 20, "Support Fixture Not Top Level Test": 13, "Upstream Known Issue Or Platform Boundary": 20}` |
| `node22` | `tests/runtime/node/classifications/node22.json` | 2380 | 20 | 2400 | `{"Expected failure": 21, "Known gap": 2359, "Skipped / excluded": 20}` | `{"Requires Native Addon Harness": 29, "Requires Pseudo Tty Host Harness": 31, "Requires Pummel Stress Harness": 55, "Requires Sequential Host State Harness": 115, "Requires Unpromoted Node Surface": 2081, "Requires Wpt Harness": 22, "Rust Watchpoint Expected Failure": 21, "Support Fixture Not Top Level Test": 20, "Upstream Known Issue Or Platform Boundary": 26}` |
| `node24` | `tests/runtime/node/classifications/node24.json` | 3226 | 48 | 3274 | `{"Expected failure": 4, "Known gap": 3222, "Skipped / excluded": 48}` | `{"Requires Native Addon Harness": 33, "Requires Pseudo Tty Host Harness": 32, "Requires Pummel Stress Harness": 65, "Requires Sequential Host State Harness": 119, "Requires Unpromoted Node Surface": 2927, "Requires Wpt Harness": 23, "Rust Watchpoint Expected Failure": 4, "Support Fixture Not Top Level Test": 47, "Upstream Known Issue Or Platform Boundary": 24}` |
| `node26` | `tests/runtime/node/classifications/node26.json` | 3795 | 55 | 3850 | `{"Known gap": 3795, "Skipped / excluded": 55}` | `{"Requires Native Addon Harness": 40, "Requires Pseudo Tty Host Harness": 33, "Requires Pummel Stress Harness": 67, "Requires Sequential Host State Harness": 120, "Requires Unpromoted Node Surface": 3488, "Requires Wpt Harness": 25, "Support Fixture Not Top Level Test": 47, "Upstream Known Issue Or Platform Boundary": 30}` |

## Family Passed Denominator

| Family | node20 | node22 | node24 | node26 |
| --- | ---: | ---: | ---: | ---: |
| `core-semantics` | 115 | 121 | 122 | 118 |
| `loader-context` | 173 | 232 | 180 | 174 |
| `networking` | 264 | 270 | 268 | 264 |
| `process-and-timing` | 46 | 48 | 48 | 46 |
| `streams-and-local-io` | 311 | 317 | 306 | 290 |

## Rust Ignored Test Inventory

- ignored Rust node_compat tests: 150
- source: `crates/nimbus-runtime/src/runtime/tests/node/`

## Expectation Catalog

- catalog: `tests/runtime/node/expectations/rust-watchpoints.json`
- entries: 150
- by expectation: `{"Expected failure": 150}`
- by classification: `{"Watchpoint": 150}`
- unexpected passes: 0

## Warnings
- none
