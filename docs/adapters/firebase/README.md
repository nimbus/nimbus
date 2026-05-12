# Firebase / Firestore Adapter

```typescript
import { initializeApp } from "@nimbus/firebase/app";
import { initializeFirestore, connectFirestoreEmulator,
  collection, addDoc, getDocs, onSnapshot, query, orderBy, limit,
} from "@nimbus/firebase/firestore";

const app = initializeApp({ apiKey: "local", projectId: "my-project" });
const db = initializeFirestore(app, { experimentalUnaryTransport: "rest" });
connectFirestoreEmulator(db, "127.0.0.1", 8080);

const messages = collection(db, "messages");
await addDoc(messages, { author: "Alice", body: "Hello" });
const snapshot = await getDocs(query(messages, orderBy("_creationTime", "desc"), limit(10)));
```

Firestore-compatible SDK pointed at a local Nimbus server. No codegen, no
special project layout. Your existing Firestore patterns work -- just swap
the import from `firebase` to `@nimbus/firebase`. ~3 minutes from install to query.

## Quick start

**1. Start Nimbus:**

```bash
nimbus start --port 8080
```

**2. Install the SDK:**

```bash
npm install @nimbus/firebase
```

**3. Write your app** (shown at the top of this page).

**4. Run it:**

```bash
npm run dev
```

## How it works

Firestore-compatible REST, gRPC-Web, and WebSocket Listen surface. Applications
use the `@nimbus/firebase` SDK -- API-compatible with `firebase/firestore` --
pointed at a local Nimbus server.

## Client package

`@nimbus/firebase`

- Exports: `@nimbus/firebase/app`, `@nimbus/firebase/firestore`

## Project layout

```
my-firebase-app/
├── src/
│   └── main.ts
├── package.json
├── vite.config.ts
└── tsconfig.json
```

No `convex/` directory, no codegen, no build artifacts.

## More examples

### Live updates

```typescript
const unsubscribe = onSnapshot(
  query(messages, orderBy("_creationTime", "desc"), limit(10)),
  (snap) => console.log("Live:", snap.docs.map((d) => d.data())),
);
```

### Transactions

```typescript
import { runTransaction, increment, arrayUnion } from "@nimbus/firebase/firestore";

await runTransaction(db, async (tx) => {
  const snap = await tx.get(query(messages, orderBy("_creationTime", "desc"), limit(1)));
  if (snap.docs[0]) {
    tx.update(snap.docs[0].ref, { likes: increment(1), tags: arrayUnion("liked") });
  }
});
```

### Batch writes

```typescript
import { writeBatch, doc, serverTimestamp } from "@nimbus/firebase/firestore";

const batch = writeBatch(db);
batch.set(doc(messages, "msg-1"), { author: "Batch", body: "First", createdAt: serverTimestamp() });
batch.set(doc(messages, "msg-2"), { author: "Batch", body: "Second", createdAt: serverTimestamp() });
await batch.commit();
```

## Configuration

- Unary transport: REST (default) or gRPC-Web (set via `experimentalUnaryTransport: "grpc-web"`)
- Live updates always use the WebSocket Listen bridge
- Firebase REST API paths: `POST /v1/projects/{project}/databases/(default)/documents:commit`, etc.

## Known Limitations

- Only default database `(default)` is supported
- No stock `firebase/firestore` browser SDK drop-in (WebChannel not implemented)
- No offline persistence, bundles, or `onSnapshotsInSync`
- No first-party React hooks (Firebase does not ship these upstream)

See the [Firebase compatibility matrix](compatibility.md) for the full scope.

## Related Docs

- [Firebase compatibility matrix](compatibility.md)
- [Firebase migration guide](migration.md)
- [Firebase auth contract](auth-contract.md)
- [Firebase WebSocket Listen](websocket-listen.md)
- [Demo: firebase/html](../../demos/firebase/html/)
