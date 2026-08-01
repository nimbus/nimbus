---
title: Migrate from Firebase
description: Move a Firestore-backed JavaScript app onto Nimbus — nimbus dev repoints the firebase dependency at the drop-in package automatically — and port Security Rules intent into application auth.
sidebar:
  order: 2
---

Move a Firestore-backed JavaScript app onto Nimbus while keeping your
imports, data model, query shapes, and helper names. The migration target
is Nimbus's first-party drop-in `firebase` package. It takes the stock npm
name. Your `firebase/app` and `firebase/firestore` imports therefore stay
exactly the same. Only dependency resolution changes. The registry-published
Google package is not the target.

The Google package's browser client depends on WebChannel transport, which
Nimbus intentionally defers.

Before you start, read
[Use Firestore SDKs with Nimbus](/developers/firebase/) for the connection
model. Keep the
[compatibility matrix](/reference/firebase/compatibility/) open as the
precise support reference.

## 1. Repoint the `firebase` dependency

```bash
# in your existing app directory
nimbus dev
```

`nimbus dev` detects the `firebase` dependency, scans your sources to
confirm that each Firebase import uses the supported surface, and repoints
the dependency at the drop-in package. If a file imports an uncovered
surface, such as `firebase/auth`, the scan refuses the change. The diagnostic
names the file, line, and import. Resolve that import, then run the command
again.

To repoint without a dev session, provision directly:

```bash
nimbus packages provision firebase
npm install
```

Both methods use the `firebase` package in the `nimbus` binary without
registry access. Nimbus writes it under `.nimbus/packages/firebase`. It also
rewrites your app's `firebase` dependency from the registry spec to
`file:./.nimbus/packages/firebase`. The provisioned package then replaces the
upstream package.

## 2. Keep your imports unchanged

This migration does not rewrite imports. Your `firebase/app` and
`firebase/firestore` import lines are byte-identical before and after. Only
the dependency resolution changes.

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

The surface behind those names changes. The provisioned package implements
the subset in the [compatibility matrix](/reference/firebase/compatibility/).
Verify the features that your app needs before the cutover.

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
connectFirestoreEmulator(db, "127.0.0.1", 3210);
```

Use the port on which your server listens. `nimbus dev` serves on `3210`, and
`nimbus start` defaults to `8080`. The `projectId` maps directly to a Nimbus
tenant id. Nimbus supports only the `(default)` database. `nimbus dev` reads
the project id from your `.firebaserc` and creates the tenant automatically.
Each server serves the Firestore-compatible routes by default. See
[Use Firestore SDKs with Nimbus](/developers/firebase/) for details.

## 4. Keep REST first, opt into gRPC-Web deliberately

Unary calls default to REST, the broadest browser-safe baseline. Confirm
CRUD, query, and watch parity on REST before opting any client into
gRPC-Web:

```typescript
import { initializeFirestore } from "firebase/firestore";

const db = initializeFirestore(app, {
  experimentalUnaryTransport: "grpc-web",
});
connectFirestoreEmulator(db, "127.0.0.1", 3210);
```

`onSnapshot(...)` never uses gRPC-Web. It always runs over the binary
[WebSocket Listen channel](/reference/firebase/websocket-listen/). In Node or
other environments without a global `WebSocket`, supply an
`experimentalWebSocketFactory` in the Firestore settings.

## 5. Migrate transactions, batches, and field transforms

Nimbus supports `writeBatch`, `runTransaction`, and the `FieldValue` sentinels.
The sentinels are `deleteField`, `serverTimestamp`, `increment`, `arrayUnion`,
and `arrayRemove`. These APIs keep their Firestore semantics. Batches commit
atomically.

Transactions cover point reads, query reads, staged writes,
bounded retries, and rollback. Migrate this code unchanged. Then verify the
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

Bearer tokens from the provisioned `firebase` package resolve into an
application principal. This applies to the covered read, write, transaction,
and listener paths. See [Firebase auth](/reference/firebase/auth/) for the
inputs that authenticate and those that require server-side opt-in.

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

These are intentional, documented boundaries. The
[compatibility matrix](/reference/firebase/compatibility/) gives the full list
with status labels.

## Suggested order

1. Run `nimbus dev` (or provision directly) to repoint the `firebase`
   dependency at the drop-in package.
2. Redirect local development with `connectFirestoreEmulator(...)`.
3. Keep REST unary first and confirm CRUD/query/watch parity.
4. Migrate transactions, write batches, and `FieldValue` usage.
5. Port Security Rules intent into application auth checks.
6. Only then evaluate gRPC-Web unary transport for clients that benefit.
