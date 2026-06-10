---
title: Node package reference
description: The package-support matrix for Node action modules, by support boundary.
sidebar:
  order: 3
---

Package and framework support for Node action modules, verified by canaries
— real packages exercised against the supported Node versions. Rows in the
host-heavy table prove an intentional denial or service route; they are not
in-process package support. See the
[Node compatibility contract](/reference/runtimes/node-compat/) for what the
support statuses mean.

## Package classes

| Package class | Support |
| --- | --- |
| Convex `"use node"` actions | Supported in process |
| HTTP SaaS SDKs | Supported in process |
| Framework networking packages | Supported in process |
| Local tooling packages | Supported for local development only |
| Native addon packages | Service/microVM required |
| Subprocess tooling packages | Service/microVM required |
| Raw server/listen behavior | Service/microVM required |

## Supported application packages

Verified in process with passing application canaries on the listed Node
versions:

| Package / scenario | Verified Node versions |
| --- | --- |
| Node platform built-ins | Node22, Node24 |
| `express` | Node20, Node22, Node24 |
| `fastify` | Node20, Node22, Node24 |
| `socket.io` | Node22, Node24 |
| `undici` | Node22, Node24 |
| `axios` | Node22, Node24 |
| Convex `"use node"` action packaging | Node22, Node24 |
| Convex `"use node"` real-app scenario | Node22, Node24, Node26 |
| `openai` | Node22, Node24, Node26 |
| `@anthropic-ai/sdk` | Node22, Node24, Node26 |
| `ai` (Vercel AI SDK) | Node22, Node24, Node26 |
| `stripe` | Node22, Node24, Node26 |
| `resend` | Node22, Node24, Node26 |
| `@aws-sdk/client-s3` | Node22, Node24, Node26 |
| `@slack/web-api` | Node22, Node24, Node26 |
| `octokit` | Node22, Node24, Node26 |
| `jose` | Node22, Node24, Node26 |
| `zod` | Node22, Node24, Node26 |
| `uuid` | Node22, Node24, Node26 |
| `nanoid` | Node22, Node24, Node26 |
| `@upstash/redis` | Node22, Node24, Node26 |

## Host-heavy boundaries (service/microVM required)

Diagnostic-verified denials or service routes on Node22, Node24, and Node26.
A passing diagnostic proves the boundary produces a clear error, not that
the package or API works in process:

| Package / scenario | Boundary |
| --- | --- |
| `node:child_process` | Denied in process |
| `node:worker_threads` | Denied in process |
| `node:inspector` | Denied in process |
| `node:repl` | Denied in process |
| `node --test` | Denied in process |
| Native addons | Denied in process |
| Persistent writable filesystem | Denied in process |
| Raw server listen | Denied in process |
| `prisma` (query engine) | Engine boundary routed |
| `sharp` | Native binary routed |
| `esbuild` | Package binary routed |

## Tooling packages (local development / build-time)

Verified on Node22 and Node24 as local-development or build-time support.
These are not in-process production support claims:

| Package | Role |
| --- | --- |
| `tsx` | Loader |
| `ts-node` | Loader |
| `jest` | Test runner |
| `prisma` | CLI tooling |
| `next` | Build tooling |

## How to read this matrix

- HTTP SDKs and Convex-compatible `"use node"` actions are supported only
  where application canaries pass on Node22 and Node24, the supported LTS
  versions.
- Node26 results are Current/non-LTS evidence, reported separately from
  supported-LTS claims.
- Tooling packages are local-development or build-time evidence unless a row
  explicitly says application support.
- Native addons, package-owned binaries, child-process tools, and raw server
  listeners require service/microVM routing in production. See
  [packages and bundling](/developers/runtimes/nodejs/packages-and-bundling/)
  for how to configure packages.
