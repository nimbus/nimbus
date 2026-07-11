# Convex tasks

A focused React task list authored with `convex/_generated/server`, a
`convex/schema.ts`, generated API references, and `convex/react`. Creates,
toggles, and deletes are Convex mutations; the newest-first list is a reactive
Convex query that updates without polling.

The app implements the full shared [`tasks` spec](../../specs/tasks.md).

> **⚠️ Monorepo-only — do not copy this directory out of the repo yet.**
> This app depends on `"convex": "*"`, which inside this workspace resolves to
> Nimbus's Convex compatibility package — the one that deliberately takes the
> official `convex` name and `convex` bin so your code runs unchanged. Copy the
> app out of the monorepo and `npm install`, and `"convex": "*"` instead
> resolves to the **official Convex Cloud** package from the npm registry,
> replacing Nimbus's — including its `convex` binary. What breaks then is
> visible, not silent:
>
> - The app's scripts run `convex codegen --app .`, but `--app` is a
>   Nimbus-only flag; the official `convex` CLI rejects it, so `npm run dev`,
>   `build`, `codegen`, and `smoke` fail loudly at codegen.
> - Even past that, the app does not quietly talk to Convex Cloud: the React
>   client pins `http://localhost:8080/convex/demo` (setting
>   `skipConvexDeploymentUrlCheck`), and the smoke uses its own local-server
>   default. A copied-out app keeps targeting your local Nimbus server.
>
> Until the `nimbus init --example` scaffolder ships (it rewrites the `convex`
> workspace dependency to a published Nimbus pin), run this example in place,
> from a checkout of this repository. The other adapter examples do not share
> this hazard: unpublished `@nimbus/*` workspace dependencies fail resolution
> with a visible install error, and the Firebase app's stock `firebase`
> dependency installs the real upstream SDK from the registry (the app still
> expects a Nimbus server to talk to).

## Spec subset

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` | yes | A new incomplete task has a stable id and creation time. |
| `tasks.list` | yes | Tasks render newest-first by `createdAt`. |
| `tasks.toggle` | yes | Toggling a task persists its opposite completed state. |
| `tasks.delete` | yes | Deleting a task removes it from the list. |
| `tasks.live-update` | yes | A reactive query pushes list changes without polling. |

The app is built and typechecks/builds to this spec. Live verification against
a running server is **complete**: all five flow anchors — `tasks.create`,
`tasks.list`, `tasks.toggle`, `tasks.delete`, `tasks.live-update` — have real
PASS evidence against anonymous local traffic. See the "Live verification"
note in [`../README.md`](../README.md) for the fixes that closed the
remaining anchors.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or configured target name; omit it to use the local target.
Tenant creation in browser code is a local-development convenience. Provision
tenants separately before deploying beyond your own environment.

## Smoke verification

With Nimbus running at `http://localhost:8080`:

```bash
NIMBUS_ADMIN_TOKEN="$(nimbus auth token)" npm run smoke -w convex-tasks
```

Set `NIMBUS_NATIVE_URL` to exercise another Nimbus URL. Set
`NIMBUS_CONVEX_URL` and `NIMBUS_TENANT_ID` together when the Convex endpoint
does not follow Nimbus's default `/convex/<tenant>` shape. The smoke resets the
selected tenant's tasks and prints one `PASS` line for every flow anchor, including
a real `ConvexClient.onUpdate` push for `tasks.live-update`. A server that does
not require local admin authentication can omit `NIMBUS_ADMIN_TOKEN`.
