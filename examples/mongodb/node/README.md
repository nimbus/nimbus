# MongoDB — Node (`node`)

A Node script using the stock `mongodb` driver against the Nimbus MongoDB
wire-protocol listener, with the `@nimbus/mongodb` URI helper building the
connection string. It runs create, read, update, and read-again over a
`messages` collection.

Set `NIMBUS_MONGODB_USERNAME` and `NIMBUS_MONGODB_PASSWORD` before running (the
host and port default to `127.0.0.1:27017`, overridable via
`NIMBUS_MONGODB_HOST` / `NIMBUS_MONGODB_PORT`).

## Run

```bash
npm run demo --workspace mongodb-node
```

Run in place from a checkout of this repository (it imports this repo's
workspace packages).

## Spec

This app is a per-surface CRUD demo, not the shared `tasks` example. See the
MongoDB [`tasks` spec support](../README.md#tasks-spec-support) and the
[`tasks` spec](../../specs/tasks.md), whose subset table is the target the
forthcoming `tasks` app will meet. Change streams are unavailable on this
surface, so the live view degrades to polling.
