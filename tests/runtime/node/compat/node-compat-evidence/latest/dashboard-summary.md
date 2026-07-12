# Node.js Runtime Support Dashboard

- Representative Node test checks: 0
- Package/framework canary claims: 79
- Package/framework canary checks: 99
- Canary artifact bundles: 5
- Oracle reports: 4
- Inventory reports: 0

## Suite Status
- source: `target/node-compat/status/status-summary.json`
- rust ignored tests: `150`

| Lane | Upstream | Role | Passed | Expected failure / known gap | Skipped / excluded | Classified total | Classified coverage count | Vendored | Unclassified | Pass rate |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `node20` | `v20.20.2` | `legacy` | 919 | 3316 | 13 | 3329 | 4248 | 4248 | 0 | 21.6% |
| `node22` | `v22.22.3` | `supported` | 2363 | 2365 | 20 | 2385 | 4748 | 4748 | 0 | 49.8% |
| `node24` | `v24.16.0` | `default` | 2400 | 2750 | 48 | 2798 | 5198 | 5198 | 0 | 46.2% |
| `node26` | `v26.2.0` | `current` | 2092 | 3432 | 54 | 3486 | 5578 | 5578 | 0 | 37.5% |

### Evidence Tiers

| Tier | Source | Primary count | Passed | Claims | Official denominator? |
| --- | --- | ---: | ---: | ---: | --- |
| `official` | `vendored_official_fixture_corpus` | 19772 fixture_count | 7774 | - | yes |
| `supplementary` | `node_compat_manifest_test_tier` | 7 fixture_count | - | - | no |
| `regression` | `crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/regression` | 26 fixture_count | - | - | no |
| `canary` | `tests/runtime/node/canary-registry.json` | 37 active_canary_count | - | 79 | no |
| `watchpoint` | `tests/runtime/node/expectations/rust-watchpoints.json` | 150 catalog_entry_count | - | - | no |
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
| `application-platform-builtins` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-platform-esm-cjs-loading` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-platform-process-metadata` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-platform-file-path-roundtrip` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-platform-stream-timer` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-platform-crypto-fetch-http` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-express` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-express-middleware` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-express-error-handler` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-fastify` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-fastify-hooks` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-fastify-error-handler` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-socket-io` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-socket-io-websocket-events` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-undici` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-undici-success-response` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-undici-error-response` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-axios` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-axios-success-response` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `application-networking-axios-error-response` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `convex-use-node-action-packaging` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `convex-use-node-action-package-metadata` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract |
| `convex-use-node-real-app` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `convex-use-node-real-app-ctx-calls` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `convex-use-node-real-app-builtins` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `convex-use-node-real-app-diagnostics` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-openai` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-openai-chat-completions` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-openai-auth-and-model` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-anthropic` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-anthropic-messages` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-anthropic-auth-and-model` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-vercel-ai` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-vercel-ai-json-schema` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-vercel-ai-tool-execution` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-stripe` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-stripe-customer-create` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-stripe-form-auth` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-resend` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-resend-email-send` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-resend-auth-subject` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-aws-sdk-v3-s3` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-aws-s3-list-buckets` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-aws-s3-signing-node-http-handler` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-slack-web-api` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-slack-auth-test` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-slack-bearer-auth` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-octokit` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-octokit-user-request` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-octokit-token-auth` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-jose` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-jose-sign-verify` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-jose-protected-header` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-zod` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-zod-parse-success` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-zod-parse-failure` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-uuid` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-uuid-v5-deterministic` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-nanoid` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-nanoid-custom-alphabet` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-upstash-redis` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-upstash-redis-set-get` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
| `application-sdk-upstash-redis-rest-command` | `Application` | Support | Supported | Passed | node22, node24 | node22:Node22/supported/supported_contract, node24:Node24/default/default_contract, node26:Node26/current/current_contract |
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
| `node20` | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement pass | `v20.20.2` | `legacy/legacy_contract` |
| `node22` | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement pass | `v22.23.1` | `supported/supported_contract` |
| `node24` | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement pass | `v24.16.0` | `default/default_contract` |
| `node26` | `test/parallel/test-buffer-alloc.js` | Passed | Passed | Agreement pass | `v26.0.0` | `current/current_contract` |
