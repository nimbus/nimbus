# Firebase / Firestore Compatibility

This reference records the currently implemented Firebase / Firestore
compatibility surface in Neovex. It is intentionally narrower than a generic
"Firestore-compatible" claim: every row below is derived from the landed
adapter work, focused server contract tests, and the `@neovex/firebase`
selftest suite.

If you are moving an app onto Neovex now, start with the
[Firebase migration guide](firebase-migration-guide.md) and
[Firebase application auth contract](firebase-auth-contract.md), then treat
this document as the precise support matrix behind that path.

Neovex currently has three distinct Firebase-facing stories:

1. A tested first-party SDK, `@neovex/firebase`.
2. A live Firestore REST + selected gRPC + gRPC-Web + WebSocket `Listen`
   server surface.
3. Deferred stock Firebase browser SDK drop-in, because upstream browsers still
   use WebChannel rather than gRPC-Web for the full Firestore client.

## Status Labels

| Label | Meaning |
|-------|---------|
| `supported` | Covered by the shipped surface and exercised by focused tests/selftests. |
| `supported with caveats` | Usable now, but requires explicit settings or has a frozen boundary called out here. |
| `server-capable, no SDK wrapper yet` | The Neovex server implements the underlying Firestore RPCs, but `@neovex/firebase` does not expose the corresponding helper yet. |
| `not claimed` | No compatibility promise yet, even if some lower-level RPCs exist. |
| `deferred` | Explicitly outside the current release target. |

## Tier Snapshot

| Tier | State in Neovex today | Notes |
|------|------------------------|-------|
| `T0` core primitives | `done` | Resource paths, atomic write batches, structured queries, transaction sessions, and subscription snapshot/diff seams are shared Neovex primitives. |
| `T1` REST / CRUD vertical slice | `done` | `Commit`, `BatchGetDocuments`, `RunQuery`, serializer, paths, and REST error mapping are live. |
| `T2` full data-path transports | `done` for the first-party SDK path / `partial` for raw upstream gRPC clients | Covered first-party Firebase paths are live across REST, gRPC-Web unary, native `Listen`, and browser WebSocket `Listen`. The server also implements native Firestore gRPC surfaces, but raw stock upstream `Write` streaming compatibility is not yet claimed because the recorded Node Firestore smoke still fails with `12 UNIMPLEMENTED`. |
| `T3` query / admin breadth | `partial` | Collection groups, aggregations, `BatchWrite`, and `ListCollectionIds` are implemented on the server; upstream Admin SDK parity is not claimed yet. |
| Stock browser SDK drop-in | `deferred` | WebChannel, emulator-control endpoints, offline/browser persistence, and full upstream browser transport parity are not implemented. |

## SDK And Runtime Matrix

| Surface | Status | Transport(s) | Current claim | Explicit gaps |
|---------|--------|--------------|---------------|---------------|
| `@neovex/firebase` in browsers | `supported with caveats` | REST unary by default, opt-in gRPC-Web unary, WebSocket `Listen` | Primary supported Firebase-style client for Neovex. Covers refs, CRUD, queries, snapshots, listeners, write batches, transactions, supported `FieldValue` transforms, and covered Firebase-route principal propagation for verified bearer tokens. JSON-object emulator `mockUserToken` values are supported only when the server explicitly enables that emulator-only auth contract. | No WebChannel, no offline persistence/cache APIs, no bundle/named-query APIs, no `onSnapshotsInSync`, no `waitForPendingWrites`, no long-polling transport, and auth behavior remains limited to the covered contract in [Firebase application auth contract](firebase-auth-contract.md). |
| `@neovex/firebase` in Node | `supported with caveats` | REST unary by default, opt-in gRPC-Web unary, WebSocket `Listen` with explicit socket wiring when needed | Same API surface as the browser package, intended for tests and server-side JS callers that want the Neovex first-party SDK instead of the Google Cloud client libraries. Covered Firebase-route principal propagation matches the browser package on the supported data paths. | Watch flows may require `experimentalWebSocketFactory`; this is not a drop-in replacement for `firebase-admin` or `@google-cloud/firestore`, and auth behavior remains limited to the covered contract in [Firebase application auth contract](firebase-auth-contract.md). |
| Stock `firebase/firestore` browser SDK | `deferred` | Upstream WebChannel + browser-specific transport stack | No drop-in claim today. Use `@neovex/firebase` instead. | WebChannel is not implemented; browser persistence/offline behavior is not implemented. |
| Stock `firebase/firestore/lite` browser SDK | `not claimed` | Upstream browser REST stack | Some overlapping unary semantics exist on the server, but Neovex does not claim import-path or transport-stack compatibility for the upstream Lite package. | No upstream package integration testing; no drop-in import compatibility promise. |
| Node Admin SDK (`firebase-admin.firestore`) | `not claimed` | Upstream Google Cloud Firestore client stack over gRPC/REST | Neovex implements many of the underlying Firestore RPCs, but Admin SDK parity is broader than the current verified surface. | No compatibility test pass yet for BulkWriter, recursive delete-style helpers, bundles, import/export, emulator control endpoints, or other Google Cloud client behavior. |
| Go / Java / Python server SDKs | `not claimed` | Upstream Firestore gRPC/REST clients | Treated the same as Admin Node: underlying protocol work exists, but no supported-compatibility claim is published yet. | No upstream SDK verification yet; broader admin/library features remain outside the current claim. |
| Android / Apple / C++ / Unity client SDKs | `not claimed` | Native mobile transport stacks | The server exposes the Firestore protocol family, but Neovex has not yet run the upstream mobile/native SDK compatibility wave. | No tested claim for auth, reconnect, persistence, or SDK-specific client behavior. |

