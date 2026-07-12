# Convex runtimes

A minimal app that runs the same SHA-256 digest computation twice, once on
each of Nimbus's two Convex action runtimes, and writes both results to a
shared `digests` table so they can be compared directly:

- **`convex/digests.ts`** has no directive at the top of the file, so it runs
  on Nimbus's **default runtime** — a V8-based, web-standard isolate with
  `fetch`, `TextEncoder`, and `crypto.subtle`, but no Node builtins.
  `hashWithDefaultRuntime` hashes its input with `crypto.subtle.digest`.
- **`convex/nodeDigests.ts`** starts with `"use node";` as the literal first
  line, so it runs on Nimbus's **Node-compatible runtime**, with full access
  to Node builtins. `hashWithNodeRuntime` hashes the same input with
  `node:crypto`'s `createHash`. A `"use node"` module may contain only
  actions — no queries or mutations, which is why the shared `store` mutation
  lives in `digests.ts` and both actions call it via `ctx.runMutation`.

Both actions write to the same table through the same internal mutation, and
`digests.list` reads rows written by either runtime back out. The smoke test
computes its own expected digest independently and asserts both runtimes
agree with it — proving the two runtimes don't just each return *some* hex
string, but the identical, correct one.

- **`convex/shareIds.ts`** has no directive either, so it also runs on the
  **default runtime** — but it imports `nanoid`, a real, browser-compatible
  npm package (no Node builtins, works in any JS runtime), directly into a
  default-runtime function. This is a separate story from `"use node"`
  external packages: any third-party package works from *either* runtime
  once listed in `convex.json`'s `node.externalPackages` (this app lists
  `nanoid` there), not just from `"use node"` modules. `shareIds.create`
  generates and persists an id with `nanoid()`; the smoke test asserts the
  stored id has nanoid's default shape (21 URL-safe characters).

> **⚠️ Monorepo-only — do not copy this directory out of the repo yet.**
> This app depends on `"convex": "file:./.nimbus/packages/convex"` — a local
> path dependency on Nimbus's Convex compatibility package, staged into the
> gitignored `.nimbus/packages/convex` directory by `nimbus`'s provisioning
> step the first time you run `nimbus dev` or `nimbus deploy` in this
> directory. That staged package is what gives you the official `convex` name
> and `convex` bin, unmodified, running Nimbus's implementation underneath.
>
> Copy this directory out of the monorepo on its own and `npm install`, and
> there is no `.nimbus/packages/convex` to stage — the `file:` path doesn't
> resolve. `npm install` still exits successfully, but it leaves
> `node_modules/convex` as a dangling symlink, which `npm ls` reports as
> `invalid`. Every script that touches `convex` after that — `codegen`,
> `typecheck`, `smoke` — fails with a module-resolution error (`Cannot find
> module 'convex/...'`), or, if some unrelated `convex` binary happens to be on
> your `PATH`, a confusing error from that CLI instead. It is not the official
> Convex Cloud package silently taking over.
>
> Until the `nimbus init --example` scaffolder ships (it rewrites the `convex`
> dependency to a published Nimbus pin instead of a local staged path), run
> this example in place, from a checkout of this repository.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or configured target name; omit it to use the local target.

`smoke.ts` requires Node.js >=22 <25 (runs via `--experimental-strip-types`).

## Smoke verification

With Nimbus running at `http://localhost:8080`:

```bash
NIMBUS_ADMIN_TOKEN="$(nimbus auth token)" npm run smoke -w convex-runtimes
```

Set `NIMBUS_NATIVE_URL` to exercise another Nimbus URL. Set
`NIMBUS_CONVEX_URL` and `NIMBUS_TENANT_ID` together when the Convex endpoint
does not follow Nimbus's default `/convex/<tenant>` shape. The smoke prints
one `PASS` line per flow anchor: `digests.hashWithDefaultRuntime`,
`nodeDigests.hashWithNodeRuntime`, `digests.list`, and `shareIds.create`. A
server that does not require local admin authentication can omit
`NIMBUS_ADMIN_TOKEN`.
