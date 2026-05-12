# Cloud Functions Compatibility

This reference records the currently implemented Cloud Functions-compatible
surface in Nimbus for both Firebase v2 authors and standalone
`@google-cloud/functions-framework` authors.

Use this document as the precise compatibility matrix. For the practical
adoption path, see
[Cloud Functions migration guide](cloud-functions-migration-guide.md).

## Status Labels

| Label | Meaning |
| --- | --- |
| `supported` | Implemented and exercised by focused tests or generated-bundle smoke. |
| `supported with caveats` | Usable now, but with an intentionally narrow option matrix or an explicit runtime boundary. |
| `deferred` | Explicitly outside the current Nimbus Cloud Functions claim. |
| `not claimed` | No compatibility promise yet. |

## Audience Snapshot

| Audience | Status | Notes |
| --- | --- | --- |
| Firebase v2 Firestore trigger authors | `supported` | Covered `firebase-functions/v2` imports and Firestore trigger helpers execute without source rewrites on Nimbus. |
| Firebase v2 HTTPS authors (`onRequest`, `onCall`) | `supported with caveats` | Base overloads are covered; option matrices stay intentionally narrow and fail fast outside the documented slice. |
| Standalone Functions Framework CloudEvent authors | `supported` | `functions.cloudEvent(name, handler)` is covered with deploy-time `targets.json` bindings. |
| Standalone Functions Framework HTTP authors | `supported` | `functions.http(name, handler)` is covered on the Nimbus-hosted HTTP surface. |
| Standalone Functions Framework local dev server parity | `deferred` | Nimbus hosts execution inside its own server; it does not claim full `FUNCTION_TARGET` / generic framework web-server parity. |

## Authoring Surface Matrix

| Surface | Status | Current claim | Explicit caveats |
| --- | --- | --- | --- |
| `firebase-functions/v2/firestore onDocumentCreated` | `supported` | Runs through the shared durable trigger registry and Firestore CloudEvent model. | Default database only. |
| `firebase-functions/v2/firestore onDocumentUpdated` | `supported` | Real updates fire with Firebase-shaped event objects. | No-op overwrites do not emit update events. |
| `firebase-functions/v2/firestore onDocumentDeleted` | `supported` | Covered on the shared trigger path. | Default database only. |
| `firebase-functions/v2/firestore onDocumentWritten` | `supported` | Covered on the shared trigger path, including retry inheritance. | Default database only. |
| `firebase-functions/v2 setGlobalOptions()` | `supported with caveats` | Covered only for documented first-slice root defaults. | Only `retry` is inherited for Firestore document triggers. |
| `firebase-functions/v2/https onRequest(handler)` | `supported` | Generated handlers run at `/<exportName>` on the Nimbus server. | Covered base overload only; unsupported `HttpsOptions` fail fast. |
| `firebase-functions/v2/https onRequest({}, handler)` | `supported` | Same runtime path as `functions.http()`. | Empty-options overload only. |
| `firebase-functions/v2/https onCall(handler)` | `supported` | Callable JSON envelope, default CORS behavior, and `HttpsError` mapping are covered. | App Check verification is deferred; option matrix is narrow. |
| `firebase-functions/v2/https onCall({}, handler)` | `supported` | Same callable protocol as `onCall(handler)`. | Empty-options overload only. |
| `@google-cloud/functions-framework functions.cloudEvent(name, handler)` | `supported` | Covered with deploy-time `targets.json` binding metadata. | Requires a binding entry in `.nimbus/firebase/targets.json`. |
| `@google-cloud/functions-framework functions.http(name, handler)` | `supported` | Covered on the Nimbus-hosted HTTP surface. | Requires a binding entry in `.nimbus/firebase/targets.json`. |

## App Discovery And Project Layout

| Concern | Status | Current claim |
| --- | --- | --- |
| Firebase `firebase.json` + `functions.source` discovery | `supported` | Nimbus auto-detects the nearest compatible Firebase app root from the current directory or its parents. |
| Firebase multi-codebase mapping | `supported` | `firebase.json` `codebase` / `source` layouts are preserved. |
| Standalone Functions Framework package discovery | `supported` | Nimbus auto-detects a package root when `package.json` declares `@google-cloud/functions-framework`. |
| Explicit `--app-dir` override | `supported` | Remains authoritative for ambiguous or nonstandard repos. |
| Generated artifact root | `supported` | Cloud Functions outputs live under `.nimbus/firebase/`. |

## Delivery And Execution Semantics

