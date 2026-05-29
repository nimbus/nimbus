# Node20 Runtime Evidence

This page is generated from the checked-in Node compatibility evidence snapshots.

## Summary

- role: `Legacy grace; EOL`
- support phase: `EOL legacy`
- product default: `no`
- evidence policy: `legacy-grace regression only`
- upstream fixture line: `v20.20.2`
- runtime execution target: `Node20`
- vendored official fixtures: `1308`
- passed official fixtures: `902`
- expected failure / known gap fixtures: `401`
- skipped / excluded fixtures: `5`
- unclassified fixtures: `0`
- official fixture pass rate: `69.0%`
- classified coverage: `100.0%`

## Classification Catalog

- catalog: `tests/runtime/node/classifications/node20.json`

| Expectation | Count |
| --- | ---: |
| Expected failure | 38 |
| Known gap | 363 |
| Skipped / excluded | 5 |

## Canary Coverage

| Package | Preset | Pinned version | Evidence | Support boundary | Status |
| --- | --- | --- | --- | --- | --- |
| `express` | Application | `4.19.2` | Support | Supported | Passed |
| `fastify` | Application | `4.28.1` | Support | Supported | Passed |

## Claim Boundary

This lane remains selectable as legacy-grace regression coverage, but it
is not an active enterprise LTS support target after Node20 EOL on
2026-04-30.
Known gaps and expected failures are intentionally not support claims.
