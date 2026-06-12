---
title: Use Firestore SDKs with Nimbus
description: Point Firestore-style apps at Nimbus with the drop-in firebase package — nimbus dev wires it automatically; host configuration, project-to-tenant mapping, and the supported operations.
sidebar:
  label: Overview
  order: 1
---

Nimbus speaks the Firestore wire protocol — REST, gRPC-Web, and a WebSocket
`Listen` channel for live queries — and ships a first-party drop-in
`firebase` package that mirrors the modular `firebase/app` and
`firebase/firestore` API. Your imports, data model, query shapes, and
helper names stay unchanged; the `nimbus` binary provisions the package
locally and a `file:` dependency points `firebase` at it.

The supported client is the Nimbus-provisioned `firebase` package, not the
registry-published Google package. The two share import paths and API
shapes, but the upstream browser SDK transports over WebChannel, which
Nimbus does not implement. See the
[Firestore compatibility matrix](/reference/firebase/compatibility/) for
the precise surface.

## Before you start

Every Nimbus server serves the Firestore-compatible routes: `nimbus dev`
and `nimbus start` both have them on by default, and
`nimbus start --no-firestore` switches them off (embedders call
`ServeOptions::with_firebase_config`). The steps below are the supported
client contract against a Nimbus endpoint serving the Firestore surface.

## 1. Wire the dependency

In your app directory, one command does the wiring:

```bash
nimbus dev
```

`nimbus dev` detects the `firebase` dependency in `package.json`, scans
your sources to confirm every Firebase import is on the supported
surface, and only then rewires the dependency at the drop-in package. If
a file imports an uncovered surface (say `firebase/auth`), the scan
refuses with the file, line, and import named — and your app is left
untouched.

To wire the dependency without a dev session — say, against a separate
`nimbus start` server — provision it directly:

```bash
# in your existing app directory
nimbus packages provision firebase
npm install
```

Either way, nothing is fetched from a registry: the package lands under
`.nimbus/packages/firebase` in your app directory, and
`dependencies.firebase` in your `package.json` is rewired to
`file:./.nimbus/packages/firebase` — replacing a registry spec if one is
there. Every stock `firebase/app` and `firebase/firestore` import then
resolves to the provisioned package.

## 2. Initialize and connect

```typescript
import { initializeApp } from "firebase/app";
import {
  connectFirestoreEmulator,
  getFirestore,
} from "firebase/firestore";

const app = initializeApp({ projectId: "demo" });
const db = getFirestore(app);
connectFirestoreEmulator(db, "127.0.0.1", 3210);
```

`connectFirestoreEmulator` redirects the SDK to a local host the same way it
does against the Firebase emulator: it is host redirection, not Firebase
Emulator Suite control-plane parity. Use the port your server listens on —
`nimbus dev` serves on `3210`, `nimbus start` defaults to `8080`.

Two mapping rules matter here:

- **Project is tenant.** The Firestore `projectId` maps directly to a Nimbus
  tenant id. `nimbus dev` discovers your project id (from `.firebaserc`'s
  default project, falling back to a `projectId` literal in your sources)
  and creates the tenant automatically. On a self-hosted server the tenant
  must exist — see the [self-host quickstart](/get-started/self-host/) for
  creating one.
- **Default database only.** Only the `(default)` Firestore database is
  accepted; named databases are rejected.

## 3. Write and read

```typescript
import {
  addDoc,
  collection,
  getDocs,
  onSnapshot,
} from "firebase/firestore";

const messages = collection(db, "messages");

await addDoc(messages, {
  body: "hello from nimbus",
  createdAt: new Date().toISOString(),
});

const snapshot = await getDocs(messages);
console.log(snapshot.docs.map((doc) => doc.data()));

const unsubscribe = onSnapshot(messages, (live) => {
  console.log("live size", live.size);
});
```

## Transports

Transport behavior is explicit rather than auto-negotiated:

- Unary calls (reads, writes, queries) use **REST by default**.
- gRPC-Web unary is available by opting in:

  ```typescript
  import { initializeFirestore } from "firebase/firestore";

  const db = initializeFirestore(app, {
    experimentalUnaryTransport: "grpc-web",
  });
  ```

- `onSnapshot` listeners always use the binary-protobuf
  [WebSocket Listen channel](/reference/firebase/websocket-listen/) — never
  WebChannel and never long polling.
- In environments without a global `WebSocket`, pass an
  `experimentalWebSocketFactory` in the Firestore settings so listeners can
  open the watch connection.

## Supported operations at a glance

- **Bootstrap:** `initializeApp`, `getFirestore`, `initializeFirestore`,
  `connectFirestoreEmulator`, `terminate`
- **References:** `collection`, `doc`, `collectionGroup`, `documentId`
- **CRUD:** `getDoc`, `setDoc`, `updateDoc`, `deleteDoc`, `addDoc`
- **Queries:** `query`, `where`, `orderBy`, `limit`, `startAt`, `startAfter`,
  `endAt`, `endBefore`, `getDocs`
- **Live queries:** `onSnapshot`
- **Atomicity:** `writeBatch`, `runTransaction`
- **Field transforms:** `deleteField`, `serverTimestamp`, `increment`,
  `arrayUnion`, `arrayRemove`
- **Equality helpers:** `refEqual`, `queryEqual`, `snapshotEqual`

For status labels, caveats, and the boundaries that are intentionally not
covered, see the [compatibility matrix](/reference/firebase/compatibility/).

## Where next

- [Migrate from Firebase](/developers/firebase/migrate/) — move an existing
  Firestore app onto Nimbus step by step.
- [Firestore compatibility](/reference/firebase/compatibility/) — the precise
  support matrix.
- [Firebase auth](/reference/firebase/auth/) — how bearer tokens and emulator
  mock user tokens authenticate.
- [WebSocket Listen](/reference/firebase/websocket-listen/) — the live-query
  transport contract.
