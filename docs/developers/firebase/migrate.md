---
title: Migrate from Firebase
description: Move a Firestore-backed JavaScript app onto Nimbus by swapping imports to @nimbus/firebase and porting Security Rules intent into application auth.
sidebar:
  order: 2
---

Move a Firestore-backed JavaScript app onto Nimbus while keeping your data
model, query shapes, and helper names. The migration target is the
first-party `@nimbus/firebase` package — not the stock `firebase/firestore`
browser package, which depends on WebChannel transport that Nimbus
intentionally defers.

Before you start, read [Use Firestore SDKs with Nimbus](/developers/firebase/)
for the connection model, and keep the
[compatibility matrix](/reference/firebase/compatibility/) open as the
precise support reference.

## 1. Provision the SDK

```bash
nimbus packages provision firebase
```

The `nimbus` binary materializes `@nimbus/firebase` under `.nimbus/packages/`
— no registry access required.

## 2. Swap your imports

| Firebase today | Nimbus migration target |
| --- | --- |
| `firebase/app` | `@nimbus/firebase/app` |
| `firebase/firestore` | `@nimbus/firebase/firestore` |

Common Firestore helpers keep the same names on the Nimbus package:

- `initializeApp`
- `getFirestore`, `initializeFirestore`, `connectFirestoreEmulator`
- `collection`, `doc`, `collectionGroup`
- `getDoc`, `getDocs`, `addDoc`, `setDoc`, `updateDoc`, `deleteDoc`
- `query`, `where`, `orderBy`, `limit`, cursors, `documentId`
- `onSnapshot`
- `writeBatch`
- `runTransaction`
- `deleteField`, `serverTimestamp`, `increment`, `arrayUnion`, `arrayRemove`

## 3. Point the SDK at Nimbus

For local work, redirect the SDK with `connectFirestoreEmulator(...)`:

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

The `projectId` maps directly to a Nimbus tenant id, and only the
`(default)` database is supported. The Firestore-compatible routes are
enabled per server deployment — see the availability note in
[Use Firestore SDKs with Nimbus](/developers/firebase/).

## 4. Keep REST first, opt into gRPC-Web deliberately

Unary calls default to REST, the broadest browser-safe baseline. Confirm
CRUD, query, and watch parity on REST before opting any client into
gRPC-Web:

```typescript
import { initializeFirestore } from "@nimbus/firebase/firestore";

const db = initializeFirestore(app, {
  experimentalUnaryTransport: "grpc-web",
});
connectFirestoreEmulator(db, "127.0.0.1", 8080);
```

`onSnapshot(...)` never uses gRPC-Web; it always runs over the binary
[WebSocket Listen channel](/reference/firebase/websocket-listen/). In Node or
other environments without a global `WebSocket`, supply an
`experimentalWebSocketFactory` in the Firestore settings.

## 5. Migrate transactions, batches, and field transforms

`writeBatch`, `runTransaction`, and the `FieldValue` sentinels
(`deleteField`, `serverTimestamp`, `increment`, `arrayUnion`, `arrayRemove`)
are supported and keep their Firestore semantics — batches commit
atomically, and transactions cover point reads, query reads, staged writes,
bounded retries, and rollback. Migrate this code unchanged, then verify the
flows against your Nimbus endpoint.

## 6. Port Security Rules intent into application auth

Nimbus does not implement the Firestore Security Rules DSL. Migrate the
**intent** of your rules into application-level auth and authorization
checks instead of copying rules text:

| Firestore rules pattern | Nimbus migration direction |
| --- | --- |
| `request.auth != null` | Require authenticated callers before serving the read or write path. |
| `request.auth.uid == resource.data.ownerId` | Persist owner identity in the document and enforce ownership in your server-side authorization checks. |
| `request.resource.data.ownerId == request.auth.uid` | Validate write input before commit so callers cannot claim another owner's identity. |
| Role or claim checks on `request.auth.token.*` | Map the same claims into your auth context and check them in the application layer. |

Bearer tokens sent by `@nimbus/firebase` resolve into an application
principal on the covered read, write, transaction, and listener paths — see
[Firebase auth](/reference/firebase/auth/) for exactly which inputs
authenticate and which require server-side opt-in.

## 7. Verify against the boundaries

Do not assume any of the following during migration:

- stock `firebase/firestore` browser drop-in
- Node Admin SDK (`firebase-admin`) parity
- mobile or native SDK parity
- named databases (only `(default)` is accepted)
- browser offline persistence, bundles, or `namedQuery`
- Firebase Emulator Suite control endpoints
- a Firestore Security Rules engine

These are intentional, documented boundaries — the full list with status
labels is in the [compatibility matrix](/reference/firebase/compatibility/).

## Suggested order

1. Move imports to `@nimbus/firebase`.
2. Redirect local development with `connectFirestoreEmulator(...)`.
3. Keep REST unary first and confirm CRUD/query/watch parity.
4. Migrate transactions, write batches, and `FieldValue` usage.
5. Port Security Rules intent into application auth checks.
6. Only then evaluate gRPC-Web unary transport for clients that benefit.
