---
title: Node API reference
description: Node API families Nimbus supports, denies, or routes to service/microVM profiles.
sidebar:
  order: 2
---

The Node API families Nimbus exposes, denies, or routes for function
workloads. This is a measured support contract, not a blanket Node.js
compatibility claim — see the
[Node compatibility contract](/reference/runtimes/node-compat/) for
supported Node versions and what each support status means.

## API families

| API family | Support | Covered surface |
| --- | --- | --- |
| Invocation lifecycle | Supported in process | Module load once, repeat invocation, awaited async completion, dangling-promise diagnostics, warm-isolate semantics |
| Module system | Supported in process | ESM, CommonJS, `node:` specifiers, bare built-in specifiers, package exports, conditional exports |
| Package loading | Supported in process | Pinned npm dependencies, transitive built-ins, dynamic import where supported, deterministic bundling diagnostics |
| Web and HTTP APIs | Supported in process | `fetch`, `Request`, `Response`, `Headers`, `URL`, Web Crypto, streams, `AbortController`, timers |
| Node built-ins for app code | Supported in process | `buffer`, `events`, `util`, `crypto`, `stream`, `path`, `url`, `querystring`, `zlib`, `assert`, `diagnostics_channel`, selected `fs/promises`, selected `os`/process metadata |
| Networking | Supported in process | Outbound HTTP(S), DNS, TLS, request timeouts, abort behavior; no ambient production listen grants |
| Environment and secrets | Supported in process | Explicit environment access, secret injection, clear `process.env` semantics; no ambient `NODE_TLS_REJECT_UNAUTHORIZED` |
| Observability | Supported in process | `console`, structured logs, error stacks, unhandled-rejection diagnostics, source-map posture, invocation IDs |
| Convex integration | Supported in process | `"use node"` action files, generated server APIs, `ctx.runQuery`, `ctx.runMutation`, `ctx.runAction` where intended, scheduler interaction, value serialization, Node package action deployment |
| AI and SaaS SDKs | Supported in process | OpenAI, Anthropic, Vercel AI SDK, Stripe, Resend, AWS SDK v3, Slack, Octokit, Jose, Zod, uuid, nanoid, database HTTP clients |
| Host-heavy behavior | Service/microVM required | Child processes, worker threads, inspector, REPL, `node --test`, native addons, raw TCP servers, Unix sockets, FFI, persistent writable filesystem |

## Host-heavy boundary

Child processes, worker threads, inspector, REPL, `node --test`, native
addons, persistent host filesystem assumptions, and raw listen behavior are
denied in the in-process runtime and require a service or microVM route. The
denials are diagnostic-tested: a passing diagnostic means Nimbus proved the
denial or routing behavior, not that the API is supported in process.

## Related reference

- [Node compatibility contract](/reference/runtimes/node-compat/) —
  supported Node versions, support vocabulary, and evidence summary.
- [Node package reference](/reference/runtimes/packages/) — the per-package
  support matrix.
