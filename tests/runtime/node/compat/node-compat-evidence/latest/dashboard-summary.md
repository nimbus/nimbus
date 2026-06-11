# Node.js Runtime Support Dashboard

- Representative Node test checks: 8
- Package/framework canary claims: 37
- Package/framework canary checks: 101
- Canary artifact bundles: 2
- Oracle reports: 2
- Inventory reports: 4

## Suite Status
- source: `target/node-compat/status/status-summary.json`
- rust ignored tests: `67`

| Lane | Upstream | Role | Passed | Expected failure / known gap | Skipped / excluded | Classified total | Classified coverage count | Vendored | Unclassified | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | `legacy` | 902 | 401 | 5 | 406 | 1308 | 1308 | 0 | 69.0% |
| `node22` | `v22.22.3` | `supported` | 1000 | 3728 | 20 | 3748 | 4748 | 4748 | 0 | 21.1% |
| `node24` | `v24.16.0` | `default` | 1002 | 4149 | 47 | 4196 | 5198 | 5198 | 0 | 19.3% |
| `node26` | `v26.2.0` | `current` | 0 | 5529 | 49 | 5578 | 5578 | 5578 | 0 | 0.0% |

### Suite Warnings
- none

## Fixture Inventory

| Lane | Upstream | Vendored | Passed | Expected failure / known gap / skipped total | Classified coverage count | Unclassified | Path-owned passed | Rust-referenced passed | Rust-unreferenced expected / skipped | Rust-unreferenced unclassified | Passed reconstructability gap | Warnings |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | 1308 | 902 | 406 | 1308 | 0 | 902 | 902 | 406 | 0 | 0 | 0 |
| `node22` | `v22.22.3` | 4748 | 1000 | 3748 | 4748 | 0 | 1000 | 1000 | 3748 | 0 | 0 | 0 |
| `node24` | `v24.16.0` | 5198 | 1002 | 4196 | 5198 | 0 | 1002 | 1002 | 4196 | 0 | 0 | 0 |
| `node26` | `v26.2.0` | 5578 | 0 | 5578 | 5578 | 0 | 0 | 0 | 5578 | 0 | 0 | 0 |

## Representative Node Test Checks

| API family | Check | Execution | Passed | Skipped | Failed | Missing | Lanes |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| `loader-context-supplementary` | `supplementary-builtin-completeness` | Sequential | 3 | 0 | 0 | 0 | node20:Node20/legacy/legacy_contract, node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `loader-context-supplementary-global-injection` | `supplementary-global-injection-fidelity` | Sequential | 3 | 0 | 0 | 0 | node20:Node20/legacy/legacy_contract, node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `loader-context-supplementary-module-bridge` | `supplementary-module-resolution-bridge` | Sequential | 3 | 0 | 0 | 0 | node20:Node20/legacy/legacy_contract, node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `networking` | `dns-net-foundation` | Sequential | 29 | 0 | 0 | 0 | node20:Node20/legacy/legacy_contract, node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `process-and-timing-supplementary` | `supplementary-process-release-shape` | Sequential | 3 | 0 | 0 | 0 | node20:Node20/legacy/legacy_contract, node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `runtime-supplementary` | `supplementary-framework-loader-patterns` | Sequential | 3 | 0 | 0 | 0 | node20:Node20/legacy/legacy_contract, node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `runtime-supplementary` | `supplementary-resource-safety` | Sequential | 3 | 0 | 0 | 0 | node20:Node20/legacy/legacy_contract, node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `runtime-supplementary-signal-lifecycle` | `supplementary-signal-listener-lifecycle` | Expected failure | 0 | 0 | 3 | 0 | node20:Node20/legacy/legacy_contract, node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |

## Package/Framework Canaries

| Claim | Preset | Evidence | Support boundary | Result | Required lanes | Observed lanes |
| --- | --- | --- | --- | --- | --- | --- |
| `application-platform-builtins` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-express` | `Application` | Support | Supported | Passed | node22, node24 | node20:Node20/legacy/legacy_contract, node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-fastify` | `Application` | Support | Supported | Passed | node22, node24 | node20:Node20/legacy/legacy_contract, node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-socket-io` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-undici` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-axios` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `convex-use-node-action-packaging` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `convex-use-node-real-app` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-openai` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-anthropic` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-vercel-ai` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-stripe` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-resend` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-aws-sdk-v3-s3` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-slack-web-api` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-octokit` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-jose` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-zod` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-uuid` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-nanoid` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-upstash-redis` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `host-heavy-child-process-denied` | `Application` | Diagnostic | Service/microVM required | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `host-heavy-worker-threads-denied` | `Application` | Diagnostic | Service/microVM required | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `host-heavy-inspector-denied` | `Application` | Diagnostic | Service/microVM required | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `host-heavy-repl-denied` | `Application` | Diagnostic | Service/microVM required | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `host-heavy-node-test-runner-denied` | `Application` | Diagnostic | Service/microVM required | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `host-heavy-native-addon-denied` | `Application` | Diagnostic | Service/microVM required | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `host-heavy-persistent-filesystem-denied` | `Application` | Diagnostic | Service/microVM required | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `host-heavy-raw-server-listen-denied` | `Application` | Diagnostic | Service/microVM required | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `host-heavy-prisma-engine-routed` | `Application` | Diagnostic | Service/microVM required | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `host-heavy-sharp-native-routed` | `Application` | Diagnostic | Service/microVM required | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `host-heavy-esbuild-binary-routed` | `Application` | Diagnostic | Service/microVM required | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `tooling-loader-tsx` | `Tooling` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `tooling-loader-ts-node` | `Tooling` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `tooling-loader-jest` | `Tooling` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `tooling-loader-prisma` | `Tooling` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `tooling-loader-next` | `Tooling` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |

## Required Canary Gaps
- none

## Oracle Reports

| Lane | Fixture | Runtime | Oracle | Drift | Node | Role |
| --- | --- | --- | --- | --- | --- | --- |
| `node22` | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement pass | `v22.22.2` | `supported/supported_contract` |
| `node24` | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement pass | `v24.16.0` | `default/default_contract` |
