# Node FaaS Runtime Compatibility Research (2026)

Status: research baseline
Date: 2026-05-28
Owner: `runtime / convex node-compat / docs`

## Purpose

This document records the research baseline for supporting realistic Node.js
applications in Nimbus functions-as-a-service execution. It extends the
completed Node LTS Runtime Trust baseline from
[`../archive/node-lts-runtime-trust-plan.md`](../archive/node-lts-runtime-trust-plan.md)
from "truthful Node lane metadata and measured fixtures" to "developer-usable
FaaS and Convex-compatible app support, with Deno-style support docs."

## Executive Finding

Nimbus needs two separate, machine-checked contracts:

1. A Node lane contract for release phases, fixture corpus tags, process
   metadata, and upstream official test-suite classification.
2. A FaaS app contract for what developers can actually run inside
   Convex-compatible `"use node"` actions and Nimbus function handlers.

The first contract should require complete classification of the official Node
suite for every targeted lane. The second contract should require 100% pass
for the declared FaaS app profile. Enterprise trust comes from refusing to blur
those two numbers: a failed or classified Node CLI fixture is acceptable only
when the public FaaS docs say the behavior is unsupported, service-routed, or
not applicable.

This research does not recommend replacing the current in-process Deno/V8
engine with embedded Node. For this plan, Node20, Node22, Node24, and Node26
are compatibility contracts implemented on Nimbus's existing `v8_deno_core`
runtime substrate. Compiling official Node or `libnode` into Nimbus would be a
separate runtime-engine project with separate build, isolation, pooling,
permission, and per-major artifact obligations.

## Current Nimbus Baseline

As of 2026-05-28:

| Lane | Registry status | Latest upstream tag in registry | Current fixture corpus | Current public role |
| --- | --- | --- | --- | --- |
| Node20 | `eol_legacy` | `v20.20.2` | `v20.20.2` | Legacy-grace regression only |
| Node22 | `maintenance_lts` | `v22.22.3` | `v22.15.0` | Product default and supported LTS |
| Node24 | `active_lts` | `v24.16.0` | `v24.15.0` | Supported LTS peer |
| Node26 | `preview_current` today; should become Current/non-LTS in NFRC | `v26.2.0` | none | Current-line metadata only |

The generated dashboard currently reports zero unclassified fixtures for
Node20, Node22, and Node24, but it has no Node26 fixture lane. It also proves
application canaries for platform builtins, Express, Fastify, Socket.IO,
Undici, Axios, Convex `"use node"` packaging, and selected tooling canaries
for Node22 and Node24. That is a good foundation, but it does not yet prove a
real Convex app action end-to-end, a Deno-style API reference, or Node26
Current-line behavior.

Nimbus's current in-process runtime is a Deno/V8 embedder path. The Node lanes
are API compatibility targets and policy profiles on that engine. Public docs
must make that visible: Nimbus can say it supports a verified Node24 FaaS
contract, but it must not imply that the official Node24 binary or `libnode` is
executing user code.

## Release Train Facts

The official Node release working group lists Node22 as Maintenance LTS,
Node24 as Active LTS, Node25 as non-LTS Maintenance, and Node26 as Current.
Node20 is End-of-Life as of 2026-04-30. Node's public releases page also says
production applications should use Active LTS or Maintenance LTS releases.

Node26 was released as Current on 2026-05-05. The Node26 release announcement
says it will enter LTS in October 2026 and calls out semver-major platform
changes such as Temporal enabled by default, V8 14.6, and Undici 8.0. That
makes Node26 important for Current-line compatibility now, but not an enterprise LTS
support promise until it enters LTS and Nimbus has lane-local evidence.

Local `~/src/github.com/nodejs/node` tags used for this research:

