---
title: Firestore example apps
description: Runnable Firestore apps on Nimbus — a browser playground and the shared tasks list, both using stock firebase/app and firebase/firestore imports.
sidebar:
  label: Examples
  order: 3
---

Runnable Firestore apps that build on Nimbus with stock `firebase/app` and
`firebase/firestore` imports. Unary calls go over REST or gRPC-Web, and live
queries use Nimbus's WebSocket `Listen` bridge. Read the
[overview](/developers/firebase/) first if you have not pointed a Firestore app
at Nimbus yet.

The apps live under
[`examples/firebase/`](https://github.com/nimbus/nimbus/tree/main/examples/firebase)
in the source repository. Run them in place from a checkout — each imports the
repository's workspace packages.

## Run any example

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. The Firestore project id is `demo`, which maps to the same-named Nimbus
tenant. Each app also has standalone `firebase:server:*` and `firebase:example:*`
npm scripts, listed in its README.

## The apps

- **[Browser (`html`)](https://github.com/nimbus/nimbus/tree/main/examples/firebase/html)**
  — a browser playground served by the Nimbus-provisioned `firebase` package.
  It exercises `connectFirestoreEmulator`, `addDoc`, `getDocs`, `onSnapshot`,
  `writeBatch`, `runTransaction`, `deleteDoc`, and the supported `FieldValue`
  sentinels, and can switch unary calls between REST and gRPC-Web.
- **[Tasks](https://github.com/nimbus/nimbus/tree/main/examples/firebase/tasks)**
  — the shared [tasks](https://github.com/nimbus/nimbus/blob/main/examples/specs/tasks.md)
  task list. Firestore CRUD calls create, toggle, and delete tasks; an
  `onSnapshot` query keeps the newest-first list current without polling,
  delivered through the Listen bridge. It implements the full spec.
