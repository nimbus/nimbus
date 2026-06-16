# Node26 Runtime Evidence

This page is generated from the checked-in Node compatibility evidence snapshots.

## Summary

- role: `Current`
- support phase: `Current non-LTS`
- product default: `no`
- evidence policy: `Current non-LTS; promote to LTS support only after LTS and lane-local evidence`
- upstream fixture line: `v26.2.0`
- runtime execution target: `Node26`
- vendored official fixtures: `5578`
- passed official fixtures: `2092`
- expected failure / known gap fixtures: `3432`
- skipped / excluded fixtures: `54`
- unclassified fixtures: `0`
- official fixture pass rate: `37.5%`
- classified coverage: `100.0%`

## Classification Catalog

- catalog: `tests/runtime/node/classifications/node26.json`

| Expectation | Count |
| --- | ---: |
| Known gap | 3432 |
| Skipped / excluded | 54 |

## Canary Coverage

| Package | Preset | Pinned version | Evidence | Support boundary | Status |
| --- | --- | --- | --- | --- | --- |
| none in current snapshot | n/a | n/a | n/a | n/a | n/a |

## Claim Boundary

This lane is supported only for the measured surfaces represented by its
passed fixtures, canaries, and explicit classifications.
Known gaps and expected failures are intentionally not support claims.
