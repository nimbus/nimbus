---
title: Cloud Functions compatibility
description: The precise Cloud Functions compatibility surface in Nimbus — authoring matrix, delivery semantics, admin slice, and option boundaries.
sidebar:
  order: 1
---

This reference records the currently implemented Cloud Functions-compatible
surface in Nimbus for both Firebase v2 authors and standalone
`@google-cloud/functions-framework` authors.

For the practical adoption path, see
[Migrate Cloud Functions](/developers/cloud-functions/migrate/).

## Status labels

| Label | Meaning |
| --- | --- |
| `supported` | Implemented and exercised by tests or generated-bundle smoke. |
| `supported with caveats` | Usable now, but with an intentionally narrow option matrix or an explicit runtime boundary. |
| `deferred` | Explicitly outside the current claim. |
| `not claimed` | No compatibility promise yet. |

## Audience snapshot

| Audience | Status | Notes |
| --- | --- | --- |
| Firebase v2 Firestore trigger authors | `supported` | Covered `firebase-functions/v2` imports and Firestore trigger helpers execute without source rewrites. |
| Firebase v2 HTTPS authors (`onRequest`, `onCall`) | `supported with caveats` | Base overloads are covered; option matrices stay intentionally narrow and fail fast outside the documented slice. |
| Standalone Functions Framework CloudEvent authors | `supported` | `functions.cloudEvent(name, handler)` is covered with deploy-time `targets.json` bindings. |
| Standalone Functions Framework HTTP authors | `supported` | `functions.http(name, handler)` is covered on the Nimbus-hosted HTTP surface. |
| Standalone Functions Framework local dev server parity | `deferred` | Nimbus hosts execution inside its own server; no full `FUNCTION_TARGET` / generic framework web-server parity claim. |

## Authoring surface matrix

| Surface | Status | Current claim | Explicit caveats |
| --- | --- | --- | --- |
| `onDocumentCreated` | `supported` | Runs through the durable trigger registry and Firestore CloudEvent model. | Default database only. |
| `onDocumentUpdated` | `supported` | Real updates fire with Firebase-shaped event objects. | No-op overwrites do not emit update events. |
| `onDocumentDeleted` | `supported` | Covered on the shared trigger path. | Default database only. |
| `onDocumentWritten` | `supported` | Covered on the shared trigger path, including retry inheritance. | Default database only. |
| `setGlobalOptions()` | `supported with caveats` | Covered only for documented first-slice root defaults. | Only `retry` is inherited, and only for Firestore document triggers. |
| `onRequest(handler)` | `supported` | Handlers serve at `/<exportName>` on the Nimbus server. | Base overload only; unsupported `HttpsOptions` fail fast. |
| `onRequest({}, handler)` | `supported` | Same runtime path as `functions.http()`. | Empty-options overload only. |
| `onCall(handler)` | `supported` | Callable JSON envelope, default CORS behavior, and `HttpsError` mapping are covered. | App Check verification is deferred; option matrix is narrow. |
| `onCall({}, handler)` | `supported` | Same callable protocol as `onCall(handler)`. | Empty-options overload only. |
| `functions.cloudEvent(name, handler)` | `supported` | Covered with deploy-time `targets.json` binding metadata. | Requires a binding entry in `.nimbus/firebase/targets.json`. |
| `functions.http(name, handler)` | `supported` | Covered on the Nimbus-hosted HTTP surface. | Requires a binding entry in `.nimbus/firebase/targets.json`. |

## App discovery and project layout

| Concern | Status | Current claim |
| --- | --- | --- |
| Firebase `firebase.json` + `functions.source` discovery | `supported` | The dev loop auto-detects the nearest compatible Firebase app root from the current directory or its parents. |
| Firebase multi-codebase mapping | `supported` | `firebase.json` `codebase` / `source` layouts are preserved. |
| Standalone Functions Framework package discovery | `supported` | A package root is auto-detected when `package.json` declares `@google-cloud/functions-framework`. |
| Explicit `--app-dir` override | `supported` | Authoritative for ambiguous or nonstandard repos; required for `nimbus start`, which does no walk-up. |
| Generated artifact root | `supported` | Cloud Functions outputs live under `.nimbus/firebase/` (`artifact.json`, `targets.json`, `bundle.mjs`, `bundle.sha256`). |

