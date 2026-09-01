# Node26 Runtime Evidence

This page is generated from the checked-in Node compatibility evidence snapshots.

## Summary

- role: `Current`
- support phase: `Current non-LTS`
- product default: `no`
- evidence policy: `Current non-LTS; promote to LTS support only after LTS and lane-local evidence`
- upstream fixture line: `v26.8.1`
- runtime execution target: `Node26`
- vendored official fixtures: `5940`
- passed official fixtures: `2090`
- expected failure / known gap fixtures: `3795`
- skipped / excluded fixtures: `55`
- unclassified fixtures: `0`
- official fixture pass rate: `35.2%`
- classified coverage: `100.0%`

## Classification Catalog

- catalog: `tests/runtime/node/classifications/node26.json`

| Expectation | Count |
| --- | ---: |
| Known gap | 3795 |
| Skipped / excluded | 55 |

## Canary Coverage

| Package | Preset | Pinned version | Evidence | Support boundary | Status |
| --- | --- | --- | --- | --- | --- |
| none in current snapshot | n/a | n/a | n/a | n/a | n/a |

## Claim Boundary

This lane is supported only for the measured surfaces represented by its
passed fixtures, canaries, and explicit classifications.
Known gaps and expected failures are intentionally not support claims.
