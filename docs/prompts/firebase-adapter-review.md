# Codex Agent Review Prompt — Firebase Adapter Plan

Use this prompt to have a Codex Agent independently review and audit
`docs/plans/archive/firebase-adapter-plan.md`. Copy the full text below into the
agent's input.

---

## Prompt

You are reviewing an execution plan for building a Firebase/Firestore
compatibility adapter in a Rust + TypeScript codebase called Neovex. Neovex is
a Convex-compatible backend server that already has a deep Convex adapter
(~10k lines of Rust, 127 files). The Firebase adapter is the second
compatibility layer, translating Firestore v1 gRPC and REST protocols to
Neovex's internal engine APIs.

**Your task:** Perform a thorough, independent audit of the plan at
`docs/plans/archive/firebase-adapter-plan.md`. Evaluate the following dimensions and
report findings as a structured list of issues (Critical / Major / Minor /
Suggestion) with specific line references.

### 1. Architecture & Software Patterns

- **Adapter registration pattern:** The plan proposes `pub(crate) mod firebase;`
  in `adapters/mod.rs`, `with_firebase()` on `RouterBuildConfig`, and
  `ActiveFirebaseConfig` in `AppState` following the Convex adapter pattern.
  Verify this is consistent with the actual Convex adapter code in
  `crates/neovex-server/src/adapters/convex/mod.rs`,
  `crates/neovex-server/src/router.rs`, and
  `crates/neovex-server/src/state.rs`. Flag any divergence.

- **Shared adapter logic:** The plan proposes each RPC has a single Rust
  implementation called from both the tonic gRPC trait method and the axum REST
  handler. Evaluate whether this is idiomatic for tonic + axum coexistence.
  Check how tonic's generated trait methods receive `Request<T>` versus axum
  handlers receiving `Json<T>` — is sharing logic between them clean or does it
  create awkward extraction/conversion boilerplate?

- **MutationExecutionUnit usage:** The plan maps Firestore's `Commit` RPC to
  `Service::begin_mutation_execution_unit()` → stage writes → `unit.commit()`.
  Verify this correctly models Firestore's atomic batch semantics. Check:
  - Does the Convex adapter use `MutationExecutionUnit` the same way?
    (see `crates/neovex-server/src/adapters/convex/host_bridge/bridge.rs`)
  - Is `MutationExecutionUnit` designed for the adapter-creates-and-commits
    pattern, or is it designed for the V8-runtime-drives-the-unit pattern?
  - Can a `MutationExecutionUnit` handle mixed insert + update + delete in
    a single commit? The Firestore Commit RPC requires this.

- **Transaction lifecycle:** The plan maps Firestore's `BeginTransaction` to
  `begin_mutation_execution_unit()` and stores the unit across multiple RPCs.
  Evaluate: how does the adapter hold a `MutationExecutionUnit` across
  separate gRPC calls (BeginTransaction → BatchGetDocuments with transaction
  → Commit with transaction)? This requires server-side session state. Does
  the existing Neovex architecture support this? Where is it stored? What
  handles timeout/cleanup?

- **WebSocket endpoint for Listen:** The plan adds a WebSocket endpoint
  (`/v1/listen/ws`) for browser `Listen` bidi streaming because gRPC-Web
  cannot do bidirectional streaming. Evaluate whether this is the right
  approach or if there's a cleaner alternative (e.g., tonic's native
  WebSocket transport support, connect-web's upcoming WebTransport support,
  or SSE with client-to-server polling).

### 2. Protocol Correctness

- **Proto3 JSON serialization:** The plan specifies bidirectional translation
  between Firestore typed values (`{ integerValue: "123" }`) and Neovex native
  JSON (`123`). Verify the mapping table is complete and correct by checking
  against the Firestore Value proto definition at
  `google/firestore/v1/document.proto`. Are there any value types missing?

- **StructuredQuery translation:** Cross-reference the filter operators table
  against `google/firestore/v1/query.proto`. Are all StructuredQuery fields
  covered (select, from, where, orderBy, startAt, endAt, offset, limit)?
  The plan mentions `limit` but not `offset` — is this intentional?

