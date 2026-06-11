# @nimbus/firebase

A first-party, tested Firebase SDK that mirrors the modular `firebase/app` and
`firebase/firestore` API and talks to a [Nimbus](../../README.md) Firestore
endpoint over REST / gRPC-Web.

Use it where you would use the stock `firebase` package's modular API:
`initializeApp`, `getFirestore`, `collection`, `doc`, `getDoc`, `setDoc`,
`query`, `where`, `onSnapshot`, batches, and transactions are all exposed with
matching signatures.

> The compatibility surface is intentionally narrower than a blanket
> "Firestore-compatible" claim. Every supported feature is backed by focused
> server contract tests and the package selftest. Check the
> [Firebase / Firestore compatibility matrix](../../docs/private/adapters/firebase/compatibility.md)
> before relying on a specific feature.

## Entry points

| Import | Use it for |
| --- | --- |
| `@nimbus/firebase` | Everything (re-exports `./app` and `./firestore`) |
| `@nimbus/firebase/app` | App lifecycle: `initializeApp`, `getApp`, `getApps`, `deleteApp` |
| `@nimbus/firebase/firestore` | Firestore: refs, queries, reads/writes, snapshots, batches, transactions |

## Usage

```ts
import { initializeApp } from "@nimbus/firebase/app";
import {
  getFirestore,
  connectFirestoreEmulator,
  collection,
  doc,
  query,
  where,
  orderBy,
  getDocs,
  setDoc,
  onSnapshot,
} from "@nimbus/firebase/firestore";

const app = initializeApp({ projectId: "demo-app" });
const db = getFirestore(app);

// Point at a local Nimbus Firestore listener.
connectFirestoreEmulator(db, "127.0.0.1", 8080);

// Write
await setDoc(doc(db, "messages", "m1"), { channel: "general", body: "hi" });

// Query
const snap = await getDocs(
  query(collection(db, "messages"), where("channel", "==", "general"), orderBy("body")),
);
snap.forEach((d) => console.log(d.id, d.data()));

// Realtime listener
const unsubscribe = onSnapshot(collection(db, "messages"), (snap) => {
  console.log("size", snap.size);
});
```

Authentication is supplied through `FirestoreSettings`/`initializeFirestore`
via a token fetcher — see the
[Firebase application auth contract](../../docs/private/adapters/firebase/auth-contract.md).

## Codegen

The Firestore protobuf bindings under `src/gen/` are generated from upstream
protos:

```bash
npm run codegen:proto --workspace @nimbus/firebase
```

## Scripts

```bash
npm run build --workspace @nimbus/firebase      # build-only selftest pass
npm run test --workspace @nimbus/firebase       # selftest suite
npm run typecheck --workspace @nimbus/firebase   # type-only selftest pass
```

## Related

- [Firebase / Firestore compatibility](../../docs/private/adapters/firebase/compatibility.md)
- [Migration guide](../../docs/private/adapters/firebase/migration.md)
- [Application auth contract](../../docs/private/adapters/firebase/auth-contract.md)
- [WebSocket `Listen` surface](../../docs/private/adapters/firebase/websocket-listen.md)
