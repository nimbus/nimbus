# Cloud Functions Artifact Contract

This document records the `T0.4` deploy/runtime decision for Cloud
Functions-compatible authoring on Neovex.

## Decision

Neovex will use a **sibling Cloud Functions artifact family** under
`.neovex/firebase/` rather than trying to make the current Convex manifest and
registry schema generic up front.

The shared parts stay shared:

- authenticated deploy-admin staging
- dry-run validation and diffing
- runtime bundle integrity checks
- atomic generation activation
- the existing runtime executor and runtime-service registry seams

The artifact schema stays separate:

- Convex keeps `.neovex/convex/` and `ConvexRegistry::from_app_dir(...)`
- Cloud Functions gets `.neovex/firebase/` with its own manifest family and
  target discovery contract

This is intentional. The current Convex manifest is built around Convex
function plans and HTTP route manifests. Cloud Functions needs different
metadata: handler targets, event bindings, import-resolution metadata, and
framework/Firebase compatibility shims. Forcing those into the existing
Convex manifest now would create a fake "generic" abstraction with low reuse
and higher migration risk.

## Internal Layout

The first-slice internal Cloud Functions artifact root is:

```text
.neovex/firebase/
  artifact.json
  bundle.mjs
  bundle.sha256
  targets.json
```

`artifact.json` is the stable family envelope. `targets.json` is the
deploy-time target and binding manifest defined in
[cloud-functions-target-binding-contract.md](/Users/jack/src/github.com/agentstation/neovex/docs/reference/cloud-functions-target-binding-contract.md).

`bundle.mjs` and `bundle.sha256` follow the same integrity rule as the current
Convex runtime bundle path: activation must validate the SHA-256 sidecar
before a generation becomes live.

## Manifest Envelope

The first-slice `artifact.json` shape is:

```json
{
  "version": 1,
  "family": "cloud_functions",
  "runtime_bundle": {
    "entry_file": "bundle.mjs",
    "sha256_file": "bundle.sha256"
  },
  "targets_manifest": "targets.json",
  "import_resolution": {
    "strategy": "deploy_alias_layer",
    "covered_specifiers": [
      "@google-cloud/functions-framework",
      "firebase-admin/app",
      "firebase-admin/firestore",
      "firebase-functions/v2",
      "firebase-functions/v2/firestore",
      "firebase-functions/v2/https"
    ]
  }
}
```

This manifest does **not** inline event bindings. That data stays in the
neighbor `targets.json` contract.

## Import Resolution

The chosen import strategy is a **Neovex-owned deploy/build alias layer**.

That means:

- user source keeps upstream imports unchanged
- Neovex resolves covered imports during build/deploy to compatibility shims
- Neovex does **not** require user source rewrites
- Neovex does **not** rely on replacing upstream packages in the user's package
  manager graph just to make covered imports work

Covered first-slice specifiers:

- `firebase-functions/v2`
- `firebase-functions/v2/firestore`
- `firebase-functions/v2/https`
- `@google-cloud/functions-framework`
- `firebase-admin/app`
- `firebase-admin/firestore`

Additional upstream specifiers stay out of scope until a later plan item
promotes them explicitly.

## Boundary

`T0.4` only fixes the artifact family, runtime bundle contract, and exact
import-resolution strategy.

It does **not** yet settle:

- Firebase `setGlobalOptions()` inheritance details
- app-root discovery and `firebase.json` codebase handling
- the covered `firebase-admin` method matrix beyond the import surface
- HTTP/Callable target binding details

Those are covered by `T0.5`, `T0.6`, and `T0.8`.