| Line | Tag | Tag object | Commit |
| --- | --- | --- | --- |
| Node20 | `v20.20.2` | `35e07843146797923006aa01c6daabf4f53a4fb9` | `3626fea570e44896ad99aaf3bf6e59def5adede5` |
| Node22 | `v22.22.3` | `354ef4b9bd94d5b662a9c300ddacc67f95a1bbe8` | `fdfa0ff0dbaf0fbf4d7d6d89a2ab807f3177fa5c` |
| Node24 | `v24.16.0` | `75143a8d75629c5d429dd0becb0d725e955f48fb` | `c7d10158bc31036de6783d66beaaaf551e3167aa` |
| Node26 | `v26.2.0` | `30ffe3cfc2fda3684c38ec43aa79c381d398bf14` | `cfd7920d5a2d84905c4292362d01d07870047e93` |

## Provider And Runtime Matrix

| Runtime/provider | Current observed Node posture | Lesson for Nimbus |
| --- | --- | --- |
| AWS Lambda | Official Node runtimes include `nodejs24.x` and `nodejs22.x`; Lambda documents runtime-specific SDK v3 versions, keep-alive behavior, CA certificate changes, and disabled experimental Node features. | Match the FaaS model, not raw Node CLI behavior. Docs need runtime-version semantics, disabled experimental behavior, and invocation constraints. |
| Google Cloud Run functions / Cloud Functions | Official runtime support lists Node.js 24, 22, and 20 with deprecation and decommission dates. | Keep a lifecycle table and make EOL/decommission separate from "can still run locally." |
| Azure Functions | Official runtime table lists Node.js 24 Preview, Node.js 22 GA, and Node.js 20 GA, with Node22 as the last Linux Consumption-plan Node version. | Provider preview labels are platform support labels, not Node release phases. Nimbus should label Node26 by Node's official Current phase and separately mark it non-LTS. Platform constraints belong beside version support. |
| Vercel Functions | Official docs list Node 24.x as default, with 22.x and 20.x selectable, and automatic minor/patch rollout. | Nimbus should move default to Node24 and treat patch/minor updates as evidence refreshes rather than hand-edited prose. |
| Netlify Functions | Functions run in the configured site Node version; docs call out Node 18+ minimum for native Fetch, ESM/CJS behavior, and bundling controls. | Developers expect package/module-format docs, not only built-in API percentages. |
| Cloudflare Workers | Workers document a subset of Node APIs, with supported, partial, non-functional stub, and unsupported statuses, plus a compatibility flag. | Copy the explicit status vocabulary. It is acceptable to support imports for some APIs while rejecting runtime use, but docs and errors must say so. |
| Convex actions | Convex actions can run in Convex's runtime by default or in Node.js with `"use node"`. Node actions handle unsupported npm packages or Node APIs, have higher memory, can do concurrent operations, and may be reused between invocations. | Nimbus should prove real Convex action flows, not only package metadata. Docs must explain runtime crossing, dangling promise/lifecycle behavior, and supported Node action packages. |
| Deno | Deno's docs pair a fundamentals page with a Node API reference and test-suite status language. The fundamentals page says Deno supports npm packages and Node built-ins, while the API page lists module/global support and notes. | Nimbus docs should split "how to use Node in Nimbus" from "API reference by support status and Node lane." |

## Embedded Node Alternative

Real embedded Node is a different architecture from this compatibility plan.
It would require either a selected `libnode` major linked into a Nimbus runtime
artifact or one runner artifact per supported major. That path may be useful if
the Deno/V8 compatibility substrate cannot satisfy important FaaS workloads,
but it is not a simplification of NFRC.

The enterprise-trust bar for a real Node engine would include reproducible
Node builds, per-major version ownership, `libuv` lifecycle control, host-call
transport, timeout/cancellation behavior, teardown and reuse proofs,
permission mediation for Node builtins, native-addon policy, and honest
operator diagnostics. Those gates belong under
`docs/architecture/runtime/new-engine-proof-harness.md`, not hidden inside a
Node compatibility lane.

The practical decision for NFRC is therefore:

- Keep `v8_deno_core` as the production in-process engine.
- Treat Node22 and Node24 as supported LTS FaaS compatibility contracts once
  their declared app canaries pass 100%.
- Treat Node26 as a Current/non-LTS compatibility contract until the release
  schedule and Nimbus evidence promote it to LTS support.
