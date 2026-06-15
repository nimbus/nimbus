# Node.js Runtime Support Dashboard

- Representative Node test checks: 0
- Package/framework canary claims: 79
- Package/framework canary checks: 0
- Canary artifact bundles: 0
- Oracle reports: 0
- Inventory reports: 0

## Suite Status
- source: `target/node-compat/status/status-summary.json`
- rust ignored tests: `147`

| Lane | Upstream | Role | Passed | Expected failure / known gap | Skipped / excluded | Classified total | Classified coverage count | Vendored | Unclassified | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | `legacy` | 917 | 3318 | 13 | 3331 | 4248 | 4248 | 0 | 21.6% |
| `node22` | `v22.22.3` | `supported` | 2363 | 2365 | 20 | 2385 | 4748 | 4748 | 0 | 49.8% |
| `node24` | `v24.16.0` | `default` | 2400 | 2750 | 48 | 2798 | 5198 | 5198 | 0 | 46.2% |
| `node26` | `v26.2.0` | `current` | 1786 | 3746 | 46 | 3792 | 5578 | 5578 | 0 | 32.0% |

### Evidence Tiers

| Tier | Source | Primary count | Passed | Claims | Official denominator? |
| --- | --- | ---: | ---: | ---: | --- |
| `official` | `vendored_official_fixture_corpus` | 19772 fixture_count | 7466 | - | yes |
| `supplementary` | `node_compat_manifest_test_tier` | 7 fixture_count | - | - | no |
| `regression` | `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/regression` | 26 fixture_count | - | - | no |
| `canary` | `tests/runtime/node/canary-registry.json` | 37 active_canary_count | - | 79 | no |
| `watchpoint` | `tests/runtime/node/expectations/rust-watchpoints.json` | 147 catalog_entry_count | - | - | no |
| `diagnostic` | `tests/runtime/node/expectations/rust-watchpoints.json + tests/runtime/node/canary-registry.json` | 11 diagnostic_count | - | 11 | no |

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
| `application-platform-esm-cjs-loading` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-platform-process-metadata` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-platform-file-path-roundtrip` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-platform-stream-timer` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-platform-crypto-fetch-http` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-express` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-express-middleware` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-express-error-handler` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-fastify` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-fastify-hooks` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-fastify-error-handler` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-socket-io` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-socket-io-websocket-events` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-undici` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-undici-success-response` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-undici-error-response` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-axios` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-axios-success-response` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-networking-axios-error-response` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `convex-use-node-action-packaging` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `convex-use-node-action-package-metadata` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `convex-use-node-real-app` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `convex-use-node-real-app-ctx-calls` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `convex-use-node-real-app-builtins` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `convex-use-node-real-app-diagnostics` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-openai` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-openai-chat-completions` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-openai-auth-and-model` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-anthropic` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-anthropic-messages` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-anthropic-auth-and-model` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-vercel-ai` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-vercel-ai-json-schema` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-vercel-ai-tool-execution` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-stripe` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-stripe-customer-create` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-stripe-form-auth` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-resend` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-resend-email-send` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-resend-auth-subject` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-aws-sdk-v3-s3` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-aws-s3-list-buckets` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-aws-s3-signing-node-http-handler` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-slack-web-api` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-slack-auth-test` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-slack-bearer-auth` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-octokit` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-octokit-user-request` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-octokit-token-auth` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-jose` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-jose-sign-verify` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-jose-protected-header` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-zod` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-zod-parse-success` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-zod-parse-failure` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-uuid` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-uuid-v5-deterministic` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-nanoid` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-nanoid-custom-alphabet` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-upstash-redis` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-upstash-redis-set-get` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
| `application-sdk-upstash-redis-rest-command` | `Application` | Support | Supported | Missing observation | node22, node24 | none |
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
