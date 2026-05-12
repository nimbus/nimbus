# Cloud Functions Target Binding Contract

This document records the `T0.5` deploy-time binding choice for Cloud
Functions-compatible authoring on Nimbus.

## Decision

Nimbus will use a typed `targets.json` manifest beside the Cloud Functions
runtime bundle. Each target entry records:

- the source authoring surface (`firebase_v2` or `functions_framework`)
- the deploy-time target name
- the runtime handler entrypoint
- the advertised signature type (`cloudevent` or `http`)
- the binding kind (`firestore_document` or `https`)
- the execution identity contract (`service` or `request`)

This keeps the target contract explicit instead of inferring deploy bindings
from source code after build time.

For Firebase `firebase-functions/v2/https` handlers, the first covered HTTP
slice derives the public `path` from the exported function name as
`/<exportName>` for both `onRequest()` and `onCall()`. Standalone Functions
Framework HTTP targets continue to use explicit binding entries in
`targets.json`.

## Manifest Shape

The first-slice `targets.json` shape is:

```json
{
  "version": 1,
  "targets": [
    {
      "name": "syncUser",
      "entrypoint": "exports.syncUser",
      "authoring_surface": "firebase_v2",
      "signature_type": "cloudevent",
      "binding": {
        "binding_kind": "firestore_document",
        "event_type": "google.cloud.firestore.document.v1.written",
        "database": "(default)",
        "document": "users/{userId}",
        "execution": "service"
      }
    },
    {
      "name": "helloWorld",
      "entrypoint": "registry.helloWorld",
      "authoring_surface": "functions_framework",
      "signature_type": "http",
      "binding": {
        "binding_kind": "https",
        "exposure": "http",
        "path": "/hello",
        "execution": "request"
      }
    },
    {
      "name": "callHello",
      "entrypoint": "exports.callHello",
      "authoring_surface": "firebase_v2",
      "signature_type": "http",
      "binding": {
        "binding_kind": "https",
        "exposure": "callable",
        "path": "/callHello",
        "execution": "request"
      }
    }
  ]
}
```

## Validation Rules

The deploy contract validates these rules before a generation can activate:

- target names must be unique within one manifest
- target names and runtime entrypoints must be non-empty
- Firestore document bindings must use `signature_type: "cloudevent"`
- HTTP bindings must use `signature_type: "http"`
- Firestore document patterns must parse as shared
  `DocumentTriggerPattern` values and therefore end on a document segment
- Firestore document bindings must use `execution: "service"`
- HTTP bindings must use `execution: "request"`
- HTTP paths must begin with `/`
- Firebase HTTPS bindings may use `exposure: "http"` or `exposure: "callable"`

## First-Slice Explicit Rejections

The manifest also rejects unsupported claims up front instead of silently
pretending to support them:

- Functions Framework legacy `event` signatures are out of scope for the
  first slice. Only `http` and `cloudevent` are accepted.
- Firestore `namespace` bindings are out of scope for the first slice. The
  trigger contract only covers the default namespace.
- Raw standalone Functions Framework server parity is still deferred. This
  manifest binds Nimbus-hosted execution targets; it does not claim support
  for `FUNCTION_TARGET`, `FUNCTION_SIGNATURE_TYPE`, or generic ingress
  unmarshalling outside the Nimbus deploy/runtime path.

## Boundary

`T0.5` only fixes the binding metadata and validation contract.

It does **not** yet settle:

- `setGlobalOptions()` inheritance or per-target `GlobalOptions`
- the broader `DocumentOptions`, `HttpsOptions`, or `CallableOptions`
  matrices beyond the covered empty-options/base-handler HTTP and callable
  overloads
- the covered `firebase-admin` method subset
- route collision policy or HTTP/callable runtime behavior
- app-root discovery and `firebase.json` codebase handling

Those remain owned by `T0.6`, `T0.8`, and `T3`.
