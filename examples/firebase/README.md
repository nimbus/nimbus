# Firebase / Firestore examples

Nimbus serves the Firestore surface, so stock `firebase/app` +
`firebase/firestore` imports work against a local Nimbus server. Unary calls go
over REST or gRPC-Web; live queries use the documented WebSocket `Listen`
bridge.

Docs: [Firestore](../../docs/developers/firebase/index.md).

## Examples

- **[`html/`](html/)** — a browser app using stock `firebase/firestore`
  imports, served by the Nimbus-provisioned `firebase` package. It exercises
  `connectFirestoreEmulator`, `addDoc`, `getDocs`, `onSnapshot`, `writeBatch`,
  `runTransaction`, `deleteDoc`, and the supported `FieldValue` sentinels, and
  can switch unary calls between REST and gRPC-Web.
- **[`tasks/`](tasks/)** — the shared [`tasks`](../specs/tasks.md) app: stock
  `firebase/app` + `firebase/firestore` CRUD plus a live `onSnapshot`
  subscription.

## `tasks` spec support

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `tasks.create` | yes | Inserted tasks are retrievable with a stable id and `createdAt`. |
| `tasks.list` | yes | Tasks are returned newest-first by `createdAt`. |
| `tasks.toggle` | yes | Toggling persists `completed`. |
| `tasks.delete` | yes | Deleting removes the task from subsequent reads. |
| `tasks.live-update` | yes (`onSnapshot`) | A listener attached before `tasks.create` delivers the new task with no explicit re-read. |

Full [`tasks`](../specs/tasks.md) spec. See [`tasks/README.md`](tasks/README.md).

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. For the standalone dev server, run `npm run firebase:server:html` and
`npm run firebase:example:html`.