| Concern | Status | Current claim |
| --- | --- | --- |
| Delivery model | `supported` | At-least-once delivery backed by a durable invocation ledger plus journal-backed materialization cursor. |
| Retry behavior | `supported` | Retryable failures persist durable retry state and replay after delay or restart. |
| Crash / restart replay | `supported` | Pending and due-retry invocations are replayed after restart. |
| Trigger execution principal | `supported` | Firestore document triggers execute under a service principal, not the calling end-user principal. |
| Chain depth limiting | `supported` | Recursive trigger chains stop at the configured depth budget instead of looping forever. |
| No-op update suppression | `supported` | No-op overwrites do not emit `onDocumentUpdated()` events. |
| Named databases | `deferred` | Current coverage is only for Firestore `(default)`. |

## Covered `firebase-admin` Matrix

The first covered admin slice is intentionally narrow and source-compatible.

| Module / helper | Status | Current claim |
| --- | --- | --- |
| `firebase-admin/app initializeApp()` | `supported` | Default-app and app-name lifecycle is covered for generated Cloud Functions bundles. |
| `firebase-admin/app getApp()` / `getApps()` / `deleteApp()` | `supported` | Covered in the first-slice admin lifecycle shim. |
| `firebase-admin/firestore getFirestore()` | `supported` | Covered for the default database handle. |
| `firestore.collection(path)` / `doc(path)` / nested `collection(path)` | `supported` | Covered for the documented path-ref subset. |
| `DocumentReference.get()` | `supported` | Covered. |
| `DocumentReference.set()` | `supported` | Covered. |
| `DocumentReference.update()` | `supported` | Covered. |
| `DocumentReference.delete()` | `supported` | Covered. |
| `DocumentSnapshot.data()` / `get(fieldPath)` | `supported` | Covered. |
| `Timestamp` helpers used by the current fixtures | `supported` | Covered in the first-slice shim. |
| Other `firebase-admin` modules or Firestore admin helpers | `deferred` | Unsupported methods must fail clearly rather than silently stub. |

## Options Matrix

### Root defaults

| Surface | Status | Covered fields |
| --- | --- | --- |
| `setGlobalOptions()` for Firestore document triggers | `supported with caveats` | `retry` only |
| `setGlobalOptions()` for `onRequest()` / `onCall()` | `supported with caveats` | none inherited in the current slice |
| `onInit()` | `deferred` | Explicit fail-fast boundary |

For the exact root-default contract, see
[Cloud Functions root defaults contract](cloud-functions-root-defaults-contract.md).

### Per-handler options

| Surface | Status | Covered contract |
| --- | --- | --- |
| `DocumentOptions` | `supported with caveats` | Narrow first-slice document trigger fields only; unsupported fields fail validation. |
| `HttpsOptions` | `supported with caveats` | Base `onRequest(handler)` and `onRequest({}, handler)` overloads only; explicit option fields fail fast. |
| `CallableOptions` | `supported with caveats` | Base `onCall(handler)` and `onCall({}, handler)` overloads only; explicit option fields fail fast. |

## HTTP And Callable Notes

| Concern | Status | Current claim |
| --- | --- | --- |
| `functions.http(name, handler)` public path | `supported` | Comes from `targets.json` binding metadata. |
| Firebase `onRequest` public path | `supported` | Derived from the exported function name as `/<exportName>`. |
| Firebase `onCall` public path | `supported` | Derived from the exported function name as `/<exportName>`. |
| Callable JSON envelope | `supported` | Covered. |
| Callable default CORS behavior | `supported` | Covered. |
| `HttpsError` / `FunctionsErrorCode` mapping | `supported` | Covered. |
| Callable auth context with shared Nimbus application auth enabled | `supported with caveats` | The runtime path supports it, but the current generated smoke primarily covers the default unauthenticated baseline. |
| App Check verification | `deferred` | Explicit fail-fast boundary. |

## Known Non-Goals And Gaps

- Full standalone Functions Framework local web-server parity.
- Firebase Emulator Suite control-plane parity.
- Non-default Firestore database routing.
- Background triggers outside Firestore document events.
- Auth-context Firestore trigger variants.
- Broad `firebase-admin` parity beyond the documented first slice.
- Broader `GlobalOptions`, `DocumentOptions`, `HttpsOptions`, or
  `CallableOptions` matrices beyond the covered base overloads.

## Verification Basis

This matrix is sourced from:

- the Cloud Functions control plan and execution log in
  `docs/plans/archive/firebase-cloud-functions-plan.md`,
- generated-bundle selftests in `packages/codegen/src/selftest/cloud_functions_fixtures.mjs`,
- live trigger and HTTP integration tests in
  `crates/nimbus-server/src/adapters/cloud_functions/{execution,http}.rs`,
- the shared engine trigger replay/retry tests in
  `crates/nimbus-engine/src/tests/mutation_journal/triggers.rs`,
- and the published contract docs for artifacts, bindings, root defaults, app
  discovery, and the covered admin surface.
