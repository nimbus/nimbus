# Nimbus tasks

A focused task list built with the native `@nimbus/nimbus` SDK. It provisions
the `demo` tenant for local development, installs the `tasks` schema, writes over
HTTP, and keeps the browser list current through a WebSocket subscription.

The app implements the full shared [`tasks` spec](../../specs/tasks.md).

## Spec subset

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` | yes | A new incomplete task has a stable id and creation time. |
| `tasks.list` | yes | Tasks render newest-first by `createdAt`. |
| `tasks.toggle` | yes | Toggling a task persists its completed state. |
| `tasks.delete` | yes | Deleting a task removes it from the list. |
| `tasks.live-update` | yes | A WebSocket subscription pushes list changes without polling. |

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or configured target name; omit it to use the local target.
Tenant creation in browser code is a local-development convenience. Provision
tenants separately before deploying beyond your own environment.

`smoke.ts` requires Node.js >=22 <25 (runs via `--experimental-strip-types`).

## Smoke verification

With Nimbus running at `http://localhost:8080`:

```bash
NIMBUS_ADMIN_TOKEN="$(nimbus auth token)" npm run smoke -w nimbus-tasks
```

Set `NIMBUS_NATIVE_URL` to exercise another Nimbus URL. The smoke creates an
isolated tenant and prints one `PASS` line for every flow anchor. A server that
does not require local admin authentication can omit `NIMBUS_ADMIN_TOKEN`.
