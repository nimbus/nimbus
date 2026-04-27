# Firebase To Neovex Migration Guide

This guide is the practical migration path for teams moving Firestore-backed
JavaScript apps onto Neovex today.

The current supported target is **`@neovex/firebase`**, not the stock
`firebase/firestore` browser package. Neovex implements the Firestore server
surface plus a first-party Firebase-shaped SDK, but upstream browser
drop-in still depends on WebChannel, which Neovex intentionally defers for
now. Raw stock upstream Firestore `Write` streaming compatibility is also not
yet claimed, so use the first-party package path instead of assuming stock Node
or browser SDK transport parity.

For the detailed support matrix, see
[Firebase compatibility](firebase-compatibility.md). For the current Firebase
application-auth truth, see
[Firebase application auth contract](firebase-auth-contract.md). For the
browser `Listen` transport contract, see
[Firebase WebSocket Listen](firebase-websocket-listen.md).

## Recommended Migration Path

1. Keep your Firestore data model and query shapes.
2. Replace browser or Node Firestore imports with `@neovex/firebase`.
3. Point the SDK at your Neovex host with `connectFirestoreEmulator(...)` for
   local work or `initializeFirestore(...)` settings for hosted environments.
4. Keep unary calls on REST first; opt into gRPC-Web only where you want that
   transport explicitly.
5. Read the current
   [Firebase application auth contract](firebase-auth-contract.md) before
   depending on Firebase-route identity or authorization behavior.
6. Move Firestore Security Rules intent into Neovex-owned application auth and
   authorization checks instead of expecting a rules DSL on the database.

## Import Mapping

| Firebase today | Neovex migration target |
| --- | --- |
| `firebase/app` | `@neovex/firebase/app` |
| `firebase/firestore` | `@neovex/firebase/firestore` |

Common Firestore helpers keep the same names on the Neovex package:

- `initializeApp`
- `getFirestore`
- `initializeFirestore`
- `connectFirestoreEmulator`
- `collection`, `doc`, `collectionGroup`
- `getDoc`, `getDocs`, `addDoc`, `setDoc`, `updateDoc`, `deleteDoc`
- `query`, `where`, `orderBy`, `limit`, cursors, `documentId`
- `onSnapshot`
- `writeBatch`
- `runTransaction`
- `deleteField`, `serverTimestamp`, `increment`, `arrayUnion`, `arrayRemove`

## Quick Start

Install the package:

```bash
npm install @neovex/firebase
```

Initialize the app and connect it to a local Neovex server:

```ts
import { initializeApp } from "@neovex/firebase/app";
import {
  addDoc,
  collection,
  connectFirestoreEmulator,
  getDocs,
  getFirestore,
  onSnapshot,
} from "@neovex/firebase/firestore";

const app = initializeApp({
  projectId: "demo-project",
});

const db = getFirestore(app);
connectFirestoreEmulator(db, "127.0.0.1", 8080);

const messages = collection(db, "messages");

await addDoc(messages, {
  body: "hello from neovex",
  createdAt: new Date().toISOString(),
});

const snapshot = await getDocs(messages);
console.log(snapshot.docs.map((doc) => doc.data()));

const unsubscribe = onSnapshot(messages, (live) => {
  console.log("live size", live.size);
});
```

If you want unary RPCs to use gRPC-Web instead of REST, initialize Firestore
explicitly:

```ts
import { initializeFirestore } from "@neovex/firebase/firestore";

const db = initializeFirestore(app, {
  experimentalUnaryTransport: "grpc-web",
});
connectFirestoreEmulator(db, "127.0.0.1", 8080);
```

REST remains the default because it is the broadest browser-safe baseline.
`onSnapshot(...)` does **not** use gRPC-Web; it uses Neovex's documented
binary-protobuf WebSocket `Listen` bridge.

## Local Demo

Neovex ships a runnable browser demo at
[`demos/firebase/html/`](../../demos/firebase/html/).

Run the local server:

```bash
npm run firebase:server:html
```

Run the demo app:

```bash
npm run firebase:demo:html
```

Then open:

- <http://127.0.0.1:5176/>
- <http://localhost:8080/demos/>

The demo exercises:

- `connectFirestoreEmulator`
- `addDoc`
- `getDocs`
- `onSnapshot`
- `writeBatch`
- `runTransaction`
- `deleteDoc`
- supported `FieldValue` transforms

## Transport Differences

Neovex intentionally keeps transport behavior explicit:

