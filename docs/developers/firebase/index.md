---
title: Use Firestore SDKs with Nimbus
description: Point Firestore-style apps at Nimbus with the @nimbus/firebase SDK — host configuration, project-to-tenant mapping, and the supported operations.
sidebar:
  label: Overview
  order: 1
---

Nimbus speaks the Firestore wire protocol — REST, gRPC-Web, and a WebSocket
`Listen` channel for live queries — and ships a first-party SDK,
`@nimbus/firebase`, that mirrors the `firebase/firestore` API. You keep your
Firestore data model, query shapes, and helper names; you swap the import
and point the SDK at a Nimbus host.

The supported client today is `@nimbus/firebase`, not the stock
`firebase/firestore` browser package. The stock browser SDK transports over
WebChannel, which Nimbus does not implement. See the
[Firestore compatibility matrix](/reference/firebase/compatibility/) for the
precise surface.

## Before you start

The Firestore-compatible routes are part of the Nimbus server, but they are
switched on per deployment by server configuration — a stock `nimbus start`
process does not serve them today, and there is no CLI flag yet that enables
them. The steps below are the supported client contract against a Nimbus
endpoint that has the Firestore surface enabled.

## 1. Provision the SDK

The `nimbus` binary materializes `@nimbus/firebase` locally — nothing is
fetched from a registry:

```bash
nimbus packages provision firebase
```

The package lands under `.nimbus/packages/` in your app directory and exports
two entry points: `@nimbus/firebase/app` and `@nimbus/firebase/firestore`.

## 2. Initialize and connect

```typescript
import { initializeApp } from "@nimbus/firebase/app";
import {
  connectFirestoreEmulator,
  getFirestore,
} from "@nimbus/firebase/firestore";

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
} from "@nimbus/firebase/firestore";

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
  import { initializeFirestore } from "@nimbus/firebase/firestore";

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
