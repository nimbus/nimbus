# Plan: Multi-Adapter Boundary Hardening

Canonical execution control plane for the post-Firebase, post-Cloud-Functions
architecture hardening wave. This plan exists to validate that Nimbus now
truly has multiple adapter families sharing canonical primitives, and to clean
up the remaining boundary leaks before activating the deferred native transport
evolution work.

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `docs/reference/reliability-posture.md`
- `docs/reference/ci-failure-investigation.md`
- `docs/plans/archive/firebase-adapter-plan.md`
- `docs/plans/archive/firebase-cloud-functions-plan.md`
- `docs/plans/websocket-protocol-plan.md`
- `docs/plans/native-transport-evolution-plan.md`
- `docs/reference/firebase-compatibility.md`
- `docs/reference/firebase-migration-guide.md`
- `docs/reference/firebase-upstream-test-catalog.md`
- `crates/nimbus-server/src/adapters/firebase/mod.rs`
- `crates/nimbus-server/src/adapters/firebase/grpc/{mod,listen_stream,listen_websocket,write_stream}.rs`
- `crates/nimbus-server/src/adapters/cloud_functions/execution.rs`
- `crates/nimbus-server/src/adapters/convex/host_bridge/db_ops/firestore_admin.rs`
- `crates/nimbus-server/src/ws/negotiation.rs`
- `crates/nimbus-server/src/tests.rs`
- `packages/firebase/src/firestore.ts`
- `packages/firebase/src/internal/{grpc-web,listen-websocket}.ts`
- the current git worktree on 2026-04-26

---

## Status

- **Status:** `done`
- **Primary owner:** completed execution record retained as a baseline
- **Activation gate:** immediate; the archived Firebase adapter,
  Firebase Cloud Functions, and WebSocket protocol plans are all complete
- **Execution order:** this plan precedes
  `docs/plans/native-transport-evolution-plan.md`
- **Verification posture:** each roadmap item must record its own focused
  verification before it can close; the plan is no longer a docs-only review
  artifact

## Why This Exists

The Firebase and Cloud Functions waves accomplished the main goal of forcing a
second adapter family into Nimbus and hardening many shared primitives:

- resource paths
- atomic write batches
- structured query execution
- transaction sessions
- durable trigger delivery
- published compatibility contracts

That goal is only partially complete. The review found that the data-layer
primitives were promoted well, but the higher server/runtime seams still carry
adapter-specific leakage:

- Firebase principal propagation now exists on the covered data paths, but the
  repo still needs proof hardening and compatibility-truth cleanup around that
  contract
- Cloud Functions execution still reuses Convex-namespaced runtime-host types
  instead of a promoted shared runtime seam
- covered `firebase-admin/firestore` runtime ops still live under
  `adapters/convex/*`
- the published Firebase compatibility matrix is ahead of the recorded upstream
  `Write`-stream evidence
- the completed WebSocket plan still preserves legacy prelaunch compatibility
  behavior
- several new adapter roots violate the repo’s modularity thresholds

This plan closes those gaps before Nimbus starts another cross-cutting wave
such as native transport evolution.

## Relationship To Other Plans

- `docs/plans/archive/firebase-adapter-plan.md`
  is the completed execution record for the Firestore adapter and the shared
  primitive hardening that preceded this plan.
- `docs/plans/archive/firebase-cloud-functions-plan.md`
  is the completed execution record for Cloud Functions compatibility and the
  durable trigger/runtime artifact work that preceded this plan.
- `docs/plans/websocket-protocol-plan.md`
  remains the completed protocol baseline for structured errors and
  version-negotiated WebSocket framing.
- `docs/plans/native-transport-evolution-plan.md`
  remains deferred and should not activate until this plan closes the adapter
  boundary, compatibility-truth, and prelaunch protocol cleanup items.
- This plan is not a general feature roadmap. It is a targeted architecture,
  compatibility-truth, and modularity hardening wave driven by the now-landed
  multiple-adapter reality.

