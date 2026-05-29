# Node Compatibility Surface Matrix

Status: generated-evidence index

This page is the architecture-facing index for Nimbus's Node compatibility
support posture. The detailed support matrix is generated from checked-in
manifest and evidence artifacts instead of being copied into this hand-written
document.

## Source Of Truth

Use these generated artifacts for current support numbers and lane details:

- [`node-lts-compat/node-lts-lanes.md`](node-lts-compat/node-lts-lanes.md)
- [`node-lts-compat/node-lts-lanes.json`](node-lts-compat/node-lts-lanes.json)
- [`node-lts-compat/node-latest-suite-tags.md`](node-lts-compat/node-latest-suite-tags.md)
- [`node-lts-compat/node-latest-suite-tags.json`](node-lts-compat/node-latest-suite-tags.json)
- [`node-faas-compatibility-profile.md`](node-faas-compatibility-profile.md)
- [`node-faas-compatibility-profile.json`](node-faas-compatibility-profile.json)
- [`docs/runtimes/nodejs/evidence/latest.md`](../../runtimes/nodejs/evidence/latest.md)
- [`docs/runtimes/nodejs/evidence/node20.md`](../../runtimes/nodejs/evidence/node20.md)
- [`docs/runtimes/nodejs/evidence/node22.md`](../../runtimes/nodejs/evidence/node22.md)
- [`docs/runtimes/nodejs/evidence/node24.md`](../../runtimes/nodejs/evidence/node24.md)
- [`node-compat-evidence/latest/status-summary.md`](node-compat-evidence/latest/status-summary.md)
- [`node-compat-evidence/latest/dashboard-summary.md`](node-compat-evidence/latest/dashboard-summary.md)
- [`node-compat-evidence/latest/trend-summary.md`](node-compat-evidence/latest/trend-summary.md)

The generator is `scripts/runtime/node/publish_docs.py`; run
`make node-compat-publish-docs CHECK=1` to verify that the checked-in public
evidence pages match the current architecture evidence snapshots.

Use `bash scripts/verify-node-latest-suite-tags.sh` to validate latest official
Node suite tag metadata. The optional
`NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1` mode intentionally fails until stale or
missing fixture corpora are synced by the NFRC fixture-corpus rows.

## Manifest Inputs

Manifest-owned fixture families live under
`crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/fixtures/`.
Lane metadata lives under
`crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/`.

The broad Node LTS source baseline remains generated separately:

- [`node-lts-compat-summary.md`](node-lts-compat/node-lts-compat-summary.md)
- [`node-lts-compat-matrix.csv`](node-lts-compat/node-lts-compat-matrix.csv)
- [`node20-symbols.csv`](node-lts-compat/node20-symbols.csv)
- [`node22-symbols.csv`](node-lts-compat/node22-symbols.csv)
- [`deno-node-impl-inventory.csv`](node-lts-compat/deno-node-impl-inventory.csv)

## Public Contract

- Node lane support phase, product default, upstream release line, and evidence
  policy are defined by the checked-in Node LTS lane registry.
- FaaS support statuses, Node API families, package classes, service/microVM
  routing requirements, local-dev-only behavior, and generated-doc fields are
  defined by the checked-in Node FaaS compatibility profile.
- Product default is a routing default, not an evidence priority. Node22 and
  Node24 are the current supported LTS lanes; Node20 remains only
  legacy-grace regression coverage after EOL, and Node26 remains
  Current/non-LTS until LTS promotion gates pass.
- Nimbus does not claim full Node built-in compatibility for any target.
- A surface is supported only when a passed fixture, canary, oracle check, or
  explicit classification supports that claim.
- Expected failures, known gaps, skips, exclusions, and unclassified fixtures
  are not support claims.
- Runtime permission mode and explicit grants remain separate from Node
  compatibility target selection.

## Runtime Posture

The generated evidence currently proves a bounded Node-compatible runtime
surface for Convex-compatible `"use node"` actions. It should be read as an
explicit, measured contract rather than an implication of Node CLI parity.

Host-heavy behavior such as `node --test`, inspector behavior, workers, child
processes, native addons, and filesystem or network access is supported only
where the generated evidence names the specific fixture, canary, or
classification.

## Package And Framework Canaries

Package/framework canary claims are registered in
`tests/runtime/node/canary-registry.json` and summarized in the generated
dashboard. The current registry uses lane-local checks for the supported LTS
lanes, Node22 and Node24; Node20 may appear only as legacy-grace regression
coverage, and Node26 Current/non-LTS canaries are reported separately from LTS
support claims.

Current registered canary packages:

- Application/runtime: `node-platform-builtins`, `express`, `fastify`,
  `socket.io`, `undici`, `axios`, `convex-use-node-action`, and
  `convex-use-node-real-app`.
- Application SDKs: `openai`, `@anthropic-ai/sdk`, `ai`, `stripe`, `resend`,
  `@aws-sdk/client-s3`, `@slack/web-api`, `octokit`, `jose`, `zod`, `uuid`,
  `nanoid`, and `@upstash/redis`.
- Tooling: `tsx`, `ts-node`, `jest`, `prisma`, and `next`.
- Host-heavy diagnostics: `node:child_process`, `node:worker_threads`,
  `node:inspector`, `node:repl`, `node --test`, `native-addon`,
  `persistent-filesystem`, `raw-server-listen`, `prisma`, `sharp`, and
  `esbuild`.

The `node-platform-builtins` canary covers ESM/CJS loading, process metadata,
fs/path, streams, timers, crypto, and fetch/http. The
`convex-use-node-action` canary covers Convex-compatible `"use node"` action
package metadata for Node external packages. The `convex-use-node-real-app`
canary covers a real Convex-compatible action flow with a staged package
import, `ctx.runQuery`, `ctx.runMutation`, intentional `ctx.runAction`
runtime crossing, scheduler interaction, value serialization, fetch,
environment/secret boundary checks, crypto/stream/path/fs temp behavior, and a
dangling-promise diagnostic.
The SDK canaries run real package code against local mock services where a
third-party API would normally be required, so the canary evidence covers
package import, request construction, response parsing, crypto/JWT helpers, and
schema/ID utilities without depending on external credentials.
Host-heavy diagnostic canaries are not positive support claims. They prove that
production in-process application profiles deny or route child processes,
worker threads, inspector, REPL, `node --test`, native addon loading,
persistent filesystem assumptions, raw server listen behavior, Prisma-style
engine loading, sharp native loading, and esbuild binary execution toward the
service/microVM boundary.

## Refresh Workflow

Use [`docs/runtimes/nodejs/evidence/refreshing.md`](../../runtimes/nodejs/evidence/refreshing.md)
when updating lane metadata, syncing against an upstream Node tag, regenerating
dashboards, or preparing a future `nodeNN` lane.
