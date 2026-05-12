# Node Compatibility Dashboard

- Slice reports: 8
- Canary reports: 2
- Oracle reports: 1
- Inventory reports: 1

## Suite Status
- source: `target/node-compat/status/status-summary.json`
- rust ignored tests: `61`

| Lane | Upstream | Role | Green | Vendored | Unclassified | Ratio |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | `validation` | 913 | 1308 | 395 | 69.8% |
| `node22` | `v22.15.0` | `primary` | 994 | 1283 | 209 | 77.5% |
| `node24` | `v24.15.0` | `preview` | 925 | 1495 | 570 | 61.9% |

### Suite Warnings
- none

## Fixture Inventory

| Lane | Upstream | Vendored | Documented green | Classified non-green | Status unclassified | Rust-referenced | Rust-unreferenced | Reconstructability gap | Warnings |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node22` | `v22.15.0` | 1283 | 994 | 80 | 209 | 901 | 382 | 93 | 2 |

## Slice Reports

| Family | Slice | NLC | Execution | Pass | Skip | Fail | Missing | Lanes |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| `loader-context-supplementary` | `supplementary-builtin-completeness` | `NCF3` | `sequential` | 3 | 0 | 0 | 0 | node20:Node20/validation/measured_validation_lane, node22:Node22/primary/primary_contract, node24:Node24/preview/preview_visibility_lane |
| `loader-context-supplementary-global-injection` | `supplementary-global-injection-fidelity` | `NCF3` | `sequential` | 3 | 0 | 0 | 0 | node20:Node20/validation/measured_validation_lane, node22:Node22/primary/primary_contract, node24:Node24/preview/preview_visibility_lane |
| `loader-context-supplementary-module-bridge` | `supplementary-module-resolution-bridge` | `NCF3` | `sequential` | 3 | 0 | 0 | 0 | node20:Node20/validation/measured_validation_lane, node22:Node22/primary/primary_contract, node24:Node24/preview/preview_visibility_lane |
| `networking` | `dns-net-foundation` | `NLC6` | `sequential` | 29 | 0 | 0 | 0 | node20:Node20/validation/measured_validation_lane, node22:Node22/primary/primary_contract, node24:Node24/preview/preview_visibility_lane |
| `process-and-timing-supplementary` | `supplementary-process-release-shape` | `NCF3` | `expected_failure` | 0 | 0 | 3 | 0 | node20:Node20/validation/measured_validation_lane, node22:Node22/primary/primary_contract, node24:Node24/preview/preview_visibility_lane |
| `runtime-supplementary` | `supplementary-framework-loader-patterns` | `NCF3` | `sequential` | 3 | 0 | 0 | 0 | node20:Node20/validation/measured_validation_lane, node22:Node22/primary/primary_contract, node24:Node24/preview/preview_visibility_lane |
| `runtime-supplementary` | `supplementary-resource-safety` | `NCF3` | `sequential` | 3 | 0 | 0 | 0 | node20:Node20/validation/measured_validation_lane, node22:Node22/primary/primary_contract, node24:Node24/preview/preview_visibility_lane |
| `runtime-supplementary-signal-lifecycle` | `supplementary-signal-listener-lifecycle` | `NCF3` | `expected_failure` | 0 | 0 | 3 | 0 | node20:Node20/validation/measured_validation_lane, node22:Node22/primary/primary_contract, node24:Node24/preview/preview_visibility_lane |

## Canary Claims

| Claim | Profile | Status | Required lanes | Observed lanes |
| --- | --- | --- | --- | --- |
| `application-networking-express` | `Application` | `passed` | node22, node20 | node20:Node20/validation/measured_validation_lane, node22:Node22/primary/primary_contract |
| `application-networking-fastify` | `Application` | `passed` | node22, node20 | node20:Node20/validation/measured_validation_lane, node22:Node22/primary/primary_contract |
| `application-networking-socket-io` | `Application` | `passed` | node22 | node22:Node22/primary/primary_contract |
| `application-networking-undici` | `Application` | `passed` | node22 | node22:Node22/primary/primary_contract |
| `application-networking-axios` | `Application` | `passed` | node22 | node22:Node22/primary/primary_contract |
| `tooling-loader-tsx` | `Tooling` | `passed` | node22 | node22:Node22/primary/primary_contract |
| `tooling-loader-ts-node` | `Tooling` | `passed` | node22 | node22:Node22/primary/primary_contract |
| `tooling-loader-jest` | `Tooling` | `passed` | node22 | node22:Node22/primary/primary_contract |
| `tooling-loader-prisma` | `Tooling` | `passed` | node22 | node22:Node22/primary/primary_contract |
| `tooling-loader-next` | `Tooling` | `passed` | node22 | node22:Node22/primary/primary_contract |

## Required Canary Gaps
- none

## Oracle Reports

| Lane | Fixture | Runtime | Oracle | Drift | Node | Role |
| --- | --- | --- | --- | --- | --- | --- |
| `node22` | `test/parallel/test-buffer-alloc.js` | `pass` | `pass` | `agreement_pass` | `v22.22.2` | `primary/primary_contract` |
