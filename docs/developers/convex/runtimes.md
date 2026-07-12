---
title: The two Convex runtimes
description: How Nimbus runs Convex functions — a deterministic default runtime and a Node-compatible runtime for "use node" actions — and the exact time, randomness, fetch, and environment semantics of each.
sidebar:
  order: 4
---

A Convex function runs on one of two runtimes, chosen per module by the
presence of a `"use node";` directive on the first line. Both are V8: there
is no separate Node process. The difference is the surface each exposes and,
on the default runtime, a set of determinism rules that match Convex's own.

- **Default runtime** — a web-standard V8 isolate with `fetch`,
  `TextEncoder`, `crypto.subtle`, and the other web globals, but no Node
  builtins. This is where `query`, `mutation`, `httpAction`, and directive-free
  `action` modules run.
- **Node-compatible runtime** — selected by `"use node";` at the top of a
  module. It is node-compat running on V8, not a spawned Node binary, and it
  reaches the Node builtin surface (`node:crypto`, `node:async_hooks`, and so
  on). A `"use node"` module may contain **only** actions; put shared queries
  and mutations in a directive-free module and call them with `ctx.runQuery` /
  `ctx.runMutation`.

The [`runtimes` example app](/developers/convex/examples/) runs the same
computation on both and compares the results.

## Determinism on the default runtime

Convex makes queries and mutations deterministic so they can be replayed and
cached. Nimbus's default runtime reproduces that behavior: time and randomness
are pinned per invocation rather than reading a live clock or entropy source.

### Randomness

`Math.random()` is a seeded PRNG (a ChaCha20 stream), not the platform's
random source. It is seeded two different ways depending on when it runs:

- **At module load**, it is seeded from the deployment's stamp, so any
  `Math.random()` value computed at module scope is stable across runs and
  server restarts of the same deployment.
- **At the start of each `query`, paginated query, or `mutation`**, it is
  re-seeded with fresh per-invocation entropy, so two calls within one handler
  differ, but the handler as a whole starts from a fresh stream each time.

### Time

`Date.now()` and `new Date()` are frozen at the start of a `query`, paginated
query, or `mutation` and stay constant for the whole handler — including across
`await` points. During module evaluation they read the deployment timestamp.
Actions are exempt: an action sees a live (side-channel-coarsened) host clock.

`performance.now()` is fixed during import and inside queries, and increments
inside mutations and actions. `performance.timeOrigin` is pinned to the
deployment timestamp for every invocation kind.

### Where the deployment timestamp comes from

The deployment stamp that anchors module-scope time and `timeOrigin` is
derived from the modification time of the deployed bundle's entrypoint file (a
deploy rewrites that file), falling back to first-observation time if the mtime
is unreadable. The seed for module-load randomness is the bundle's SHA-256.
Both are stable for a given deployed bundle, which is what keeps import-time
values steady across restarts.

This is a proxy, not a recorded deploy event: copying or restoring a bundle in
a way that changes the entrypoint's mtime will shift the deployment timestamp
those functions observe.

### It is parity, not a sandbox boundary

These rules reproduce Convex's observable behavior. They are not a security
control — a function on the default runtime shares a trust domain with its own
seed, so determinism is behavioral parity, not isolation. Treat it as a
compatibility feature, not a confinement one.

## `fetch`

`fetch` is available in **actions only**. Calling it from a query or mutation
fails closed with the same message Convex uses:

```text
Can't use fetch() in queries and mutations. Please consider using an action.
```

An action that passes this check is still subject to the tenant's egress
policy, which is deny-by-default. Passing the query/mutation gate does not by
itself authorize a network destination — the egress policy decides that
separately.

## `process.env`

The default runtime exposes `process.env` and nothing else on `process`: no
`versions`, `release`, `cwd`, or `Buffer`. It is not a Node environment. Reads
go through the same capability-gated environment proxy the Node lanes use, so a
variable that has not been granted to the runtime is denied rather than silently
returning `undefined`.

## Async context

`node:async_hooks` is available on the default runtime, but only its
`AsyncLocalStorage` and `AsyncResource` classes (the bare `async_hooks`
specifier is accepted too). It is not the full Node surface — there is no
`createHook` or `executionAsyncId` machinery. The implementation is backed by
V8's async-context frame directly.

There is one caveat to know: an `AsyncLocalStorage` store does **not** propagate
into `ctx.runQuery` / `ctx.runMutation` / `ctx.runAction`. Those host calls run
detached from the invocation's async context, so a store you set with `.run(...)`
is not visible inside the function they invoke. Pass the value explicitly as an
argument instead.

## WebAssembly

The `WebAssembly` API is available — `WebAssembly.instantiate`,
`WebAssembly.Module`, and `WebAssembly.Instance` all work. Shared WebAssembly
memory is disabled as a side-channel hardening measure: constructing a
`WebAssembly.Memory` with `shared: true` throws.

## The global surface is a superset

Nimbus's default runtime seeds a `WebSocket` global plus `URL`,
`structuredClone`, and the timer functions (`setTimeout` and friends) on top of
the web-standard scope. This is a **superset** of the globals Convex's own
default runtime documents. Code that assumes a particular global is *absent* in
order to detect the runtime it is on may therefore behave differently here than
on Convex Cloud; code that only uses globals it expects to be present is
unaffected.

## Related pages

- [Migrate a Convex app to Nimbus](/developers/convex/migrate/) — what carries
  over and what to check first.
- [Convex API compatibility](/reference/convex/compatibility/) — the full
  supported-surface matrix, including the runtime semantics summarized here.
- [Convex example apps](/developers/convex/examples/) — runnable apps,
  including the two-runtime `runtimes` showcase.