- **Commit/Write format:** Verify the mutation types against
  `google/firestore/v1/write.proto`. The plan lists Set, Patch, Delete,
  Verify, and Field Transforms. Does the proto define additional write
  types? Is the `currentDocument` precondition handling complete?

- **Listen protocol:** Compare the plan's Listen message format against the
  proto definitions and the Firebase JS SDK's implementation in
  `packages/firestore/src/remote/persistent_stream.ts`. Are there edge
  cases in the resume token lifecycle, existence filter, or target change
  state machine that the plan doesn't address?

- **Error codes:** The plan references all 17 gRPC status codes. Verify that
  the error format for REST (`{ "error": { "status": "...", "message": "..." } }`)
  matches what the SDK actually parses. Check `rpc_error.ts` and any error
  response parsing in `rest_connection.ts`.

- **Document resource path format:** Verify the path format
  `projects/{p}/databases/{d}/documents/{collection}/{doc}` against the
  Firestore REST API documentation and the SDK's path construction code.
  Are there edge cases around document IDs with special characters (slashes,
  dots, Unicode)?

### 3. Idiomatic Rust Patterns

- **tonic service implementation:** The plan uses pre-generated bindings from
  `googleapis-tonic-google-firestore-v1`. Evaluate:
  - Is using a community-maintained crate (v0.31.0, bouzuya/googleapis-tonic)
    appropriate for production use, or should the project compile from
    googleapis protos directly via `tonic-build`?
  - Does the generated trait require implementing ALL 17 RPCs, or can
    unimplemented RPCs return `Status::UNIMPLEMENTED`?

- **tonic + axum integration:** The plan uses
  `tonic::service::Routes::into_axum_router()` to share a port. Evaluate
  middleware ordering concerns: will CORS, auth, and rate limiting middleware
  apply correctly to both gRPC and REST routes? Does `tonic-web` need to be
  layered before or after these?

- **Error handling:** The plan proposes a Firebase-specific `AppError` variant
  or error mapper. Evaluate whether this should be a separate error type or
  integrated into the existing `AppError` enum. Check how the Convex adapter
  handles errors.

- **Async patterns:** The plan references `subscribe_async_with_principal` and
  `mpsc::Sender<SubscriptionUpdate>`. Verify that the proposed Listen stream
  implementation (accepting `Streaming<ListenRequest>`, returning
  `Stream<ListenResponse>`) is idiomatic tonic. Check if `tokio::select!`
  over the inbound stream + subscription updates is the right pattern.

### 4. Testing Strategy

- **Layer 6 (Firebase SDK integration tests):** The plan proposes running
  Firebase's own test suite against the Neovex adapter. Evaluate feasibility:
  - The tests are in `~/src/github.com/firebase/firebase-js-sdk/packages/firestore/test/integration/api/`.
    Read a few test files to understand what they actually test and what
    backend capabilities they assume.
  - Do the Node.js tests use gRPC directly or do they go through an
    abstraction layer that might add WebChannel-specific behavior?
  - Do tests use any emulator-specific control endpoints (data clearing,
    security rules, etc.) that we'd need to implement?
  - Is the `FIRESTORE_TARGET_BACKEND=emulator` mode meaningfully different
    from production mode in ways that affect test behavior?

- **Layer 4 (RPC contract tests):** The plan tests against "the exact wire
  format the Firebase SDK sends." Evaluate whether the test approach should
  use actual serialized proto bytes or JSON representations. For gRPC tests,
  should the tests use a gRPC client or construct raw proto messages?

- **Missing test scenarios:** Are there important scenarios not covered?
  Consider: concurrent Listen streams, target ID reuse after removal,
  transaction timeout behavior, precondition failures in batch commits,
  large document handling (1 MiB limit), deeply nested subcollection paths,
  collection group queries across many tables.

### 5. Subcollection Convention

