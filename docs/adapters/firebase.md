# Firebase / Firestore Adapter

## Overview

Firestore-compatible REST, gRPC-Web, and WebSocket Listen surface. Applications use the `@neovex/firebase` SDK -- API-compatible with `firebase/firestore` -- pointed at a local Neovex server. No codegen step needed.

## Client Package

`@neovex/firebase`

- Exports: `@neovex/firebase/app`, `@neovex/firebase/firestore`

## Quick Start

```bash
# No app directory needed -- just start the server
neovex start --port 8080

# Start your Vite/webpack frontend
npm run dev
```

## Project Layout

```
my-firebase-app/
├── src/
│   └── main.ts
├── package.json
├── vite.config.ts
└── tsconfig.json
```

No `convex/` directory, no codegen, no build artifacts.

## Example Code

```typescript
import { initializeApp } from "@neovex/firebase/app";
import {
  initializeFirestore, connectFirestoreEmulator,
  collection, addDoc, getDocs, deleteDoc, onSnapshot,
  query, orderBy, limit,
  writeBatch, runTransaction,
  serverTimestamp, increment, arrayUnion, doc,
} from "@neovex/firebase/firestore";

// Initialize
const app = initializeApp({ apiKey: "local", projectId: "my-project" });
const db = initializeFirestore(app, { experimentalUnaryTransport: "rest" });
connectFirestoreEmulator(db, "127.0.0.1", 8080);

// CRUD
const messages = collection(db, "messages");
await addDoc(messages, { author: "Alice", body: "Hello", createdAt: serverTimestamp() });
const snapshot = await getDocs(query(messages, orderBy("createdAt", "desc"), limit(10)));

// Live updates
const unsubscribe = onSnapshot(
  query(messages, orderBy("createdAt", "desc"), limit(10)),
  (snap) => console.log("Live:", snap.docs.map((d) => d.data())),
);

// Transactions
await runTransaction(db, async (tx) => {
  const snap = await tx.get(query(messages, orderBy("createdAt", "desc"), limit(1)));
  if (snap.docs[0]) {
    tx.update(snap.docs[0].ref, { likes: increment(1), tags: arrayUnion("liked") });
  }
});

// Batch writes
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

See the [Firebase compatibility matrix](../reference/firebase-compatibility.md) for the full scope.

## Related Docs

- [Firebase compatibility matrix](../reference/firebase-compatibility.md)
- [Firebase migration guide](../reference/firebase-migration-guide.md)
- [Firebase auth contract](../reference/firebase-auth-contract.md)
- [Firebase WebSocket Listen](../reference/firebase-websocket-listen.md)
- [Demo: firebase/html](../../demos/firebase/html/)