## Scope

This plan covers:

- Firebase principal propagation and application-auth truth across REST, gRPC,
  gRPC-Web, `Write`, and `Listen`
- Cloud Functions runtime-host seam promotion out of Convex-only namespaces
- relocation of covered `firebase-admin/firestore` runtime ops onto shared
  runtime-host boundaries
- reconciliation of published Firebase compatibility claims with upstream
  Firestore evidence
- removal of prelaunch WebSocket legacy compatibility paths that no longer fit
  repo policy
- modular decomposition of the worst new adapter and proof hotspots
- docs and plan-index refresh needed to keep later waves honest

This plan does not cover:

- new Firebase product breadth
- Firebase WebChannel support
- new Cloud Functions handler families beyond the already published scope
- native transport codec or WebTransport work
- new Convex product features
- a generic repo-wide maintainability sweep unrelated to the multi-adapter
  findings below

## Control Plan Rules

1. Shared semantics must live above adapter namespaces.
   If behavior is needed by Convex plus Firebase plus Cloud Functions, it does
   not belong under `adapters/convex/*` or `adapters/firebase/*`.

2. Identity must enter once at the server edge and flow through shared
   principal-aware engine APIs.
   No adapter may advertise auth-token support while still executing as
   `PrincipalContext::anonymous()`.

3. Public compatibility docs may not outrun the strongest verified evidence.
   If upstream stock SDK evidence is narrower than the first-party SDK claim,
   the docs must say so plainly.

4. Prelaunch breaking cleanup is preferred.
   Remove stale protocol and compatibility shims instead of preserving them for
   hypothetical old clients.

5. Modularity thresholds in `AGENTS.md` are mandatory here.
   Files at 2,000+ lines must be decomposed or justified explicitly. Files in
   the 1,500-1,999 range need an owning-plan justification if retained.

6. Split by ownership, not by raw line count.
   Move behavior into concept-owned children and keep composition roots thin.

## Current Assessed State

- Shared database primitives are in meaningfully better shape than before the
  Firebase wave: resource paths, structured queries, atomic write batches,
  transaction sessions, and durable trigger invocation records now exist in
  `nimbus-core`, `nimbus-engine`, and `nimbus-storage` rather than only inside
  adapter code.
- Firebase server execution now resolves and propagates principals on the
  covered REST, gRPC, `Write`, and `Listen` paths, but the public docs and
  compatibility matrix needed a proof-aligned cleanup to match that reality.
- Cloud Functions runtime execution still depends on
  `ConvexHostBridge`, `ConvexHostBridgeScope`, and `ConvexRegistry`, which
  means the second adapter family is still borrowing the first adapter’s
  runtime integration surface rather than a promoted shared abstraction.
- The covered `firebase-admin/firestore` operations are implemented as methods
  on `ConvexHostBridge`, which is a naming and ownership leak even if the
  underlying behavior is reusable.
- The published Firebase compatibility matrix currently claims more transport
  breadth than the upstream Firestore smoke catalog proves.
- The completed WebSocket plan still preserves `ImplicitV1` / undeclared
  fallback behavior that does not fit the repo’s prelaunch “no compatibility
  shims” posture.
- The largest new hotspots from these landed waves are currently:
  - `packages/firebase/src/firestore.ts` — 1975 lines after the ownership split
  - `crates/nimbus-server/src/tests.rs` — 7614 lines
  - `crates/nimbus-server/src/adapters/cloud_functions/execution.rs` — 1420 lines
  - `crates/nimbus-server/src/adapters/firebase/mod.rs` — 672 lines after the
    route/response/operation split; the Firebase server ownership wave is now
    spread across `mod.rs`, `operations.rs`, `response.rs`, and `errors.rs`
  These do not all need the same treatment, but the first two now clearly
  need plan-owned decomposition and the Firebase server root no longer
  requires a threshold exception.

## Context Window Scale

