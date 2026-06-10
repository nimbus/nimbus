---
title: Firestore compatibility
description: The precise Firebase / Firestore compatibility surface in Nimbus — SDK matrix, API coverage, and intentional boundaries.
sidebar:
  order: 1
---

This reference records the currently implemented Firebase / Firestore
compatibility surface in Nimbus. It is intentionally narrower than a generic
"Firestore-compatible" claim: every row reflects shipped, tested behavior.

Nimbus has three distinct Firebase-facing stories:

1. A tested first-party SDK, `@nimbus/firebase`.
2. A live Firestore server surface: REST, selected native gRPC, gRPC-Web,
   and WebSocket `Listen`.
3. Deferred stock Firebase browser SDK drop-in, because upstream browsers
   still use WebChannel rather than gRPC-Web for the full Firestore client.

For the adoption path, see
[Use Firestore SDKs with Nimbus](/developers/firebase/) and
[Migrate from Firebase](/developers/firebase/migrate/).

## Status labels

| Label | Meaning |
| --- | --- |
| `supported` | Covered by the shipped surface and exercised by tests. |
| `supported with caveats` | Usable now, but requires explicit settings or has a frozen boundary called out here. |
| `server-capable, no SDK wrapper yet` | The server implements the underlying Firestore RPCs, but `@nimbus/firebase` does not expose a helper yet. |
| `not claimed` | No compatibility promise yet, even if some lower-level RPCs exist. |
| `deferred` | Explicitly outside the current release target. |

## SDK and runtime matrix

| Surface | Status | Transport(s) | Current claim |
| --- | --- | --- | --- |
| `@nimbus/firebase` in browsers | `supported with caveats` | REST unary by default, opt-in gRPC-Web unary, WebSocket `Listen` | Primary supported Firebase-style client for Nimbus. Covers refs, CRUD, queries, snapshots, listeners, write batches, transactions, supported `FieldValue` transforms, and bearer-token auth on the covered routes. No WebChannel, no offline persistence, no bundle/named-query APIs, no `onSnapshotsInSync`, no `waitForPendingWrites`, no long polling. |
| `@nimbus/firebase` in Node | `supported with caveats` | Same as browser; watch flows may need an explicit `experimentalWebSocketFactory` | Same API surface as the browser package, for tests and server-side JS callers. Not a drop-in replacement for `firebase-admin` or `@google-cloud/firestore`. |
| Stock `firebase/firestore` browser SDK | `deferred` | Upstream WebChannel stack | No drop-in claim. WebChannel and browser persistence are not implemented; use `@nimbus/firebase` instead. |
| Stock `firebase/firestore/lite` | `not claimed` | Upstream browser REST stack | Overlapping unary semantics exist on the server, but no import-path or transport-stack compatibility promise. |
| Node Admin SDK (`firebase-admin.firestore`) | `not claimed` | Upstream Google Cloud client stack | Many underlying RPCs exist, but Admin SDK parity (BulkWriter, recursive delete, bundles, import/export, emulator control) is broader than the verified surface. |
| Go / Java / Python server SDKs | `not claimed` | Upstream Firestore gRPC/REST clients | Underlying protocol work exists; no supported-compatibility claim is published. |
| Android / Apple / C++ / Unity SDKs | `not claimed` | Native mobile transport stacks | No tested claim for auth, reconnect, persistence, or SDK-specific behavior. |

## `@nimbus/firebase` API coverage

