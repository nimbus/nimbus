# Convex examples

Nimbus speaks the Convex surface: you author functions with
`convex/_generated/server`, `convex/values`, and a `convex/schema.ts`, and run
`convex/react` and `convex/browser` clients against Nimbus unchanged. These
examples are Nimbus ports and adaptations of common Convex shapes, not the
official Convex demos running as-is.

Docs: [Convex](../../docs/developers/convex/index.md). Contributor status on the
compiled/runtime subset these apps exercise lives in [`DEVNOTES.md`](DEVNOTES.md).

> **⚠️ Monorepo-only — do not copy this directory out of the repo yet.**
> `html`, `http`, `node`, and `tasks` depend on `"convex": "*"`, which inside
> this workspace resolves to Nimbus's Convex compatibility package — the one
> that deliberately takes the official `convex` name and `convex` bin so your
> code runs unchanged. Copy one of those apps out of the monorepo and
> `npm install`, and `"convex": "*"` instead resolves to the **official Convex
> Cloud** package from the npm registry, replacing Nimbus's — including its
> `convex` binary. What breaks then is visible, not silent:
>
> - The app's scripts run `convex codegen --app .`, but `--app` is a
>   Nimbus-only flag; the official `convex` CLI rejects it, so `npm run dev`,
>   `build`, and `codegen` fail loudly at codegen.
> - Even past that, none of the apps quietly talks to Convex Cloud: the React
>   app pins `http://localhost:8080/convex/demo` (setting
>   `skipConvexDeploymentUrlCheck`), and the http and node apps use their own
>   local-server defaults — a copied-out app keeps targeting your local Nimbus
>   server.
>
> `showcase` is different: it pins `"convex": "file:./.nimbus/packages/convex"`,
> a workspace-relative local-file dependency, not `"convex": "*"`. Copy it out
> and `npm install` fails at dependency resolution — a missing local path, not
> a silent swap to Convex Cloud.
>
> Until the `nimbus init --example` scaffolder ships (it rewrites the `convex`
> workspace dependency to a published Nimbus pin), run these examples in place,
> from a checkout of this repository. The other adapter examples don't share
> this hazard: unpublished `@nimbus/*` workspace deps fail resolution with a
> visible install error, and the Firebase app's stock `firebase` dependency
> installs the real upstream SDK from the registry (the app still expects a
> Nimbus server to talk to).

## Examples

- **[`html/`](html/)** — a React app using `convex/react` and generated
  `_generated/api.ts` over Nimbus's Convex transport, exercising live inserts,
  scheduled writes, patch/delete, and paginated queries.
- **[`http/`](http/)** — a browser app using `convex/browser` and generated refs
  without React, including compiled `httpAction` routes and an action that
  delegates to an internal mutation.
- **[`node/`](node/)** — a Node app using generated refs, an injected WebSocket
  implementation, point-in-time reads, and live subscriptions.
- **[`runtimes/`](runtimes/)** — a two-runtime app: one action runs on the
  default V8-based runtime, a `"use node"` action runs on the Node-compatible
  runtime, and both write to a shared table so the results can be compared.
  See its [README](runtimes/README.md).
- **[`showcase/`](showcase/)** — a small app used to exercise the developer
  console's function source visibility (source view, symbol navigation, and
  type-hover). See its [README](showcase/README.md).
- **[`tasks/`](tasks/)** — the shared [`tasks`](../specs/tasks.md) app, authored
  with `convex/_generated/server` and `schema.ts` against a reactive query for
  `tasks.live-update`. Built to spec, typechecks/builds clean, and all five
  flow anchors have real live PASS evidence — see the note below.

## `tasks` spec support

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` | yes | Inserted tasks are retrievable with a stable id and `createdAt`. |
| `tasks.list` | yes | Tasks are returned newest-first by `createdAt`. |
| `tasks.toggle` | yes | Toggling persists `completed`. |
| `tasks.delete` | yes | Deleting removes the task from subsequent reads. |
| `tasks.live-update` | yes (reactive query) | A subscription opened before `tasks.create` delivers the new task with no explicit re-read. |

**Live verification: 5/5 anchors PASS.** The team-binding gate that used to
refuse every anonymous application-Convex request outright
(`crates/nimbus-convex/src/tenancy.rs`, enforced in
`crates/nimbus-server/src/adapters/convex/handlers/registry_auth.rs`) now has
dev-mode defaults: `nimbus dev` auto-provisions anonymous local traffic with
zero env config, and `start` accepts an explicit opt-in
(`NIMBUS_CONVEX_SILO_TEAMS`/`NIMBUS_CONVEX_PRINCIPAL_TEAMS`/`NIMBUS_CONVEX_ANONYMOUS_TEAM`).
A runtime-bridge fix (`ctx.db.get`/`patch`/`delete` now accept the public
SDK's single table-scoped-id calling convention, plus faithful compiled-plan
subscription replay) and an app-level fix (`toggle` now takes the next value
from the client instead of reading-then-negating server-side, avoiding a
generic-`{}`-typed `ctx.db.get` narrowing conflict in the compile-time
planner) together closed every remaining blocker. Confirmed live against a
real `nimbus dev` boot: `ensureTenant()`, `tasks.create`, `tasks.list`,
`tasks.toggle`, `tasks.delete`, and `tasks.live-update` all PASS anonymously.
The underlying product gap (a DataModel-typed `ctx.db` in the compat package,
or planner tolerance for JS narrowing) remains a recorded follow-up owned by
EX9.1, not a live blocker. Two unrelated, pre-existing defects are still open
and tracked as flagged follow-ups, not fixed: an intermittent boot-time
bundle-hash race, and a codegen/live-server bundle race triggered by running
client codegen against a live `nimbus dev` server for the same app. Full
detail and evidence in the EX3.2 row of
`docs/private/plans/examples-and-target-resolution-plan.md`. Full
[`tasks`](../specs/tasks.md) spec. See [`tasks/README.md`](tasks/README.md).

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. The individual apps also have standalone dev-server scripts —
`npm run convex:example:html`, `convex:example:http`, `convex:example:node`, each paired
with the matching `npm run convex:server:*` support server.
