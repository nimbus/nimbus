---
title: Migrate Cloud Functions
description: Move Firebase Cloud Functions or standalone Functions Framework handlers onto Nimbus with unchanged source imports.
sidebar:
  order: 2
---

Move Firebase Cloud Functions or standalone Functions Framework handlers
onto Nimbus today. The goal is unchanged source imports for the covered
surface — not a second Nimbus-specific handler API.

Two audiences are covered:

1. Firebase v2 authors using `firebase-functions/v2/*`
2. Standalone authors using `@google-cloud/functions-framework`

Keep the
[compatibility matrix](/reference/cloud-functions/compatibility/) open as
the precise support reference.

## 1. Keep your handler modules and imports

These imports stay unchanged for the covered surface:

**Firebase v2:**

- `firebase-functions/v2`
- `firebase-functions/v2/firestore`
- `firebase-functions/v2/https`
- covered `firebase-admin/app`
- covered `firebase-admin/firestore`

**Standalone Functions Framework:**

- `@google-cloud/functions-framework`
- covered `firebase-admin/app`
- covered `firebase-admin/firestore`

## 2. Check your project layout

**Firebase project** — the conventional layout is preserved, including
`firebase.json` `functions.source` and multi-codebase layouts:

```text
my-app/
  firebase.json
  functions/
    package.json
    src/
      index.ts
```

**Standalone Functions Framework package:**

```text
my-functions/
  package.json
  src/
    index.ts
  .nimbus/
    firebase/
      targets.json
```

Standalone packages require a hand-authored
`.nimbus/firebase/targets.json`, because `functions.cloudEvent()` and
`functions.http()` name targets without carrying their Firestore or HTTP
binding metadata in source:

```json
{
  "version": 1,
  "targets": [
    {
      "name": "helloWorld",
      "entrypoint": "registry.helloWorld",
      "authoring_surface": "functions_framework",
      "signature_type": "http",
      "binding": {
        "binding_kind": "https",
        "exposure": "http",
        "path": "/hello",
        "execution": "request_principal"
      }
    },
    {
      "name": "syncUser",
      "entrypoint": "registry.syncUser",
      "authoring_surface": "functions_framework",
      "signature_type": "cloud_event",
      "binding": {
        "binding_kind": "firestore_document",
        "event_type": "google.cloud.firestore.document.v1.written",
        "database": "(default)",
        "document": "users/{userId}",
        "execution": "service_account"
      }
    }
  ]
}
```

Firestore document bindings must use `"execution": "service_account"`.

## 3. Generate artifacts

From the project root:

```bash
nimbus codegen
```

Generated outputs land under `.nimbus/firebase/`: `artifact.json`,
`targets.json`, `bundle.mjs`, and `bundle.sha256`.

## 4. Run locally

For the watched dev loop with app-root auto-detection (`nimbus dev` walks
up from the current directory, bounded by the nearest `.git`):

```bash
nimbus dev
```

For production-shaped startup, pass the app path explicitly — `nimbus
start` does no walk-up. If the artifact contains `onRequest`, `onCall`, or
Functions Framework HTTP targets, bind that deployment to an existing Nimbus
tenant:

```bash
NIMBUS_CLOUD_FUNCTIONS_TENANT=app-production nimbus start --app-dir .
```

In a monorepo where the functions package is not at the root:

```bash
NIMBUS_CLOUD_FUNCTIONS_TENANT=app-production \
  nimbus start --app-dir ./packages/functions
```

The binding is server configuration, not part of the request URL. It remains
fixed when a new Cloud Functions artifact is deployed, so adding more tenants
to the same Nimbus server cannot redirect an HTTP handler. Startup and deploy
both refuse HTTP targets when the binding is missing. `nimbus dev` binds the
detected app to its auto-provisioned development tenant.

## 5. Know the HTTP path rules

| Surface | Path rule |
| --- | --- |
| `functions.http(name, handler)` | Comes from `targets.json` |
| Firebase `onRequest` | `/<exportName>` |
| Firebase `onCall` | `/<exportName>` |

For example, `export const hello = onRequest(...)` is served at `/hello` on
the main server port.

## 6. Write for at-least-once delivery

Firestore triggers on Nimbus use durable at-least-once delivery,
journal-backed crash/restart replay, bounded retry, service-principal
execution, and chain-depth limiting. Practically:

- make handler code idempotent
- expect follow-up writes to be retried
- expect recursive trigger chains to be intentionally bounded

## 7. Stay inside the covered `firebase-admin` slice

The covered admin surface for handler bodies is:

- `initializeApp()`, `getApp()`, `getApps()`, `deleteApp()`
- `getFirestore()`
- `collection(path)`, `doc(path)`
- `get()`, `set()`, `update()`, `delete()`
- `DocumentSnapshot.data()`, `get(fieldPath)`
- covered `Timestamp` helpers

Treat broader Admin SDK usage as unsupported rather than assuming parity —
unsupported operations fail clearly instead of silently stubbing.

## 8. Respect the option boundaries

Covered today:

- Firestore document triggers with the documented first-slice options
- `setGlobalOptions({ retry })` for Firestore document triggers
- base `onRequest(handler)` and `onRequest({}, handler)` overloads
- base `onCall(handler)` and `onCall({}, handler)` overloads

Fail-fast today:

- unsupported `DocumentOptions`, `HttpsOptions`, or `CallableOptions` fields
- root `onInit()`
- broader `setGlobalOptions()` fields such as `region`, `memory`,
  `serviceAccount`, and `enforceAppCheck`

The covered callable slice includes the Firebase callable JSON envelope,
default CORS behavior, `HttpsError` / `FunctionsErrorCode` mapping, and the
unauthenticated baseline. App Check verification is deferred.

## 9. Deploy

```bash
nimbus deploy <server-url> --token <deploy-token>
```

## Suggested order

1. Confirm your app root is detected by running `nimbus codegen`.
2. Add `targets.json` for standalone Functions Framework targets if needed.
3. Run `nimbus start --app-dir .` (or `nimbus dev`) and verify trigger and
   HTTP flows locally.
4. Confirm `firebase-admin` usage stays inside the covered slice.
5. Make handler writes idempotent with at-least-once delivery in mind.
6. Deploy with `nimbus deploy`.