| Area | Status | Notes |
| --- | --- | --- |
| App + Firestore bootstrap (`initializeApp`, `getFirestore`, `initializeFirestore`) | `supported` | App-scoped Firestore instances. |
| Emulator host switching (`connectFirestoreEmulator`) | `supported with caveats` | Redirects the SDK to a local Nimbus host; transport redirection, not Firebase Emulator Suite control-plane parity. |
| Termination (`terminate`) | `supported` | |
| Collection / document / collection-group refs | `supported` | Canonical path validation, including nested refs and collection groups. |
| CRUD (`getDoc`, `setDoc`, `updateDoc`, `deleteDoc`, `addDoc`) | `supported` | Backed by live REST or gRPC-Web unary paths. |
| Query builder (`query`, `where`, `orderBy`, cursors, `limit`, `documentId`) | `supported with caveats` | Supported for the implemented Firestore subset, including collection-group queries and document-name ordering/cursors. Nested field-path helpers remain intentionally narrow. |
| Query execution (`getDocs`, `QuerySnapshot`, `QueryDocumentSnapshot`) | `supported` | |
| Equality helpers (`refEqual`, `queryEqual`, `snapshotEqual`) | `supported` | |
| Converters (`withConverter`, typed `data()`) | `supported` | Converter-backed refs, queries, and snapshots. |
| Listeners (`onSnapshot`) | `supported` | Uses the binary-protobuf [WebSocket Listen channel](/reference/firebase/websocket-listen/) instead of WebChannel. Covers bootstrap, resume, retry budget, and auth upgrade. |
| `writeBatch` | `supported` | Atomic Firestore `Commit` semantics. |
| `runTransaction` | `supported` | Point reads, transactional query reads, staged writes, bounded retries, rollback. |
| `FieldValue` sentinels (`deleteField`, `serverTimestamp`, `increment`, `arrayUnion`, `arrayRemove`) | `supported` | Round-trip through REST and gRPC-Web. |
| Aggregation query helpers | `server-capable, no SDK wrapper yet` | The server implements `RunAggregationQuery`; no JS builders yet. |
| `ListCollectionIds` helpers | `server-capable, no SDK wrapper yet` | |
| `BatchWrite` helpers | `server-capable, no SDK wrapper yet` | |

## Frozen compatibility boundaries

These are intentional, documented boundaries rather than accidental gaps:

- Only the default Firestore database, `(default)`, is supported end to end.
  The SDK can construct non-default database handles, but the server rejects
  named databases.
- Browser full-SDK compatibility means `@nimbus/firebase`, not stock
  `firebase/firestore` drop-in.
- Unary transport defaults to REST. gRPC-Web is available only through the
  explicit `experimentalUnaryTransport: "grpc-web"` setting.
- Watch support uses the WebSocket `Listen` channel — not WebChannel and not
  gRPC-Web.
- Raw stock Firestore `Write` streaming compatibility is not part of the
  current public claim; upstream Node Firestore clients that open a `Write`
  stream are not covered.
- Firebase read surfaces only project Firestore-native typed scalars.
  Documents carrying foreign adapter typed scalars (for example MongoDB
  `ObjectId`, `Decimal128`, `Binary`, `Regex`) are rejected on Firebase
  REST/gRPC reads rather than lossy-projected into strings.
- Application auth on Firebase routes is enforced on the covered
  CRUD/query/transaction/`Write`/`Listen` paths within the contract in
  [Firebase auth](/reference/firebase/auth/); breadth outside that contract
  is not a verified auth promise.
- `experimentalForceLongPolling` and `experimentalAutoDetectLongPolling` are
  accepted as settings-shape compatibility fields, but no long-polling
  transport is implemented behind them.

## Known gaps relative to upstream SDK breadth

- Offline persistence and related browser APIs
  (`enableIndexedDbPersistence`, `clearIndexedDbPersistence`, cache-only
  reads, network toggles) are not implemented.
- Bundle and local query APIs (`loadBundle`, `namedQuery`) are not
  implemented.
- Client-coordination helpers (`onSnapshotsInSync`, `waitForPendingWrites`)
  are not implemented.
- Emulator control-plane compatibility is limited to host redirection via
  `connectFirestoreEmulator`; the Firebase Emulator Suite control/admin
  endpoint family is not implemented.
- Admin-library breadth (bulk writer, recursive delete, import/export) is
  not part of the current claim.