- The plan encodes `cities/SF/landmarks/1` as table `cities__landmarks` with
  `_parent_id: "SF"`. Evaluate:
  - What happens with 3+ level nesting (`a/1/b/2/c/3`)? Is it
    `a__b__c` with `_parent_id: "2"`? What about the grandparent `"1"`?
  - What if a table name contains `__` naturally?
  - Is `_parent_id` a reserved field name? What if a document has a field
    called `_parent_id`?
  - How do collection group queries work with this encoding?

### 6. Completeness & Gaps

- **Missing RPCs:** The plan defers `PartitionQuery`, `Write` (bidi mutation
  stream), and `ExecutePipeline`. For each, evaluate whether any common SDK
  operation depends on it that would cause failures.

- **Firestore features not addressed:** Are there Firestore features that
  developers commonly use that aren't mentioned in the plan? Consider:
  document snapshots with `source` option (cache vs server), `getDocsFromCache`,
  `enableNetwork`/`disableNetwork`, `waitForPendingWrites`, `snapshotEqual`,
  `documentId()` sentinel for queries, `Timestamp` class behavior.

- **Admin SDK compatibility:** The plan mentions Admin SDKs (Python, Go, Java,
  etc.) use gRPC. Do Admin SDKs use any RPCs or features not covered by the
  client SDK? (e.g., `BatchWrite` is used by Admin SDKs but not client SDKs
  in some cases; recursive deletes; import/export.)

- **Multi-database support:** Firestore supports named databases beyond
  `(default)`. The plan says "Neovex is single-database-per-tenant." If a
  client sends `databases/my-custom-db`, what happens? Is this validated
  and rejected with a clear error?

### 7. Risk Assessment

- Review each of the 6 listed risks (R1-R6). For each, evaluate:
  - Is the severity rating accurate?
  - Is the mitigation concrete and actionable?
  - Are there unlisted risks that should be added?

- **Specific risk questions:**
  - R1 (proto dependency chain): Is `googleapis-tonic-google-firestore-v1`
    actually compatible with the latest Firestore proto? Check the crate's
    last update date vs recent proto changes.
  - R2 (browser Listen transport): The mitigation proposes a WebSocket
    endpoint. Is this well-specified enough to estimate? What message
    framing format will it use?
  - R4 (field transforms): How significant is the engine extension needed
    for `increment()`? Is this a small change or a fundamental capability
    the engine lacks?

### 8. Estimate Accuracy

- The total estimate is 10.5-11.5 weeks. For each phase, evaluate whether
  the duration is realistic given the scope. Pay special attention to:
  - F1 (3 weeks): Includes scaffolding, serializer, path parser, 3 RPC
    handlers, and auto-ID. Is this front-loaded enough?
  - F2 (2.5 weeks): Includes tonic scaffold, Listen bidi stream, remaining
    unary RPCs, advanced filters, aggregation queries, and collection group
    queries. Is this enough for the Listen stream alone?
  - F4 (2 weeks): A full SDK package with gRPC-Web + WebSocket transport.
    Compare against the Convex JS SDK size and development time.

### Output Format

Structure your response as:

```
## Critical Issues
[issues that would cause the implementation to fail or produce incorrect behavior]

## Major Issues
[issues that would cause significant rework or miss important functionality]

## Minor Issues
[issues that should be fixed but won't block implementation]

## Suggestions
[optional improvements, alternative approaches worth considering]

## Verdict
[1-2 sentence overall assessment: is this plan ready for execution, or does
it need another revision pass?]
```

**Important context files to read before reviewing:**
- `docs/plans/archive/firebase-adapter-plan.md` (the plan under review)
- `CLAUDE.md` (repo conventions, architecture invariants)
- `ARCHITECTURE.md` (system architecture)
- `crates/neovex-server/src/router.rs` (RouterBuildConfig)
- `crates/neovex-server/src/state.rs` (AppState, ActiveConvexRegistry)
- `crates/neovex-server/src/adapters/convex/mod.rs` (Convex adapter structure)
- `crates/neovex-engine/src/service/execution_units/` (MutationExecutionUnit)
- `crates/neovex-engine/src/service/subscriptions.rs` (subscription API)