## `@neovex/firebase` API Coverage

| Area | Status | Notes |
|------|--------|-------|
| App + Firestore bootstrap (`initializeApp`, `getFirestore`, `initializeFirestore`) | `supported` | Mirrors the first-party SDK surface for app-scoped Firestore instances. |
| Emulator host switching (`connectFirestoreEmulator`) | `supported with caveats` | Redirects the SDK to a local Neovex host and disables TLS/fetch streams; this is transport redirection, not full Firebase Emulator Suite control-plane parity. |
| Termination (`terminate`) | `supported` | Supported for the first-party SDK surface. |
| Collection / document / collection-group refs | `supported` | Canonical path validation matches the landed Firestore resource-path model, including nested refs and collection groups. |
| CRUD (`getDoc`, `setDoc`, `updateDoc`, `deleteDoc`, `addDoc`) | `supported` | Backed by live REST or gRPC-Web unary paths. |
| Query builder (`query`, `where`, `orderBy`, cursors, `limit`, `documentId`) | `supported with caveats` | Supported for the implemented Firestore subset, including collection-group queries and document-name ordering/cursors. Nested field-path helpers remain intentionally narrow. |
| Query execution (`getDocs`, `QuerySnapshot`, `QueryDocumentSnapshot`) | `supported` | Covered by package selftests and live server smoke. |
| Equality helpers (`refEqual`, `queryEqual`, `snapshotEqual`) | `supported` | Supported for refs, queries, and snapshots on the current first-party surface. |
| Converters (`withConverter`, typed `data()`) | `supported` | Converter-backed refs, queries, and snapshots are part of the tested package surface. |
| Listeners (`onSnapshot`) | `supported` | Uses Neovex's documented binary-protobuf WebSocket `Listen` transport instead of WebChannel. Covers bootstrap, resume, retry budget, and auth-upgrade behavior. |
| `writeBatch` | `supported` | Maps to atomic Firestore `Commit` semantics through the shared Neovex write-batch primitive. |
| `runTransaction` | `supported` | Covers point reads, transactional query reads, staged writes, bounded retries, and rollback behavior. |
| `FieldValue` sentinels (`deleteField`, `serverTimestamp`, `increment`, `arrayUnion`, `arrayRemove`) | `supported` | Lower onto the shared transform/delete primitives and round-trip through REST and gRPC-Web. |
| Aggregation query helpers | `not claimed` | The server implements `RunAggregationQuery`, but the first-party JS package does not expose aggregation builders/helpers yet. |
| `ListCollectionIds` helpers | `not claimed` | Server-capable today; no JS SDK wrapper yet. |
| `BatchWrite` helpers | `not claimed` | Server-capable today; no JS SDK wrapper yet. |

## Frozen Compatibility Boundaries

These are intentional, documented boundaries rather than accidental gaps:

- Only the default Firestore database, `(default)`, is supported end to end.
  The first-party SDK can construct non-default database handles, but the
  current server adapter rejects named databases.
- Browser full-SDK compatibility means `@neovex/firebase`, not stock
  `firebase/firestore` drop-in. Upstream browser SDK support remains tied to a
  separate WebChannel compatibility effort.
- Unary browser transport defaults to REST. gRPC-Web is available through the
  explicit `experimentalUnaryTransport: "grpc-web"` setting.
- Browser and JS watch support uses the documented WebSocket `Listen` bridge,
  not WebChannel and not gRPC-Web.
- Raw stock Firestore `Write` streaming compatibility is not part of the
  current public claim. The first-party SDK uses covered REST and WebSocket
  paths today, while the upstream Node Firestore smoke catalog still records
  `GrpcConnection RPC 'Write' stream ... 12 UNIMPLEMENTED`.
- Firebase route-family application auth is documented separately in
  [Firebase application auth contract](firebase-auth-contract.md). Covered
  CRUD/query/transaction/`Write`/`Listen` paths now enforce the resolved
  principal, but unclaimed Firebase/admin breadth outside that contract should
  not be treated as a verified auth-compatibility promise.
- `experimentalForceLongPolling` and
  `experimentalAutoDetectLongPolling` are accepted as settings-shape
  compatibility fields, but Neovex does not currently implement a long-polling
  Firestore transport behind them.

## Known Gaps Relative To Upstream Firestore SDK Breadth

- Offline persistence and related browser APIs:
  `enableIndexedDbPersistence`, `clearIndexedDbPersistence`, cache-only
  behavior, and network toggle flows are not implemented.
- Bundle and local query APIs:
  `loadBundle`, `namedQuery`, and related bundle metadata flows are not
  implemented.
- Client-coordination helpers:
  `onSnapshotsInSync`, `waitForPendingWrites`, and similar coordination hooks
  are not implemented.
- Emulator control-plane compatibility is limited:
  Neovex supports redirecting the first-party SDK to a local host, but not the
  Firebase Emulator Suite's full control/admin endpoint family.
- Admin-library breadth is still open:
  Bulk writer, recursive delete-style helpers, import/export, and other
  Google Cloud Firestore admin features are not part of the current
  compatibility claim.

## Verification Basis

This matrix is sourced from:

- the Firebase adapter control plan and its execution log,
- the current Firebase auth/principal baseline in
  [Firebase application auth contract](firebase-auth-contract.md),
- the landed `@neovex/firebase` export surface in `packages/firebase/src/`,
- the package selftest suite in `packages/firebase/src/selftest.mjs`,
- the Firestore server contract tests in `crates/neovex-server/src/tests.rs`,
- the documented browser `Listen` transport in
  [Firebase WebSocket Listen](firebase-websocket-listen.md),
- and the Source Evidence Map in
  `docs/plans/archive/firebase-adapter-plan.md`.
