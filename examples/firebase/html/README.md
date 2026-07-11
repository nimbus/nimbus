# Firebase / Firestore — browser (`html`)

A browser app using stock `firebase/app` + `firebase/firestore` imports against
a local Nimbus server, served by the Nimbus-provisioned `firebase` package. It
exercises `connectFirestoreEmulator`, `addDoc`, `getDocs`, `onSnapshot`,
`writeBatch`, `runTransaction`, `deleteDoc`, and the supported `FieldValue`
sentinels, and can switch unary calls between REST and gRPC-Web.

## Run

```bash
npm run firebase:server:html   # support server
npm run firebase:example:html  # Vite dev server for this app
```

Run in place from a checkout of this repository (it imports this repo's
workspace packages).

## Spec

This app is a per-surface Firestore demo, not the shared `tasks` example. See
the Firebase [`tasks` spec support](../README.md#tasks-spec-support) and the
[`tasks` spec](../../specs/tasks.md), whose subset table is the target the
forthcoming `tasks` app will meet.