- Document unsupported or host-heavy Node behavior as unsupported,
  local-dev-only, or service/microVM-required instead of silently widening the
  in-process engine.

## Realistic FaaS App Requirements

Realistic Node FaaS apps and Convex `"use node"` actions need these behavior
families to be boringly reliable:

| Family | Required FaaS behavior |
| --- | --- |
| Invocation lifecycle | Load module once, invoke handlers repeatedly, await all work, report dangling async hazards, preserve documented warm-isolate semantics, and fail closed on unsupported background work. |
| Module system | ESM, CommonJS, `node:` and bare built-in specifiers, package `exports`, conditional exports, `type`, `main`, `module`, local workspace packages, and package manager layouts used by bundled apps. |
| Package loading | Pinned npm dependencies, package manifests, transitive builtins, dynamic import where supported, deterministic bundling diagnostics, private registry guidance, and clear native-addon/subprocess boundaries. |
| Web and HTTP APIs | `fetch`, `Request`, `Response`, `Headers`, URL, Web Crypto, streams, AbortController, timers, and keep-alive-friendly outbound HTTP clients. |
| Node builtins for app code | `buffer`, `events`, `util`, `crypto`, `stream`, `path`, `url`, `querystring`, `zlib`, `assert`, `diagnostics_channel`, selected `fs`/`fs/promises` for packaged assets and temporary files, selected `os`/`process` metadata. |
| Networking | Outbound HTTP(S), DNS, TLS, request timeouts, aborts, proxies if supported, and no accidental generic inbound listen grants in production in-process profiles. |
| Environment and secrets | Explicit environment access, secrets injection, no ambient unsafe `NODE_TLS_REJECT_UNAUTHORIZED`, and docs for what `process.env` means per profile. |
| Observability | `console`, structured logs, error stacks, unhandled rejection diagnostics, source-map posture, invocation IDs, and supportable failure messages. |
| Convex integration | `"use node"` action files, generated server APIs, `ctx.runQuery`, `ctx.runMutation`, `ctx.runAction` only where appropriate, scheduler interaction, value serialization, and Node package action deployment. |
| AI/SaaS SDKs | Representative SDKs such as OpenAI, Anthropic, Vercel AI SDK, Stripe, Resend/SendGrid, AWS SDK v3, Slack, Octokit, Jose, Zod, uuid, nanoid, and database HTTP clients. |
| Host-heavy behavior | Child processes, workers, inspector, REPL, test runner, native addons, raw TCP servers, Unix sockets, FFI, and persistent writable filesystems require service/microVM routing or explicit unsupported diagnostics. |

## Support Tiers

Nimbus should publish the same status vocabulary everywhere:

| Status | Meaning |
| --- | --- |
| Supported in-process | Passes official fixtures or app canaries under the production in-process FaaS profile. |
| Supported local-dev only | Useful for local tooling or development, but not part of the production in-process contract. |
| Service/microVM required | Supported only when routed to a service or microVM profile with explicit host authority. |
| Import-compatible stub | The module can be imported because packages probe it, but runtime use throws a clear Nimbus diagnostic. |
| Unsupported | The behavior is rejected with a documented diagnostic and is not silently shimmed. |
| Not applicable to FaaS | Node CLI, interactive, or long-running process behavior that does not map to a short-lived function invocation. |

## Test Strategy

The latest official Node suite should be the upstream corpus for every targeted
lane:

| Lane | Corpus action |
| --- | --- |
| Node20 | Keep `v20.20.2` as legacy-grace regression only. Do not add new public support claims. |
| Node22 | Refresh official fixtures from `v22.15.0` to the latest Node22 tag, currently `v22.22.3`. |
| Node24 | Refresh official fixtures from `v24.15.0` to the latest Node24 tag, currently `v24.16.0`, and make it the product default. |
| Node26 | Add `v26.2.0` fixture corpus, target metadata, process metadata, classification, app canaries, and Current/non-LTS docs. |

The test suite should enforce three different gates:

