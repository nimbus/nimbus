# Node.js Runtime Compatibility

Nimbus's Node.js runtime compatibility is evidence-backed and deliberately
bounded. A surface is considered supported only when it has checked-in fixture,
canary, oracle, or classification evidence.

## Current Version Support

| Node target | Product role | Upstream fixture line | Official fixture pass rate | Classified coverage |
| --- | --- | --- | ---: | ---: |
| Node20 | Supported selectable target | `v20.20.2` | 70.1% | 100.0% |
| Node22 | Default selectable target | `v22.15.0` | 68.5% | 100.0% |
| Node24 | Supported selectable target | `v24.15.0` | 61.9% | 100.0% |

These percentages come from the current checked-in evidence snapshot. They
should be regenerated through the Node compatibility workflow after any fixture,
classification, or runtime behavior change.

## Status Vocabulary

| Label | Meaning |
| --- | --- |
| Passed | Fixture or canary is a measured pass and may support a claim |
| Expected failure / known gap | Fixture is intentionally classified and is not a support claim |
| Skipped / excluded | Fixture is outside the current lane denominator for a documented reason |
| Unclassified | Fixture is not yet a pass claim and not yet classified |
| Classified coverage | Passed plus explicitly classified fixtures divided by the vendored official fixture denominator |

Avoid reading an expected failure, known gap, skip, or classified fixture as a
pass claim.

## Current Public Contract

- Node22 is the default compatibility target.
- Node20 and Node24 are supported selectable targets.
- Node target selection does not grant ambient host access. Runtime permission
  mode and explicit grants remain separate from Node compatibility target.
- Convex-compatible `"use node"` action modules can select Node20, Node22, or
  Node24 through `convex.json`.
- Nimbus does not currently claim full Node built-in compatibility for any
  target.
- Runtime support is narrower than Node CLI parity; `node --test`, inspector,
  worker, child process, native addon, and host-heavy behavior is only
  supported where explicitly documented by fixture or canary evidence.

## Evidence Sources

Current engineering evidence:

- `docs/architecture/runtime/node-compat-evidence/latest/status-summary.md`
- `docs/architecture/runtime/node-compat-evidence/latest/dashboard-summary.md`
- `docs/architecture/runtime/node-compat-evidence/latest/trend-summary.md`
- `docs/architecture/runtime/node-compat-surface-matrix.md`

Generated public evidence pages live under `docs/runtimes/nodejs/evidence/` so
developers do not need to read architecture internals for routine support
questions. The current aggregate generated page is
`docs/runtimes/nodejs/evidence/latest.md`, and the maintainer refresh workflow
is `docs/runtimes/nodejs/evidence/refreshing.md`.
