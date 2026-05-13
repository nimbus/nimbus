# Node.js Runtime Support Dashboard

- Representative Node test checks: 8
- Package/framework canary claims: 10
- Package/framework canary checks: 12
- Canary artifact bundles: 2
- Oracle reports: 1
- Inventory reports: 3

## Suite Status
- source: `target/node-compat/status/status-summary.json`
- rust ignored tests: `61`

| Lane | Upstream | Role | Passed | Expected failure / known gap | Skipped / excluded | Classified total | Classified coverage count | Vendored | Unclassified | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | `supported` | 904 | 399 | 5 | 404 | 1308 | 1308 | 0 | 69.1% |
| `node22` | `v22.15.0` | `default` | 876 | 403 | 4 | 407 | 1283 | 1283 | 0 | 68.3% |
| `node24` | `v24.15.0` | `supported` | 925 | 567 | 3 | 570 | 1495 | 1495 | 0 | 61.9% |

### Suite Warnings
- none

## Fixture Inventory

| Lane | Upstream | Vendored | Passed | Expected failure / known gap / skipped total | Classified coverage count | Unclassified | Path-owned passed | Rust-referenced passed | Rust-unreferenced expected / skipped | Rust-unreferenced unclassified | Passed reconstructability gap | Warnings |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 1308 | 904 | 404 | 1308 | 0 | 904 | 904 | 404 | 0 | 0 | 0 |
| `node22` | `v22.15.0` | 1283 | 876 | 407 | 1283 | 0 | 876 | 876 | 407 | 0 | 0 | 0 |
| `node24` | `v24.15.0` | 1495 | 925 | 570 | 1495 | 0 | 925 | 925 | 570 | 0 | 0 | 0 |

## Representative Node Test Checks

| API family | Check | Execution | Passed | Skipped | Failed | Missing | Lanes |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| `loader-context-supplementary` | `supplementary-builtin-completeness` | Sequential | 3 | 0 | 0 | 0 | node20:Node20/supported/supported_contract, node22:Node22/default/default_contract, node24:Node24/supported/supported_contract |
| `loader-context-supplementary-global-injection` | `supplementary-global-injection-fidelity` | Sequential | 3 | 0 | 0 | 0 | node20:Node20/supported/supported_contract, node22:Node22/default/default_contract, node24:Node24/supported/supported_contract |
| `loader-context-supplementary-module-bridge` | `supplementary-module-resolution-bridge` | Sequential | 3 | 0 | 0 | 0 | node20:Node20/supported/supported_contract, node22:Node22/default/default_contract, node24:Node24/supported/supported_contract |
| `networking` | `dns-net-foundation` | Sequential | 29 | 0 | 0 | 0 | node20:Node20/supported/supported_contract, node22:Node22/default/default_contract, node24:Node24/supported/supported_contract |
| `process-and-timing-supplementary` | `supplementary-process-release-shape` | Expected failure | 0 | 0 | 3 | 0 | node20:Node20/supported/supported_contract, node22:Node22/default/default_contract, node24:Node24/supported/supported_contract |
| `runtime-supplementary` | `supplementary-framework-loader-patterns` | Sequential | 3 | 0 | 0 | 0 | node20:Node20/supported/supported_contract, node22:Node22/default/default_contract, node24:Node24/supported/supported_contract |
| `runtime-supplementary` | `supplementary-resource-safety` | Sequential | 3 | 0 | 0 | 0 | node20:Node20/supported/supported_contract, node22:Node22/default/default_contract, node24:Node24/supported/supported_contract |
| `runtime-supplementary-signal-lifecycle` | `supplementary-signal-listener-lifecycle` | Expected failure | 0 | 0 | 3 | 0 | node20:Node20/supported/supported_contract, node22:Node22/default/default_contract, node24:Node24/supported/supported_contract |

## Package/Framework Canaries

| Claim | Preset | Status | Required lanes | Observed lanes |
| --- | --- | --- | --- | --- |
| `application-networking-express` | `Application` | Passed | node22, node20 | node20:Node20/supported/supported_contract, node22:Node22/default/default_contract |
| `application-networking-fastify` | `Application` | Passed | node22, node20 | node20:Node20/supported/supported_contract, node22:Node22/default/default_contract |
| `application-networking-socket-io` | `Application` | Passed | node22 | node22:Node22/default/default_contract |
| `application-networking-undici` | `Application` | Passed | node22 | node22:Node22/default/default_contract |
| `application-networking-axios` | `Application` | Passed | node22 | node22:Node22/default/default_contract |
| `tooling-loader-tsx` | `Tooling` | Passed | node22 | node22:Node22/default/default_contract |
| `tooling-loader-ts-node` | `Tooling` | Passed | node22 | node22:Node22/default/default_contract |
| `tooling-loader-jest` | `Tooling` | Passed | node22 | node22:Node22/default/default_contract |
| `tooling-loader-prisma` | `Tooling` | Passed | node22 | node22:Node22/default/default_contract |
| `tooling-loader-next` | `Tooling` | Passed | node22 | node22:Node22/default/default_contract |

## Required Canary Gaps
- none

## Oracle Reports

| Lane | Fixture | Runtime | Oracle | Drift | Node | Role |
| --- | --- | --- | --- | --- | --- | --- |
| `node22` | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement pass | `v22.22.2` | `default/default_contract` |
