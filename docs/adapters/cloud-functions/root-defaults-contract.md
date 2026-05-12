# Cloud Functions Root Defaults Contract

This document records the `T0.6` choice for `firebase-functions/v2` root
imports and `setGlobalOptions()` inheritance on Nimbus.

## Decision

The first Cloud Functions slice supports a **narrow, fail-fast** root defaults
contract:

- `firebase-functions/v2` root imports may use `setGlobalOptions()`
- document-trigger handlers inherit only the `retry` field from root defaults
- explicit per-handler options override root defaults
- HTTPS `onRequest()` / `onCall()` handlers inherit no root defaults in the
  covered base HTTP and callable slices
- root-level `onInit()` is explicitly deferred and must fail validation

This keeps the root package source-compatible without pretending that Nimbus
already implements the full Google Cloud deployment and runtime option matrix.

## Inheritance Order

For covered document triggers, Nimbus resolves defaults in this order:

1. explicit per-handler option
2. `setGlobalOptions()` default
3. no value

The only inherited field in the first slice is `retry`.

## Supported First-Slice Root Default Fields

| Surface | Supported inherited fields |
| --- | --- |
| Firestore document triggers | `retry` |
| HTTPS `onRequest()` | none |
| HTTPS `onCall()` | none |

`retry` is the only root default that already maps cleanly onto the shared
durable trigger-delivery contract. Everything else is deferred until later
runtime and HTTP phases own the underlying behavior.

For the covered callable slice, this means:

- `onCall(handler)` and `onCall({}, handler)` are allowed
- explicit `CallableOptions` fields still fail validation
- `enforceAppCheck` remains an explicit fail-fast root/default option rather
  than an implied verification feature

## Explicit Rejections

The first slice rejects these root-level claims up front:

- `onInit()` from `firebase-functions/v2`
- `region`
- `memory`
- `timeoutSeconds`
- `minInstances`
- `maxInstances`
- `concurrency`
- `cpu`
- `serviceAccount`
- `ingressSettings`
- `invoker`
- `labels`
- `secrets`
- `enforceAppCheck`
- `preserveExternalChanges`
- `omit`
- `vpcConnector`
- `vpcEgress`
- `vpcConnectorEgressSettings`
- `networkInterface`

Those are not being ignored. They are simply outside the first-slice Nimbus
contract and must fail validation until a later phase promotes them.

## Boundary

`T0.6` only fixes the root default and root API contract.

It does **not** yet settle:

- the broader `DocumentOptions`, `HttpsOptions`, or `CallableOptions`
  matrices beyond the covered empty-options/base-handler HTTPS and callable
  overloads
- `params`, `config()`, or emulator helper compatibility beyond what later
  package-surface work chooses to cover
- app-root discovery, codebase mapping, or `firebase-admin` coverage

Those remain owned by later `T2` / `T4` compatibility and documentation work.