## Delivery and execution semantics

| Concern | Status | Current claim |
| --- | --- | --- |
| Delivery model | `supported` | At-least-once delivery backed by a durable invocation ledger plus a journal-backed materialization cursor. |
| Retry behavior | `supported` | Retryable failures persist durable retry state and replay after delay or restart. |
| Crash / restart replay | `supported` | Pending and due-retry invocations are replayed after restart. |
| Trigger execution principal | `supported` | Firestore document triggers execute under a service principal, not the calling end-user principal. |
| Chain depth limiting | `supported` | Recursive trigger chains stop at the configured depth budget instead of looping forever. |
| No-op update suppression | `supported` | No-op overwrites do not emit `onDocumentUpdated()` events. |
| Named databases | `deferred` | Coverage is only for Firestore `(default)`. |

## Covered `firebase-admin` matrix

The covered admin slice is intentionally narrow and source-compatible.

| Module / helper | Status |
| --- | --- |
| `firebase-admin/app` `initializeApp()` | `supported` |
| `firebase-admin/app` `getApp()` / `getApps()` / `deleteApp()` | `supported` |
| `firebase-admin/firestore` `getFirestore()` (default database) | `supported` |
| `collection(path)` / `doc(path)` / nested `collection(path)` | `supported` |
| `DocumentReference.get()` / `set()` / `update()` / `delete()` | `supported` |
| `DocumentSnapshot.data()` / `get(fieldPath)` | `supported` |
| Covered `Timestamp` helpers | `supported` |
| Other `firebase-admin` modules or Firestore admin helpers | `deferred` — unsupported methods fail clearly rather than silently stub |

## Options matrix

### Root defaults

| Surface | Status | Covered fields |
| --- | --- | --- |
| `setGlobalOptions()` for Firestore document triggers | `supported with caveats` | `retry` only |
| `setGlobalOptions()` for `onRequest()` / `onCall()` | `supported with caveats` | None inherited in the current slice |
| `onInit()` | `deferred` | Explicit fail-fast boundary |

### Per-handler options

| Surface | Status | Covered contract |
| --- | --- | --- |
| `DocumentOptions` | `supported with caveats` | Narrow first-slice document trigger fields only; unsupported fields fail validation. |
| `HttpsOptions` | `supported with caveats` | Base `onRequest(handler)` and `onRequest({}, handler)` overloads only; explicit option fields fail fast. |
| `CallableOptions` | `supported with caveats` | Base `onCall(handler)` and `onCall({}, handler)` overloads only; explicit option fields fail fast. |

## HTTP and callable notes

| Concern | Status | Current claim |
| --- | --- | --- |
| `functions.http(name, handler)` public path | `supported` | Comes from `targets.json` binding metadata. |
| Firebase `onRequest` public path | `supported` | `/<exportName>`. |
| Firebase `onCall` public path | `supported` | `/<exportName>`. |
| Callable JSON envelope | `supported` | Covered. |
| Callable default CORS behavior | `supported` | Covered. |
| `HttpsError` / `FunctionsErrorCode` mapping | `supported` | Covered. |
| Callable auth context with application auth enabled | `supported with caveats` | The runtime path supports it; current coverage centers on the default unauthenticated baseline. |
| App Check verification | `deferred` | Explicit fail-fast boundary. |

## Known non-goals and gaps

- Full standalone Functions Framework local web-server parity.
- Firebase Emulator Suite control-plane parity.
- Non-default Firestore database routing.
- Background triggers outside Firestore document events.
- Auth-context Firestore trigger variants.
- Broad `firebase-admin` parity beyond the documented slice.
- Broader `GlobalOptions`, `DocumentOptions`, `HttpsOptions`, or
  `CallableOptions` matrices beyond the covered base overloads.
