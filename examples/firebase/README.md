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

## `tasks` spec support

| Create / List / Toggle / Delete | Live view |
| --- | --- |
| yes | yes (`onSnapshot`) |

Full [`tasks`](../specs/tasks.md) spec; the live view is delivered through the
`Listen` bridge.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. For the standalone dev server, run `npm run firebase:server:html` and
`npm run firebase:demo:html`.
