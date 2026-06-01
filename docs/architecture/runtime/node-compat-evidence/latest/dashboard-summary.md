# Node.js Runtime Support Dashboard

- Representative Node test checks: 0
- Package/framework canary claims: 37
- Package/framework canary checks: 0
- Canary artifact bundles: 0
- Oracle reports: 0
- Inventory reports: 0

## Suite Status
- source: `target/node-compat/status/status-summary.json`
- rust ignored tests: `78`

| Lane | Upstream | Role | Passed | Expected failure / known gap | Skipped / excluded | Classified total | Classified coverage count | Vendored | Unclassified | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | `legacy` | 899 | 403 | 6 | 409 | 1308 | 1308 | 0 | 68.7% |
| `node22` | `v22.22.3` | `supported` | 1024 | 3729 | 20 | 3749 | 4773 | 4773 | 0 | 21.5% |
| `node24` | `v24.16.0` | `default` | 276 | 4872 | 50 | 4922 | 5198 | 5198 | 0 | 5.3% |
| `node26` | `v26.2.0` | `current` | 0 | 5529 | 49 | 5578 | 5578 | 5578 | 0 | 0.0% |

### Suite Warnings
- none

## Fixture Inventory

| Lane | Upstream | Vendored | Passed | Expected failure / known gap / skipped total | Classified coverage count | Unclassified | Path-owned passed | Rust-referenced passed | Rust-unreferenced expected / skipped | Rust-unreferenced unclassified | Passed reconstructability gap | Warnings |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| none | - | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

## Representative Node Test Checks

| API family | Check | Execution | Passed | Skipped | Failed | Missing | Lanes |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |

## Package/Framework Canaries

| Claim | Preset | Evidence | Support boundary | Result | Required lanes | Observed lanes |
| --- | --- | --- | --- | --- | --- | --- |
| `application-platform-builtins` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-express` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-fastify` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-socket-io` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-undici` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-axios` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `convex-use-node-action-packaging` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `convex-use-node-real-app` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-openai` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-anthropic` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-vercel-ai` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-stripe` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-resend` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-aws-sdk-v3-s3` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-slack-web-api` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-octokit` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-jose` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-zod` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-uuid` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-nanoid` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-upstash-redis` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `host-heavy-child-process-denied` | `Application` | Diagnostic | Service/microVM required | Missing observation | node22, node24 | none |
| `host-heavy-worker-threads-denied` | `Application` | Diagnostic | Service/microVM required | Missing observation | node22, node24 | none |
| `host-heavy-inspector-denied` | `Application` | Diagnostic | Service/microVM required | Missing observation | node22, node24 | none |
| `host-heavy-repl-denied` | `Application` | Diagnostic | Service/microVM required | Missing observation | node22, node24 | none |
| `host-heavy-node-test-runner-denied` | `Application` | Diagnostic | Service/microVM required | Missing observation | node22, node24 | none |
| `host-heavy-native-addon-denied` | `Application` | Diagnostic | Service/microVM required | Missing observation | node22, node24 | none |
| `host-heavy-persistent-filesystem-denied` | `Application` | Diagnostic | Service/microVM required | Missing observation | node22, node24 | none |
| `host-heavy-raw-server-listen-denied` | `Application` | Diagnostic | Service/microVM required | Missing observation | node22, node24 | none |
| `host-heavy-prisma-engine-routed` | `Application` | Diagnostic | Service/microVM required | Missing observation | node22, node24 | none |
| `host-heavy-sharp-native-routed` | `Application` | Diagnostic | Service/microVM required | Missing observation | node22, node24 | none |
| `host-heavy-esbuild-binary-routed` | `Application` | Diagnostic | Service/microVM required | Missing observation | node22, node24 | none |
| `tooling-loader-tsx` | `Tooling` | Support | Supported | Missing observation | node22, node24 | none |
| `tooling-loader-ts-node` | `Tooling` | Support | Supported | Missing observation | node22, node24 | none |
| `tooling-loader-jest` | `Tooling` | Support | Supported | Missing observation | node22, node24 | none |
| `tooling-loader-prisma` | `Tooling` | Support | Supported | Missing observation | node22, node24 | none |
| `tooling-loader-next` | `Tooling` | Support | Supported | Missing observation | node22, node24 | none |

## Required Canary Gaps
- none

## Oracle Reports
- none
