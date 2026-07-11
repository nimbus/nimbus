# Native examples

The native surface is Nimbus's own SDK, `@nimbus/nimbus`: reactive documents
written over HTTP and read through live WebSocket subscriptions, with no
compatibility layer in between. This is the most direct way to build on Nimbus.

Docs: [Native API](../../docs/developers/native/index.md).

## Examples

- **[`html/`](html/)** — a single-page browser playground built with Vite and
  `@nimbus/nimbus/transports/rest`. It creates a tenant, installs a schema,
  inserts documents, schedules a mutation with `ctx.scheduler.runAfter(...)`,
  and watches live subscription results update in place.
- **[`tasks/`](tasks/)** — the shared [`tasks`](../specs/tasks.md) app: CRUD
  plus a live WebSocket subscription via the native SDK.
- **[`agent-chat/`](agent-chat/)** — a durable chat agent: query/mutation +
  `ctx.scheduler.runAfter` + the database, no hosted inference. See the
  [`agent-chat` spec](../specs/agent-chat.md) and
  [`agent-chat/README.md`](agent-chat/README.md).
- **[`agent-worker/`](agent-worker/)** — a headless, autonomous worker: no UI,
  `ctx.scheduler.runAfter` schedules a batch of jobs that run to completion
  entirely server-side. See [`agent-worker/README.md`](agent-worker/README.md).

## `tasks` spec support

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` | yes | Inserted tasks are retrievable with a stable id and `createdAt`. |
| `tasks.list` | yes | Tasks are returned newest-first by `createdAt`. |
| `tasks.toggle` | yes | Toggling persists `completed`. |
| `tasks.delete` | yes | Deleting removes the task from subsequent reads. |
| `tasks.live-update` | yes (WebSocket subscription) | A subscription opened before `tasks.create` delivers the new task with no explicit re-read. |

Full [`tasks`](../specs/tasks.md) spec. See [`tasks/README.md`](tasks/README.md).

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. For the standalone Vite dev server, run `npm run nimbus:demo:html`.
