---
title: Use Firestore SDKs with Nimbus
description: Point Firestore-style apps at Nimbus with the provisioned drop-in firebase package — host configuration, project-to-tenant mapping, and the supported operations.
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

The Firestore-compatible routes are switched on per server: start yours
with `nimbus start --firestore` (embedders call
`ServeOptions::with_firebase_config`). The steps below are the supported
client contract against a Nimbus endpoint with the Firestore surface
enabled.

## 1. Wire the dependency

The `nimbus` binary materializes its `firebase` package locally — nothing
is fetched from a registry:

```bash
nimbus packages provision firebase
npm pkg set dependencies.firebase=file:./.nimbus/packages/firebase
npm install
```

The package lands under `.nimbus/packages/firebase` in your app directory,
and the `file:` dependency makes every stock `firebase/app` and
`firebase/firestore` import resolve to it.

## 2. Initialize and connect

```typescript
import { initializeApp } from "firebase/app";
import {
  connectFirestoreEmulator,
  getFirestore,
} from "firebase/firestore";

const app = initializeApp({ projectId: "demo" });
const db = getFirestore(app);
connectFirestoreEmulator(db, "127.0.0.1", 8080);
```

`connectFirestoreEmulator` redirects the SDK to a local host the same way it
does against the Firebase emulator: it is host redirection, not Firebase
Emulator Suite control-plane parity.

Two mapping rules matter here:

- **Project is tenant.** The Firestore `projectId` maps directly to a Nimbus
  tenant id. The tenant must exist on the server — see the
  [self-host quickstart](/get-started/self-host/) for creating one.
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