Use these sizing bands before loading code:

| Band | Estimated context | Expected shape |
| --- | --- | --- |
| `S` | 8k-12k tokens, 6-10 files | one narrow contract or docs-only slice |
| `M` | 12k-18k tokens, 8-14 files | one behavior seam plus focused proofs |
| `L` | 18k-28k tokens, 12-20 files | one cross-layer implementation slice |
| `XL` | 28k-40k tokens, 18-30 files | only for decomposition or end-to-end compatibility passes; split if possible |

Rule:

- If execution needs more than the estimated band for an item, split the item
  in this plan before continuing.

## Roadmap Summary

| Item | Focus | Depends on | Context estimate | Status |
| --- | --- | --- | --- | --- |
| `MAB1` | Firebase auth contract and principal-entry baseline | — | `M` | `done` |
| `MAB2` | Firebase principal propagation across all server paths | `MAB1` | `L` | `done` |
| `MAB3` | Firebase auth proofs and published compatibility truth | `MAB2` | `M` | `done` |
| `MAB4` | Shared runtime-host seam extraction out of Convex namespace | — | `L` | `done` |
| `MAB5` | Rehome `firebase-admin/firestore` runtime ops onto shared seam | `MAB4` | `L` | `done` |
| `MAB6` | Stock Firestore `Write` truth pass: fix or narrow claim | `MAB2`, `MAB3` | `L` | `done` |
| `MAB7` | Prelaunch WebSocket legacy-path removal | `MAB6` | `M` | `done` |
| `MAB8` | Split `packages/firebase/src/firestore.ts` by ownership | `MAB2`, `MAB3` | `XL` | `done` |
| `MAB9` | Split `crates/nimbus-server/src/adapters/firebase/mod.rs` by ownership | `MAB2`, `MAB3` | `L` | `done` |
| `MAB10` | Split `crates/nimbus-server/src/tests.rs` into concept-owned suites | `MAB6`, `MAB7`, `MAB9` | `XL` | `done` |
| `MAB11` | Closeout docs, plan-index, and native-transport gate refresh | `MAB5`, `MAB6`, `MAB7`, `MAB8`, `MAB9`, `MAB10` | `S` | `done` |

## Roadmap

### MAB1 — Firebase Auth Contract And Principal-Entry Baseline

Define and document one canonical Firebase application-auth contract across:

- REST unary
- native gRPC
- gRPC-Web unary
- native `Write`
- native `Listen`
- browser WebSocket `Listen`

Scope:

- identify which headers, tokens, and emulator-only shapes are accepted
- define how those inputs resolve into a `PrincipalContext`
- record what remains intentionally unsupported
- correct any docs that currently imply auth works end to end when it does not

Completion gate:

- one documented server-edge contract exists for Firebase application auth
- server and SDK docs no longer imply auth semantics that the server does not
  honor
- the next item has a clear principal-extraction target instead of multiple ad
  hoc token paths

Focused verification:

- docs consistency review across compatibility and migration docs
- focused proof that every Firebase entrypoint under review is covered by the
  contract doc or explicitly marked unsupported

### MAB2 — Firebase Principal Propagation Across All Server Paths

Replace anonymous-principal execution in the Firebase adapter with canonical
principal-aware flow.

Scope:

- REST commit, batch-get, run-query, aggregations, transactions, and
  batch-write
- gRPC unary and streaming write/read surfaces
- `Listen` over native gRPC and browser WebSocket upgrade
- emulator-specific token behavior only where explicitly documented

Completion gate:

- Firebase requests stop hardcoding `PrincipalContext::anonymous()` on covered
  paths
- the resolved principal reaches the same shared engine APIs used elsewhere
- watch flows and unary flows use one server-owned identity story

Focused verification:

- focused server tests for authenticated versus anonymous reads/writes
- watch tests proving identity-sensitive subscriptions do not silently run as
  anonymous
