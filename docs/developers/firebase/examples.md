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
in the source repository. Run them in place from a checkout. Each imports the
repository's workspace packages.

## Run any example

```bash
nimbus dev
```

`nimbus dev` starts a local server with the Firestore-compatible routes. The
Firestore project id is `demo`, which maps to the same-named Nimbus tenant.
These apps are Firestore *clients*: a browser UI plus a support server. They
have no Nimbus function bundle, so there is no `nimbus deploy` step. Each app
also has standalone npm scripts.

The `firebase:server:*` script runs the support server. The
`firebase:example:*` script runs the Vite dev server. Each README lists the
scripts. Run them in place from a checkout.

## The apps

**[Browser (`html`)](https://github.com/nimbus/nimbus/tree/main/examples/firebase/html).**
The Nimbus-provisioned `firebase` package serves this browser playground. The
app covers `connectFirestoreEmulator`, `addDoc`, `getDocs`, `onSnapshot`,
`writeBatch`, `runTransaction`, `deleteDoc`, and the supported `FieldValue`
sentinels. It can switch unary calls between REST and gRPC-Web.

**[Tasks](https://github.com/nimbus/nimbus/tree/main/examples/firebase/tasks).**
This app implements the shared
[tasks](https://github.com/nimbus/nimbus/blob/main/examples/specs/tasks.md)
task list. Firestore CRUD calls create, toggle, and delete tasks. An
`onSnapshot` query keeps the newest-first list current without polling. The
Listen bridge delivers each update. The app implements the full spec.
