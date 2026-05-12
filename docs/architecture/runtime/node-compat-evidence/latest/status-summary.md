# Node Compatibility Suite Status

Counts every vendored lane-local test-* JS/CJS/MJS fixture, then compares that denominator to the documented manifested green subset plus explicit lane classification catalogs. Classified non-green entries are not pass claims; the remaining remainder is intentionally reported as unmanifested_or_unclassified, not as pass or fail.

## Lane Summary

| Lane | Role | Upstream | Vendored test files | Documented green | Classified non-green | Unmanifested/unclassified | Ratio |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| `node20` | `validation` | `v20.20.2` | 1308 | 913 | 0 | 395 | 69.8% |
| `node22` | `primary` | `v22.15.0` | 1283 | 994 | 20 | 269 | 77.5% |
| `node24` | `preview` | `v24.15.0` | 1495 | 925 | 0 | 570 | 61.9% |

## Lane Classification Catalogs

| Lane | Catalog | Classified non-green | By expectation | By classification |
| --- | --- | ---: | --- | --- |
| `node20` | `tests/node-compat/classifications/node20.json` | 0 | `{}` | `{}` |
| `node22` | `tests/node-compat/classifications/node22.json` | 20 | `{"expected_gap": 13, "expected_skip": 7}` | `{"requires_host_process_abort_harness": 1, "requires_native_addon_non_node_context_harness": 1, "requires_pseudo_tty_host_harness": 11, "support_fixture_not_top_level_test": 3, "vendored_non_official_placeholder": 4}` |
| `node24` | `tests/node-compat/classifications/node24.json` | 0 | `{}` | `{}` |

## Family Green Denominator

| Family | NLC | node20 | node22 | node24 |
| --- | --- | ---: | ---: | ---: |
| `core-semantics` | `NLC3` | 116 | 120 | 122 |
| `loader-context` | `NLC7` | 175 | 239 | 179 |
| `networking` | `NLC6` | 265 | 270 | 268 |
| `process-and-timing` | `NLC4` | 46 | 48 | 48 |
| `streams-and-local-io` | `NLC5` | 311 | 317 | 308 |

## Rust Ignored Test Inventory

- ignored Rust node_compat tests: 61
- source: `crates/neovex-runtime/src/runtime/tests/node_compat.rs`

## Expectation Catalog

- catalog: `tests/node-compat/expectations/rust-watchpoints.json`
- entries: 61
- by expectation: `{"diagnostic_expected_failure": 1, "expected_failure": 55, "expected_skip": 5}`
- by classification: `{"local_patch_regression": 1, "preview_lane_gate": 5, "watchpoint": 55}`
- unexpected passes: 0

## Warnings
- none