- Official Node fixture gate: 100% classified for every targeted lane.
- Declared FaaS app gate: 100% passing for every supported LTS lane.
- Current-line gate: Node26 has lane-local evidence and zero unclassified
  fixtures before public Current-line claims; failures do not block LTS support
  unless the same behavior is claimed in the FaaS profile.

The execution strategy should deliberately favor wide feedback before focused
debugging. For every lane or app-profile change, Nimbus should first vendor,
sync, or enable the broadest relevant official fixture and canary corpus, run
that group to get a complete issue inventory, then use isolated fixtures to fix
or classify individual failures. A row is not complete until the broad group
reruns and proves that the issue inventory is closed or explicitly classified.
That keeps the work aimed at compatibility gaps rather than a long sequence of
small passing tests.

## Deno-Style Docs Model

Nimbus should copy the shape, not the exact claims:

- `docs/runtimes/nodejs/README.md`: quick start, `"use node"`, version
  selection, package model, examples, and sharp boundaries.
- `docs/runtimes/nodejs/compatibility.md`: support vocabulary, lifecycle
  table, FaaS profile, version defaults, and links to evidence.
- `docs/runtimes/nodejs/reference/node-apis.md`: generated API table with
  per-module status, per-lane notes, import/runtime behavior, and service route
  hints.
- `docs/runtimes/nodejs/reference/packages.md`: package canary matrix and
  common package guidance for SDKs, native addons, subprocess tools, and
  framework tooling.
- `docs/runtimes/nodejs/evidence/*.md`: generated evidence snapshots with no
  hand-maintained pass rates.

The docs guard must reject stale hand-written pass rates, stale default-lane
prose, and API claims that are not backed by fixtures, canaries, oracle output,
or explicit classification.

## Conclusions

- Node24 should become the Nimbus default because it is the active LTS line and
  provider defaults already moved there.
- Node22 remains a supported Maintenance LTS peer until its EOL date.
- Node20 should remain legacy-grace only and must not appear as active
  enterprise support.
- Node26 should be added now as a Current-line lane with latest official
  fixture classification and app canaries, but its enterprise LTS support
  status should flip only when it enters LTS and the FaaS app gate passes.
- Node lanes should remain compatibility contracts on the current
  `v8_deno_core` engine. A future real-Node/`libnode` backend is a separate
  new-engine proof, not part of this FaaS compatibility plan.
- Full upstream Node test-suite pass is not the right success metric for a
  FaaS runtime. The right metric is full classification of upstream tests plus
  full pass of the declared FaaS/Convex app support profile.
- The public docs should look more like Deno and Cloudflare: explicit status
  per API, clear package guidance, and sharp unsupported/service-routed
  boundaries.

## Sources

- Node release working group schedule:
  <https://github.com/nodejs/Release>
- Node previous releases:
  <https://nodejs.org/en/about/previous-releases>
- Node 26.0.0 release announcement:
  <https://nodejs.org/en/blog/release/v26.0.0/>
- Node release tags:
  <https://github.com/nodejs/node/releases>
- AWS Lambda Node.js runtime docs:
  <https://docs.aws.amazon.com/lambda/latest/dg/lambda-nodejs.html>
- Google Cloud Run functions runtime support:
  <https://cloud.google.com/functions/docs/runtime-support>
- Azure Functions runtime versions:
  <https://learn.microsoft.com/en-us/azure/azure-functions/functions-versions>
- Vercel Node.js versions:
  <https://vercel.com/docs/functions/runtimes/node-js/node-js-versions>
- Netlify Functions runtime docs:
  <https://docs.netlify.com/functions/get-started/>
- Cloudflare Workers Node.js compatibility:
  <https://developers.cloudflare.com/workers/runtime-apis/nodejs/>
- Convex actions docs:
  <https://docs.convex.dev/functions/actions>
- Deno Node and npm compatibility:
  <https://docs.deno.com/runtime/fundamentals/node/>
- Deno Node APIs reference:
  <https://docs.deno.com/runtime/reference/node_apis/>
