---
title: The two Convex runtimes
description: How Nimbus runs Convex functions — a deterministic default runtime and a Node-compatible runtime for "use node" actions — and the exact time, randomness, fetch, and environment semantics of each.
sidebar:
  order: 4
---

A Convex function runs on one of two runtimes. Nimbus selects the runtime for
each module. A `"use node";` directive on the first line selects the
Node-compatible runtime. Both runtimes use V8, with no separate Node process.
They expose different surfaces. The default runtime also applies determinism
rules that match Convex.

- **Default runtime:** a web-standard V8 isolate with `fetch`,
  `TextEncoder`, `crypto.subtle`, and the other web globals, but no Node
  builtins. This is where `query`, `mutation`, `httpAction`, and directive-free
  `action` modules run.
- **Node-compatible runtime:** select this runtime with `"use node";` at the
  top of a module. It runs node-compat on V8, not a spawned Node binary. It
  reaches Node builtins such as `node:crypto` and `node:async_hooks`. A
  `"use node"` module may contain **only** actions. Put shared queries and
  mutations in a directive-free module. Call them with `ctx.runQuery` or
  `ctx.runMutation`.

The [`runtimes` example app](/developers/convex/examples/) runs the same
computation on both and compares the results.

## Determinism on the default runtime

Convex makes queries and mutations deterministic for replay and caching.
Nimbus's default runtime reproduces that behavior. It pins time and randomness
for each invocation instead of reading a live clock or entropy source.

### Randomness

`Math.random()` is a seeded pseudorandom number generator (PRNG). It uses a
ChaCha20 stream instead of the platform's random source. Nimbus uses two seed
methods:

- **At module load**, Nimbus seeds it from the deployment stamp. A
  module-scope `Math.random()` value stays stable across runs and server
  restarts of the same deployment.
- **At each `query`, paginated query, or `mutation` start**, Nimbus seeds it
  with fresh per-invocation entropy. Two calls in one handler differ. Each new
  handler starts with a fresh stream.

### Time

Nimbus freezes `Date.now()` and `new Date()` at each `query`, paginated query,
or `mutation` start. They stay constant for the whole handler, including
across `await` points. During module evaluation, they read the deployment
timestamp. Actions are exempt and see a live, side-channel-coarsened host
clock.

Nimbus fixes `performance.now()` during import and inside queries. It
increments inside mutations and actions. Nimbus pins `performance.timeOrigin`
to the deployment timestamp for every invocation kind.

### Where the deployment timestamp comes from

Nimbus derives the deployment stamp from the deployed bundle entrypoint's
modification time. A deployment rewrites that file. If Nimbus cannot read the
modification time, it uses the first-observation time. The bundle SHA-256 seeds
module-load randomness. Both values stay stable for a given deployed bundle.
This stability keeps import-time values steady across restarts.

The modification time is a proxy, not a recorded deploy event. Copying or
restoring a bundle can change the entrypoint modification time. Such a change
shifts the deployment timestamp that functions observe.

### It is parity, not a sandbox boundary

These rules reproduce Convex's observable behavior. They are not a security
control. A default-runtime function shares a trust domain with its own seed.
Determinism provides behavioral parity, not isolation. Treat it as a
compatibility feature, not a confinement feature.

## `fetch`

`fetch` is available in **actions only**. Calling it from a query or mutation
fails closed with the same message Convex uses:

```text
Can't use fetch() in queries and mutations. Please consider using an action.
```

The tenant's deny-by-default egress policy still applies to an action that
passes this check. The query or mutation gate does not authorize a network
destination. The egress policy decides that separately.

## `process.env`

The default runtime exposes only `process.env` on `process`. It does not expose
`versions`, `release`, `cwd`, or `Buffer`. It is not a Node environment. Reads
use the same capability-gated environment proxy as the Node lanes. Nimbus
denies access to variables without runtime grants instead of returning
`undefined`.

## Async context

The default runtime provides `node:async_hooks`, but only with
`AsyncLocalStorage` and `AsyncResource`. It also accepts the bare `async_hooks`
specifier. This is not the full Node surface. Nimbus does not provide
`createHook` or `executionAsyncId`. V8's async-context frame backs the
implementation directly.

An `AsyncLocalStorage` store does **not** propagate into `ctx.runQuery`,
`ctx.runMutation`, or `ctx.runAction`. Those host calls run outside the
invocation's async context. The called function cannot see a store that you set
with `.run(...)`. Pass the value as an explicit argument.

## WebAssembly

The `WebAssembly` API provides `WebAssembly.instantiate`,
`WebAssembly.Module`, and `WebAssembly.Instance`. Nimbus disables shared
WebAssembly memory to reduce side channels. A `WebAssembly.Memory` constructor
with `shared: true` throws.

## The global surface is a superset

Nimbus's default runtime adds several globals to the web-standard scope. They
include `WebSocket`, `URL`, `structuredClone`, and timer functions such as
`setTimeout`. This is a **superset** of the globals that Convex documents for
its default runtime. Code can behave differently if it detects a runtime by an
absent global. Code that uses only expected globals behaves the same.

## Related pages

- [Migrate a Convex app to Nimbus](/developers/convex/migrate/): what carries
  over and what to check first.
- [Convex API compatibility](/reference/convex/compatibility/): the full
  supported-surface matrix, including the runtime semantics summarized here.
- [Convex example apps](/developers/convex/examples/): runnable apps,
  including the two-runtime `runtimes` demo.