- package selftests updated to assert end-to-end server behavior, not only
  emitted headers

### MAB3 — Firebase Auth Proofs And Published Compatibility Truth

Bring the public compatibility docs and proof surface in line with what is
actually verified.

Scope:

- `docs/reference/firebase-compatibility.md`
- `docs/reference/firebase-migration-guide.md`
- `docs/reference/firebase-websocket-listen.md`
- `packages/firebase/src/selftest.mjs`
- focused server contract tests

Completion gate:

- the published Firebase docs state exactly what auth and listener semantics
  are covered
- first-party SDK behavior, server behavior, and docs no longer disagree

Focused verification:

- focused `@nimbus/firebase` selftest lanes
- focused server auth/listen contract lanes

### MAB4 — Shared Runtime-Host Seam Extraction

Promote the runtime-host seam used by Convex and Cloud Functions out of the
Convex adapter namespace.

Scope:

- extract shared invocation scope and shared runtime DB/service host behavior
- leave Convex-specific authoring and registry semantics in the Convex adapter
- give Cloud Functions a shared runtime integration surface that does not
  depend on Convex-named types

Completion gate:

- Cloud Functions trigger execution no longer constructs itself out of
  `ConvexHostBridge`, `ConvexHostBridgeScope`, or `ConvexRegistry`
- shared runtime-host concerns live under a server-owned shared seam
- adapter-specific surfaces only own their authoring and protocol behavior

Focused verification:

- focused Cloud Functions execution tests
- focused Convex runtime tests for regression protection

### MAB5 — Rehome `firebase-admin/firestore` Runtime Ops

Move the covered admin Firestore operations onto the shared runtime-host
surface created in `MAB4`.

Scope:

- reads and writes currently implemented in
  `adapters/convex/host_bridge/db_ops/firestore_admin.rs`
- shared atomic batch staging path reuse
- Cloud Functions and any future runtime consumers

Completion gate:

- covered `firebase-admin/firestore` runtime ops no longer live under
  `adapters/convex/*`
- operation ownership matches semantics rather than historical placement

Focused verification:

- focused Cloud Functions admin Firestore tests
- focused runtime host-call regression tests

### MAB6 — Stock Firestore `Write` Truth Pass

Resolve the current mismatch between the published compatibility matrix and the
upstream Firestore catalog around the raw `Write` bidi RPC.

Scope:

- reproduce the upstream Node Firestore smoke against the current server
- determine whether the remaining gap is:
  - actual route/wiring failure
  - protocol mismatch
  - unsupported upstream client behavior outside the first-party claim
- then either:
  - fix the server/client path, or
  - narrow the published claim explicitly

Completion gate:

- the compatibility matrix and upstream catalog no longer disagree
- the repo has one honest answer for whether raw Firestore `Write` is part of
  the current compatibility claim

Focused verification:

- representative upstream Firestore node repro
- focused Nimbus server lanes for the actual root cause

### MAB7 — Prelaunch WebSocket Legacy-Path Removal

Delete the remaining legacy WebSocket compatibility behavior that no longer
fits prelaunch policy.

Scope:

- remove implicit no-subprotocol fallback
- decide and execute the explicit v1-removal or v2-only transition for
  first-party clients
- keep one canonical structured error path

Completion gate:

- the server requires explicit negotiated protocol use
- undeclared legacy fallback is gone
- first-party clients, tests, and docs all match the new protocol baseline

Focused verification:

- focused WebSocket protocol tests
- focused JS SDK client connection tests

### MAB8 — Split `packages/firebase/src/firestore.ts`

Decompose the Firebase first-party SDK root by ownership.

Target direction:

- public surface and exports
- path/reference modeling
- write lowering and `FieldValue`
- query building and execution
- transactions
- watch/listener integration
- transport/auth helpers

Completion gate:

- `firestore.ts` is reduced to a thin composition root or public entrypoint
- no behavior changes are introduced by the split
- each moved module owns one coherent concept family

