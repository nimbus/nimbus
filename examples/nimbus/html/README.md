# Native — browser (`html`)

A single-page browser playground built with Vite on `@nimbus/nimbus`'s REST and
subscription transports (`@nimbus/nimbus/transports/rest`). It creates a tenant,
installs a schema, inserts documents, schedules a mutation with
`ctx.scheduler.runAfter(...)`, and watches live WebSocket subscription results
update in place.

## Run

```bash
npm run nimbus:demo:html   # Vite dev server for this app
```

Run in place from a checkout of this repository (it imports this repo's
workspace packages).

## Spec

This app is a native-SDK playground, not the shared `tasks` example. See the
native [`tasks` spec support](../README.md#tasks-spec-support) and the
[`tasks` spec](../../specs/tasks.md), whose subset table is the target the
forthcoming `tasks` app will meet.
