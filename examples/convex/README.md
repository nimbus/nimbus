# Convex examples

Nimbus speaks the Convex surface: you author functions with
`convex/_generated/server`, `convex/values`, and a `convex/schema.ts`, and run
`convex/react` and `convex/browser` clients against Nimbus unchanged. These
examples are Nimbus ports and adaptations of common Convex shapes, not the
official Convex demos running as-is.

Docs: [Convex](../../docs/developers/convex/index.md). Contributor status on the
compiled/runtime subset these apps exercise lives in [`DEVNOTES.md`](DEVNOTES.md).

> **⚠️ Monorepo-only — do not copy this directory out of the repo yet.**
> These apps depend on `"convex": "*"`, which inside this workspace resolves to
> Nimbus's Convex compatibility package — the one that deliberately takes the
> official `convex` name and `convex` bin so your code runs unchanged. Copy an
> app out of the monorepo and `npm install`, and `"convex": "*"` instead
> resolves to the **official Convex Cloud** package from the npm registry,
> replacing Nimbus's — including its `convex` binary. What breaks then is
> visible, not silent:
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
- **[`showcase/`](showcase/)** — a small app used to exercise the developer
  console's function source visibility (source view, symbol navigation, and
  type-hover). See its [README](showcase/README.md).
- **[`tasks/`](tasks/)** — the shared [`tasks`](../specs/tasks.md) app, authored
  with `convex/_generated/server` and `schema.ts` against a reactive query for
  `tasks.live-update`. Built to spec and typechecks/builds clean, but its live
  smoke is currently **blocked**, not verified — see the note below.

## `tasks` spec support

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` | yes (by spec) | Inserted tasks are retrievable with a stable id and `createdAt`. |
| `tasks.list` | yes (by spec) | Tasks are returned newest-first by `createdAt`. |
| `tasks.toggle` | yes (by spec) | Toggling persists `completed`. |
| `tasks.delete` | yes (by spec) | Deleting removes the task from subsequent reads. |
| `tasks.live-update` | yes (by spec, reactive query) | A subscription opened before `tasks.create` delivers the new task with no explicit re-read. |

**Live verification is blocked.** Every application-Convex request — including
plain anonymous local-dev traffic, which is what every example and demo script
in this directory already sends — passes through a fail-closed team-binding
gate (`crates/nimbus-convex/src/tenancy.rs`, enforced unconditionally in
`crates/nimbus-server/src/adapters/convex/handlers/registry_auth.rs`). The gate
only admits a request when the URL's silo *and* the caller's verified JWT
`subject`/`issuer` both resolve to the same team via
`NIMBUS_CONVEX_SILO_TEAMS` / `NIMBUS_CONVEX_PRINCIPAL_TEAMS`; anonymous
principals can never pass. None of today's Convex examples set those env vars
or send a signed JWT, so `tasks/`'s live smoke — and any equivalent live check
against the other Convex apps in this directory — cannot currently reach a
running server. This is a repo-wide local-dev gap, not specific to the `tasks`
app; see the EX3.2 row in
`docs/private/plans/examples-and-target-resolution-plan.md` for the tracked
evidence and resolution options. Full [`tasks`](../specs/tasks.md) spec. See
[`tasks/README.md`](tasks/README.md).

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. The individual apps also have standalone dev-server scripts —
`npm run convex:demo:html`, `convex:demo:http`, `convex:demo:node`, each paired
with the matching `npm run convex:server:*` support server.