Focused verification:

- package typecheck, tests, and build
- focused smoke for CRUD, query, watch, batch, and transaction paths

### MAB9 — Split `crates/nimbus-server/src/adapters/firebase/mod.rs`

Decompose the server Firebase adapter root by ownership.

Target direction:

- HTTP handlers
- shared execution helpers
- resource mapping
- serializer/error mapping
- transaction helpers
- query/aggregation helpers

Completion gate:

- `mod.rs` becomes a thin composition root
- shared Firebase execution behavior stays shared rather than copied into
  transport-specific children

Focused verification:

- focused `cargo test -p nimbus-server firebase --lib`
- focused gRPC and REST lanes covering touched modules

### MAB10 — Split `crates/nimbus-server/src/tests.rs`

Move the Firebase, Cloud Functions, and related compatibility proofs into
concept-owned test modules.

Scope:

- Firebase transport and compatibility suites
- Cloud Functions runtime/artifact/HTTP suites
- shared helper/support seams near the owning test families

Completion gate:

- `crates/nimbus-server/src/tests.rs` is no longer a multi-thousand-line proof
  catch-all
- proof ownership is clearer and narrower
- no focused verification lane becomes harder to run

Focused verification:

- focused server test lanes per moved suite
- `cargo fmt --all --check`

### MAB11 — Closeout Docs And Native-Transport Gate Refresh

Close the wave by refreshing the public and planning docs that depend on these
boundary decisions.

Scope:

- `docs/plans/README.md`
- `docs/reference/firebase-compatibility.md`
- `docs/reference/firebase-upstream-test-catalog.md`
- `docs/reference/cloud-functions-compatibility.md`
- `docs/plans/native-transport-evolution-plan.md`

Completion gate:

- docs and plan registry reflect the cleaned-up compatibility truth
- native transport remains deferred behind an honest, current baseline

Focused verification:

- docs consistency review
- `rg` checks for stale claims or old boundary wording

## Execution Order

Run this plan in six passes:

1. `MAB1` → `MAB3`
   so Firebase identity and docs stop lying before other cleanup builds on them.
2. `MAB4` → `MAB5`
   so the runtime-host seam becomes genuinely multi-adapter.
3. `MAB6`
   so published compatibility truth matches upstream evidence.
4. `MAB7`
   so prelaunch protocol cleanup happens before native transport follow-on work.
5. `MAB8` → `MAB10`
   so decomposition happens after semantics settle.