| Concern | Neovex today |
| --- | --- |
| Default unary transport | REST |
| Optional unary transport | gRPC-Web via `experimentalUnaryTransport: "grpc-web"` |
| Browser watch transport | Binary-protobuf WebSocket `Listen` |
| Browser WebChannel | not implemented |
| Browser long polling | not implemented |
| Native gRPC server surface | partially implemented on the server; raw stock upstream `Write` streaming compatibility is not yet claimed |

Implications:

- Browser apps should migrate to `@neovex/firebase`, not assume stock
  `firebase/firestore` transport behavior.
- Node callers can use the same package surface, but watch flows may need an
  explicit `experimentalWebSocketFactory` when no global WebSocket exists.
- If you need exact stock browser SDK behavior or WebChannel semantics, that
  remains a separate follow-on effort.

## Data Model And Query Notes

The Firestore-shaped resource model is live in Neovex, including nested
document paths and collection-group query metadata, but there are still
important boundaries to keep in mind:

- Only the default database, `(default)`, is supported end to end.
- Collection groups are supported on the server and through the current
  `@neovex/firebase` query surface.
- Aggregation queries, `BatchWrite`, and `ListCollectionIds` are server-capable
  today, but the first-party SDK does not yet wrap all of those helpers.
- Offline persistence, cache-only reads, bundles, `namedQuery`,
  `waitForPendingWrites`, and `onSnapshotsInSync` are not implemented.
- Emulator redirection is supported through `connectFirestoreEmulator(...)`,
  but that is host redirection, not full Firebase Emulator Suite parity.

## Security Rules Migration

Neovex does **not** implement the Firestore Security Rules DSL today.

That means migration is about preserving the **intent** of your rules, not
copying rules text unchanged. Treat the rules layer as application
authorization logic that should move into your Neovex-owned auth and mutation /
query boundaries.

Common translations:

| Firestore rules pattern | Neovex migration direction |
| --- | --- |
| `request.auth != null` | Require authenticated callers before serving the read or write path. |
| `request.auth.uid == resource.data.ownerId` | Persist owner identity in the document and enforce ownership in your server-side authorization checks. |
| `request.resource.data.ownerId == request.auth.uid` | Validate write input before commit so callers cannot claim another owner's identity. |
| role or claim checks on `request.auth.token.*` | Map the same claims into your Neovex auth context and check them in the application/runtime layer. |

Two practical rules help keep this migration clean:

1. Keep localhost server-access auth separate from application auth. Neovex's
   local-origin and server-access protections are not a replacement for tenant
   or user authorization.
2. Put authorization where the mutation or query meaning lives. Do not rely on
   a client-only convention to preserve rule behavior.

One current caveat matters here:

- Firebase-route application auth is now enforced on the covered
  CRUD/query/transaction/`Write`/`Listen` paths, but only within the explicit
  contract documented in
  [Firebase application auth contract](firebase-auth-contract.md). Verified
  bearer tokens now reach the shared Neovex principal path on those covered
  routes. JSON-object emulator `mockUserToken` values require explicit
  server-side opt-in for the emulator-only auth contract. Do not assume broader
  upstream Firebase/Auth/Admin parity outside that documented contract.

If your app depends on advanced rules evaluation, treat that as explicit
follow-on work instead of assuming parity from the Firestore transport alone.

## Current Compatibility Boundaries

Use `@neovex/firebase` when you want the supported path today.

Do **not** currently assume:

- stock `firebase/firestore` browser drop-in,
- full Node Admin SDK parity,
- mobile/native SDK parity,
- named databases,
- browser offline persistence,
- Firebase Emulator Suite control endpoints,
- or a Firestore Security Rules engine.

Those boundaries are intentional and documented, not accidental gaps.

## Recommended Adoption Order

For a typical app migration:

1. Move imports to `@neovex/firebase`.
2. Redirect local development with `connectFirestoreEmulator(...)`.
3. Keep REST unary first and confirm CRUD/query/watch parity.
4. Migrate transactions, write batches, and `FieldValue` usage.
5. Port Security Rules intent into application auth and authorization checks.
6. Only then evaluate optional gRPC-Web unary transport for the clients that
   benefit from it.

## See Also

- [Firebase compatibility](firebase-compatibility.md)
- [Firebase application auth contract](firebase-auth-contract.md)
- [Firebase WebSocket Listen](firebase-websocket-listen.md)
- [Firebase upstream test catalog](firebase-upstream-test-catalog.md)
- [Demos](../../demos/README.md)
