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

## `tasks` spec support

| Create / List / Toggle / Delete | Live view |
| --- | --- |
| yes | yes (WebSocket subscription) |

Full [`tasks`](../specs/tasks.md) spec: CRUD plus a live view that updates
without polling.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. For the standalone Vite dev server, run `npm run nimbus:demo:html`.
