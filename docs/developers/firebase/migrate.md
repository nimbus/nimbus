---
title: Migrate from Firebase
description: Move a Firestore-backed JavaScript app onto Nimbus — nimbus dev repoints the firebase dependency at the drop-in package automatically — and port Security Rules intent into application auth.
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
# in your existing app directory
nimbus dev
```

`nimbus dev` detects the `firebase` dependency, scans your sources to
confirm every Firebase import is on the supported surface, and repoints
the dependency at the drop-in package. If a file imports an uncovered
surface (say `firebase/auth`), the scan refuses with the file, line, and
import named, and nothing is changed — resolve that import first, then
rerun.

To repoint without a dev session, provision directly:

```bash
nimbus packages provision firebase
npm install
```

Either way, the `nimbus` binary materializes its `firebase` package under
`.nimbus/packages/firebase` — no registry access required — and rewrites
your app's `firebase` dependency from the registry spec to
`file:./.nimbus/packages/firebase`, swapping the provisioned package in
for the upstream one.

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
connectFirestoreEmulator(db, "127.0.0.1", 3210);
```

Use the port your server listens on — `nimbus dev` serves on `3210`,
`nimbus start` defaults to `8080`. The `projectId` maps directly to a
Nimbus tenant id, and only the `(default)` database is supported.
`nimbus dev` discovers the project id from your `.firebaserc` and creates
the tenant automatically; every server serves the Firestore-compatible
routes by default — see
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
connectFirestoreEmulator(db, "127.0.0.1", 3210);
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

1. Run `nimbus dev` (or provision directly) to repoint the `firebase`
   dependency at the drop-in package.
2. Redirect local development with `connectFirestoreEmulator(...)`.
3. Keep REST unary first and confirm CRUD/query/watch parity.
4. Migrate transactions, write batches, and `FieldValue` usage.
5. Port Security Rules intent into application auth checks.
6. Only then evaluate gRPC-Web unary transport for clients that benefit.
