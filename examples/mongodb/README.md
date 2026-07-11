# MongoDB examples

Nimbus exposes a MongoDB wire-protocol listener, so the stock `mongodb` driver
connects and runs CRUD unchanged. The `@nimbus/mongodb` URI helper builds the
connection string that points the driver at Nimbus.

Docs: [MongoDB](../../docs/developers/mongodb/index.md).

## Examples

- **[`node/`](node/)** — a Node app using the `@nimbus/mongodb` URI helper with
  the stock `mongodb` driver for create, read, update, and delete against the
  Nimbus wire-protocol listener.

## `tasks` spec support

| Create / List / Toggle / Delete | Live view |
| --- | --- |
| yes | no |

Full CRUD from the [`tasks`](../specs/tasks.md) spec. The live view is **not**
supported — change streams are unavailable on this surface, so the example's
live assertion degrades to polling the list.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. The example also runs standalone with `npm run demo --workspace mongodb-node`
against the Nimbus MongoDB listener.
