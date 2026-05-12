# Cloud Functions Migration Guide

This guide is the practical migration path for teams moving Firebase Cloud
Functions or standalone Functions Framework handlers onto Nimbus today.

For the exact support matrix, see
[Cloud Functions compatibility](cloud-functions-compatibility.md).

## Who This Covers

Nimbus currently supports two migration audiences:

1. Firebase v2 authors using `firebase-functions/v2/*`
2. Standalone authors using `@google-cloud/functions-framework`

The goal is unchanged source imports for the covered slices, not a second
Nimbus-specific handler API.

## Recommended Migration Path

1. Keep your existing handler modules and imports for the covered surface.
2. Run Nimbus from the existing Firebase repo root or standalone package root.
3. Let Nimbus auto-detect the app root in the common case.
4. Use `--app-dir` only when the repo layout is ambiguous or nonstandard.
5. Generate and validate artifacts with `nimbus codegen`.
6. Run locally with `nimbus start`.
7. Deploy with `nimbus deploy` once the local path is verified.

## Project Layouts That Work Today

### Firebase project

Nimbus preserves the conventional Firebase layout:

```text
my-app/
  firebase.json
  functions/
    package.json
    src/
      index.ts
```

`firebase.json` `functions.source` and multi-codebase layouts are preserved.
Generated outputs land under:

```text
.nimbus/firebase/
  artifact.json
  targets.json
  bundle.mjs
  bundle.sha256
```

### Standalone Functions Framework package

Nimbus also supports the common standalone package-root shape:

```text
my-functions/
  package.json
  src/
    index.ts
  .nimbus/
    firebase/
      targets.json
```

For standalone packages, `targets.json` is required because
`functions.cloudEvent()` and `functions.http()` name targets but do not carry
their Firestore or HTTP binding metadata in source.

## Commands

Local codegen:

```bash
nimbus codegen
```

Local server:

```bash
nimbus start
```

Explicit override when needed:

```bash
nimbus start --app-dir ./packages/functions
```

Deploy:

```bash
nimbus deploy --url http://127.0.0.1:8080 --token <deploy-token>
```

## Covered Source-Compatible Imports

### Firebase v2

These covered imports can stay unchanged:

- `firebase-functions/v2`
- `firebase-functions/v2/firestore`
- `firebase-functions/v2/https`
- covered `firebase-admin/app`
- covered `firebase-admin/firestore`

### Standalone Functions Framework

These covered imports can stay unchanged:

- `@google-cloud/functions-framework`
- covered `firebase-admin/app`
- covered `firebase-admin/firestore`

## Authoring Surface Summary

| Surface | Migration posture |
| --- | --- |
| `onDocumentCreated`, `onDocumentUpdated`, `onDocumentDeleted`, `onDocumentWritten` | Keep imports and handler bodies for the covered first slice. |
| `functions.cloudEvent(name, handler)` | Keep source, but ensure `.nimbus/firebase/targets.json` binds the named target. |
| `functions.http(name, handler)` | Keep source, but ensure `.nimbus/firebase/targets.json` binds the named target and path. |
| `onRequest(handler)` / `onRequest({}, handler)` | Keep source for the covered base overloads. |
| `onCall(handler)` / `onCall({}, handler)` | Keep source for the covered base overloads. |

## HTTP Path Rules

Nimbus keeps path derivation explicit:

| Surface | Path rule |
| --- | --- |
| `functions.http(name, handler)` | Comes from `targets.json` |
| Firebase `onRequest` | `/<exportName>` |
| Firebase `onCall` | `/<exportName>` |

For example:

```ts
export const hello = onRequest(async (req, res) => {
  res.json({ ok: true });
});
```

is served at:

```text
/hello
```

## Trigger Delivery Behavior

Cloud Functions-compatible Firestore triggers on Nimbus use:

- durable at-least-once delivery
- journal-backed crash/restart replay
- bounded retry replay for retryable failures
- service-principal execution
- chain-depth limiting for recursive write-back triggers

Practical implications:

- handler code should be idempotent
- follow-up writes may be retried
- recursive trigger chains are intentionally bounded

## Covered `firebase-admin` Usage

The first covered admin slice is enough for the current migration fixtures and
common handler bodies:

- `initializeApp()`
- `getApp()`, `getApps()`, `deleteApp()`
- `getFirestore()`
- `collection(path)`, `doc(path)`
- `get()`, `set()`, `update()`, `delete()`
- `DocumentSnapshot.data()`, `get(fieldPath)`
- covered `Timestamp` helpers

Treat broader Admin SDK usage as explicit follow-on work rather than assuming
parity.

## Option Boundaries

Nimbus is intentionally strict about unsupported options.

### Covered today

- Firestore document triggers with the documented first-slice option subset
- `setGlobalOptions({ retry })` for Firestore document triggers
- base `onRequest(handler)` and `onRequest({}, handler)` overloads
- base `onCall(handler)` and `onCall({}, handler)` overloads

### Fail-fast today

- unsupported `DocumentOptions` fields
- unsupported `HttpsOptions` fields
- unsupported `CallableOptions` fields
- root `onInit()`
- broader `setGlobalOptions()` fields such as `region`, `memory`,
  `serviceAccount`, `enforceAppCheck`, and related deployment/runtime knobs

## Callable Notes

The covered callable slice includes:

- Firebase callable JSON envelope
- default CORS behavior
- `HttpsError` / `FunctionsErrorCode` mapping
- unauthenticated baseline behavior

Still deferred:

- App Check verification
- broader callable option coverage
- broader Firebase platform parity beyond the documented request/response path

## When To Use `--app-dir`

You should not need `--app-dir` for the common case.

Use it when:

- a monorepo contains multiple compatible Firebase or Functions Framework roots
- you are intentionally targeting a nested package instead of the nearest root
- the working directory is not inside the desired app tree

## Known Non-Goals

Do not currently assume:

- full standalone Functions Framework server parity
- Firebase Emulator Suite control-plane parity
- named Firestore databases
- broad `firebase-admin` parity
- non-Firestore trigger families
- App Check verification

## Suggested Adoption Order

For most teams:

1. Confirm your app root auto-detects with `nimbus codegen`.
2. Add `targets.json` for standalone Functions Framework targets if needed.
3. Run `nimbus start` locally and verify trigger/HTTP flows.
4. Confirm any `firebase-admin` usage stays inside the documented subset.
5. Make handler writes idempotent with at-least-once delivery in mind.
6. Deploy with `nimbus deploy`.

## See Also

- [Cloud Functions compatibility](cloud-functions-compatibility.md)
- [Cloud Functions artifact contract](cloud-functions-artifact-contract.md)
- [Cloud Functions target binding contract](cloud-functions-target-binding-contract.md)
- [Cloud Functions root defaults contract](cloud-functions-root-defaults-contract.md)
- [Cloud Functions app-root and admin contract](cloud-functions-app-root-and-admin-contract.md)
