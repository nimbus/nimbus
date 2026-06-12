# firebase (Nimbus drop-in)

Nimbus's drop-in `firebase` package: a first-party, tested implementation of
the modular `firebase/app` and `firebase/firestore` API that talks to a
[Nimbus](../../README.md) Firestore endpoint over REST / gRPC-Web.

It takes the stock npm name — like Nimbus's `convex` package — so existing
code keeps its imports unchanged: `initializeApp`, `getFirestore`,
`collection`, `doc`, `getDoc`, `setDoc`, `query`, `where`, `onSnapshot`,
batches, and transactions are all exposed with matching signatures. The
`nimbus` binary provisions it locally into an app's `.nimbus/packages/`
(`nimbus packages provision firebase`); it is never published to a registry.

> The compatibility surface is intentionally narrower than a blanket
> "Firestore-compatible" claim. Every supported feature is backed by focused
> server contract tests and the package selftest. Check the
> [Firestore compatibility matrix](../../docs/reference/firebase/compatibility.md)
> before relying on a specific feature.

## Entry points

| Import | Use it for |
| --- | --- |
| `firebase` | Everything (re-exports `./app` and `./firestore`) |
| `firebase/app` | App lifecycle: `initializeApp`, `getApp`, `getApps`, `deleteApp` |
| `firebase/firestore` | Firestore: refs, queries, reads/writes, snapshots, batches, transactions |

## Usage

Wire an app's `firebase` dependency at the provisioned package, then keep
stock imports:

```bash
nimbus packages provision firebase
npm pkg set dependencies.firebase=file:./.nimbus/packages/firebase
npm install
```

```ts
import { initializeApp } from "firebase/app";
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
} from "firebase/firestore";

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
[Firestore auth reference](../../docs/reference/firebase/auth.md).

## Codegen

The Firestore protobuf bindings under `src/gen/` are generated from upstream
protos:

```bash
npm run codegen:proto --workspace firebase
```

## Scripts

```bash
npm run build --workspace firebase      # build-only selftest pass
npm run test --workspace firebase       # selftest suite
npm run typecheck --workspace firebase   # type-only selftest pass
```

## Related

- [Firestore compatibility matrix](../../docs/reference/firebase/compatibility.md)
- [Migration guide](../../docs/developers/firebase/migrate.md)
- [Firestore auth reference](../../docs/reference/firebase/auth.md)
- [WebSocket `Listen` surface](../../docs/reference/firebase/websocket-listen.md)
