# MongoDB examples

Nimbus exposes a MongoDB wire-protocol listener, so the stock `mongodb` driver
connects and runs CRUD unchanged. The `@nimbus/mongodb` URI helper builds the
connection string that points the driver at Nimbus.

Docs: [MongoDB](../../docs/developers/mongodb/index.md).

## Examples

- **[`node/`](node/)** — a Node app using the `@nimbus/mongodb` URI helper with
  the stock `mongodb` driver for create, read, update, and delete against the
  Nimbus wire-protocol listener.
- **[`tasks/`](tasks/)** — the shared [`tasks`](../specs/tasks.md) app: stock
  `mongodb` driver CRUD, with `tasks.live-update` satisfied by polling since
  change streams are unavailable on this surface.

## `tasks` spec support

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` | yes | A new incomplete task has a stable id and creation time. |
| `tasks.list` | yes | Tasks are returned newest-first by `createdAt`. |
| `tasks.toggle` | yes | Updating a task persists `completed: true`. |
| `tasks.delete` | yes | Deleting a task removes it from the list. |
| `tasks.live-update` | polled | Repeated `tasks.list` reads observe changes; this is polling, not a live subscription. |

MongoDB change streams are not supported by Nimbus, so this surface cannot meet
the spec's no-polling live-subscription behavior — the example records that gap
directly. Full [`tasks`](../specs/tasks.md) spec. See
[`tasks/README.md`](tasks/README.md).

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. The example also runs standalone with `npm run demo --workspace mongodb-node`
against the Nimbus MongoDB listener.