6. `MAB11`
   for closeout and deferred-plan gate refresh.

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-26 | Plan authored | `done` | Created from the post-Firebase / post-Cloud-Functions / post-WebSocket architecture review. The active findings are: Firebase identity is still dropped to anonymous on the server, Cloud Functions still depends on Convex-namespaced runtime-host seams, raw Firestore `Write` compatibility claims are ahead of the upstream evidence, prelaunch WebSocket v1 fallback still exists, and the largest new adapter/test roots need ownership-based decomposition. |
| 2026-04-26 | `MAB1` Firebase auth contract and principal-entry baseline | `done` | Added `docs/reference/firebase-auth-contract.md` and updated `AGENTS.md`, the Firebase compatibility docs, the migration guide, and the browser `Listen` reference so the repo now says one exact thing: `@nimbus/firebase` can emit bearer-shaped auth inputs, but the current Firebase server routes still resolve covered requests as `PrincipalContext::anonymous()`. This closes the docs/contract gap and gives `MAB2` one explicit principal-entry target. |
| 2026-04-26 | `MAB2` Firebase principal propagation across all server paths | `done` | Added one shared Firebase application-auth resolver in `crates/nimbus-server/src/application_auth.rs` and threaded the resolved `PrincipalContext` through covered REST handlers, gRPC unary methods, native `Write`, native `Listen`, and browser WebSocket `Listen`. Covered Firebase reads/writes/transactions/listeners no longer hardcode `PrincipalContext::anonymous()`, and focused server tests plus the `@nimbus/firebase` smoke lane now prove authenticated versus anonymous behavior across unary, write-stream, and watch flows. |
| 2026-04-26 | `MAB3` Firebase auth proofs and published compatibility truth | `done` | Reconciled the published Firebase docs with the landed principal flow in `docs/reference/firebase-auth-contract.md`, `firebase-compatibility.md`, `firebase-migration-guide.md`, and `firebase-websocket-listen.md`. The current public story is now consistent: covered Firebase CRUD/query/transaction/`Write`/`Listen` paths enforce the resolved principal contract, while broader upstream Firebase/Admin parity remains explicitly unclaimed. Focused verification came from the server auth/listen contract tests and the `@nimbus/firebase` selftest smoke lane. |
| 2026-04-26 | `MAB4` Shared runtime-host seam extraction | `done` | Added a new server-owned `crates/nimbus-server/src/runtime_host/mod.rs` seam with `RuntimeHostEnvironment`, `RuntimeHostBridgeScope`, `RuntimeHostBridgeInvocation`, and `RuntimeHostBridge`, then switched Cloud Functions trigger/http execution and the top-level Convex runtime invocation context to build through that shared seam instead of constructing `ConvexHostBridge*` types directly. The initial verification was temporarily blocked by unrelated MongoDB compile churn, but the focused proofs are now green: `cargo test -p nimbus-server cloud_functions --lib` and `cargo test -p nimbus-server adapters::convex::tests::authorization --lib` both pass. |
| 2026-04-26 | `MAB5` Rehome `firebase-admin/firestore` runtime ops onto shared seam | `done` | Moved the covered admin Firestore host-call implementation out of `adapters/convex/host_bridge/db_ops/firestore_admin.rs` into `crates/nimbus-server/src/runtime_host/firestore_admin.rs`, rewired Convex document host-call dispatch through that shared module, and exposed only the minimal server-owned imports needed to keep the runtime ABI stable. Focused proof comes from the shared Cloud Functions lane, including the generated Firebase bundle path that exercises `firebase-admin/firestore` reads and writes end to end. |
| 2026-04-26 | `MAB6` Stock Firestore `Write` truth pass | `done` | Chose the narrow-truth path instead of inventing a broader compatibility claim. Updated `docs/reference/firebase-compatibility.md` and `docs/reference/firebase-migration-guide.md` so the public contract is now explicit: the first-party `@nimbus/firebase` path is supported, but raw stock upstream Firestore `Write` streaming compatibility remains unclaimed while the upstream Node smoke catalog still records `GrpcConnection RPC 'Write' stream ... 12 UNIMPLEMENTED`. The compatibility matrix and upstream catalog no longer disagree. |
| 2026-04-26 | `MAB7` Prelaunch WebSocket legacy-path removal | `done` | Removed implicit no-subprotocol fallback and explicit `nimbus.v1` negotiation from the server, updated local server discovery and public error metadata to advertise only `nimbus.v2`, switched the first-party JS client and the shared demo subscription client to explicit `nimbus.v2` plus `client_hello`, and updated the WebSocket protocol/error docs to match the single supported prelaunch baseline. Focused verification is green across `cargo test -p nimbus-server websocket_protocol --lib`, `cargo test -p nimbus-server cloud_functions --lib`, `cargo test -p nimbus-server adapters::convex::tests::authorization --lib`, and `npm run typecheck --workspace nimbus`. |
| 2026-04-26 | `MAB8` Firebase SDK `firestore.ts` decomposition sizing and first split | `in_progress` | The next active slice is the 3.5k-line `packages/firebase/src/firestore.ts` ownership split. Start by sizing the module into public surface, path/reference modeling, watch transport/session handling, write lowering, transaction control flow, and snapshot/query shaping so the first split can move one coherent ownership block instead of carving mechanically by line count. |
| 2026-04-26 | `MAB8` Firebase SDK ownership decomposition progress | `done` | Completed the `packages/firebase/src/firestore.ts` ownership split without behavior drift. Added `packages/firebase/src/internal/watch.ts`, `internal/auth.ts`, `internal/document-data.ts`, `internal/writes.ts`, `internal/unary.ts`, and `internal/watch-snapshots.ts`, so browser `Listen`, auth/subprotocol shaping, document value helpers, write lowering, unary/query/transaction transport execution, and watched-query snapshot shaping now live in concept-owned children instead of the public root. Focused verification stayed green throughout with `npm run typecheck --workspace @nimbus/firebase`, `npm run test --workspace @nimbus/firebase`, and `npm run build --workspace @nimbus/firebase`, and `firestore.ts` now sits at 1975 lines. That keeps it below the hard 2000-line threshold; the remaining size is the public API/entrypoint composition surface, which is intentionally retained under this plan as the last unsplit SDK root. |
| 2026-04-26 | `MAB9` Firebase server `mod.rs` decomposition sizing | `in_progress` | The next active slice is the 1553-line `crates/nimbus-server/src/adapters/firebase/mod.rs` composition root. Start by sizing the ownership blocks into HTTP handlers, shared execution helpers, resource mapping, serializer/error mapping, transaction helpers, and query/aggregation helpers so the first split can move one coherent server concept family without duplicating REST and gRPC behavior. |
| 2026-04-26 | `MAB9` Firebase server ownership decomposition progress | `done` | Completed the Firebase server root split in two coherent cuts. Moved REST and gRPC error/status shaping into `crates/nimbus-server/src/adapters/firebase/errors.rs`, moved REST response/document serialization into `response.rs`, then moved shared database-operation helpers into `operations.rs`. `crates/nimbus-server/src/adapters/firebase/mod.rs` is now a 672-line route/composition root instead of a 1.5k mixed adapter switchboard. Focused verification is green with `cargo fmt --all --check`, `cargo check -p nimbus-server`, and `cargo test -p nimbus-server cloud_functions --lib`, which also proves the Firebase refactor did not bleed across the second adapter family. |
| 2026-04-26 | `MAB10` server proof-surface split sizing | `in_progress` | The next active slice is the 7614-line `crates/nimbus-server/src/tests.rs` proof root. It already delegates many concept suites through bottom-of-file `mod` declarations, which means the remaining work should start by separating shared fixture/helper families and any still-inline Firebase/WebSocket/local-server proofs into concept-owned children instead of carving blindly by line count. |
| 2026-04-26 | `MAB10` server proof-surface decomposition progress | `done` | Split the remaining inline Firebase proof slab out of `crates/nimbus-server/src/tests.rs` into seven concept-owned suites under `crates/nimbus-server/src/tests/firebase/`: `rest_and_cors.rs`, `grpc_unary.rs`, `write_stream.rs`, `listen.rs`, `auth_and_availability.rs`, `rest_crud.rs`, and `rest_query.rs`. The root proof file now sits at 1057 lines, and every extracted Firebase suite stays below the hard 2000-line threshold. Focused verification is green with `cargo fmt --all --check`, `cargo check -p nimbus-server`, `cargo test -p nimbus-server cloud_functions --lib`, and `cargo test -p nimbus-server firebase_grpc_commit_executes_atomic_batch_and_consumes_transaction_token --lib`. |
| 2026-04-26 | `MAB11` closeout docs and native-transport gate refresh | `done` | Closed the wave by refreshing this plan to `done`, moving it out of the active-plan posture in `docs/plans/README.md`, updating `AGENTS.md` so Firebase/Cloud Functions work treats this plan as the completed multi-adapter baseline instead of the current owner, and tightening `docs/plans/native-transport-evolution-plan.md` so its deferred posture explicitly depends on the now-complete boundary-hardening wave. The repo’s adapter-boundary guidance, active-plan registry, and deferred native-transport gate now agree. |
