---
title: Migrate from Firebase
description: Move a Firestore-backed JavaScript app onto Nimbus by repointing the firebase dependency at the provisioned drop-in package and porting Security Rules intent into application auth.
sidebar:
  order: 2
---

Move a Firestore-backed JavaScript app onto Nimbus while keeping your
imports, data model, query shapes, and helper names. The migration target
is Nimbus's first-party drop-in `firebase` package: it takes the stock npm
name, so `firebase/app` and `firebase/firestore` imports stay exactly as
they are — only the dependency resolution changes. The registry-published
Google package is not the target, because its browser client depends on
WebChannel transport that Nimbus intentionally defers.

Before you start, read [Use Firestore SDKs with Nimbus](/developers/firebase/)
for the connection model, and keep the
[compatibility matrix](/reference/firebase/compatibility/) open as the
precise support reference.

## 1. Repoint the `firebase` dependency

```bash
nimbus packages provision firebase
npm pkg set dependencies.firebase=file:./.nimbus/packages/firebase
npm install
```

The `nimbus` binary materializes its `firebase` package under
`.nimbus/packages/firebase` — no registry access required — and the `file:`
specifier swaps it in for the upstream package.

## 2. Keep your imports unchanged

No import rewrite step exists in this migration:

| Firebase today | After migration |
| --- | --- |
| `import { ... } from "firebase/app"` | unchanged |
| `import { ... } from "firebase/firestore"` | unchanged |

Common Firestore helpers keep the same names and signatures:

- `initializeApp`
- `getFirestore`, `initializeFirestore`, `connectFirestoreEmulator`
- `collection`, `doc`, `collectionGroup`
- `getDoc`, `getDocs`, `addDoc`, `setDoc`, `updateDoc`, `deleteDoc`
- `query`, `where`, `orderBy`, `limit`, cursors, `documentId`
- `onSnapshot`
- `writeBatch`
- `runTransaction`
- `deleteField`, `serverTimestamp`, `increment`, `arrayUnion`, `arrayRemove`

What changes is the surface behind those names: the provisioned package
implements the subset in the
[compatibility matrix](/reference/firebase/compatibility/), so verify the
features your app relies on before cutting over.

## 3. Point the SDK at Nimbus

For local work, redirect the SDK with `connectFirestoreEmulator(...)`:

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

The `projectId` maps directly to a Nimbus tenant id, and only the
`(default)` database is supported. The Firestore-compatible routes are
enabled per server deployment — see the availability note in
[Use Firestore SDKs with Nimbus](/developers/firebase/).

## 4. Keep REST first, opt into gRPC-Web deliberately

Unary calls default to REST, the broadest browser-safe baseline. Confirm
CRUD, query, and watch parity on REST before opting any client into
gRPC-Web:

```typescript
import { initializeFirestore } from "firebase/firestore";

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

Bearer tokens sent by the provisioned `firebase` package resolve into an
application principal on the covered read, write, transaction, and listener
paths — see [Firebase auth](/reference/firebase/auth/) for exactly which
inputs authenticate and which require server-side opt-in.

## 7. Verify against the boundaries

Do not assume any of the following during migration:

- the registry-published Google `firebase` package working against Nimbus
  (the provisioned package is the supported client)
- Node Admin SDK (`firebase-admin`) parity
- mobile or native SDK parity
- named databases (only `(default)` is accepted)
- browser offline persistence, bundles, or `namedQuery`
- Firebase Emulator Suite control endpoints
- a Firestore Security Rules engine

These are intentional, documented boundaries — the full list with status
labels is in the [compatibility matrix](/reference/firebase/compatibility/).

## Suggested order

1. Repoint the `firebase` dependency at the provisioned package.
2. Redirect local development with `connectFirestoreEmulator(...)`.
3. Keep REST unary first and confirm CRUD/query/watch parity.
4. Migrate transactions, write batches, and `FieldValue` usage.
5. Port Security Rules intent into application auth checks.
6. Only then evaluate gRPC-Web unary transport for clients that benefit.
