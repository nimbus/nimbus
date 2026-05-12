# Cloud Functions App-Root And Admin Contract

This document records the `T0.8` contract for Cloud Functions-compatible app
discovery and the first covered `firebase-admin` method matrix.

## Decision

Nimbus will settle one shared app-root resolver contract before deploy and
runtime phases wire Cloud Functions layouts into the CLI or server.

The first-slice rules are:

- `--app-dir` stays as an explicit override.
- Auto-discovery walks the current directory and its parents.
- Firebase project roots are discovered from `firebase.json`.
- Standalone Cloud Functions package roots are discovered from `package.json`
  plus an `@google-cloud/functions-framework` dependency.
- Firebase project roots preserve `functions.source`, default `functions/`,
  and multi-codebase `functions[].codebase` layouts.
- Generated Cloud Functions artifacts remain under `.nimbus/firebase/`.
- The covered `firebase-admin` surface is explicit and fail-fast.

This is a contract decision, not the full live CLI rollout yet. Later phases
reuse this contract instead of duplicating different heuristics in deploy,
runtime, and package-surface code.

## App-Root Discovery

### Explicit Override

`--app-dir` is the operator escape hatch for ambiguous or nonstandard repos.

When supplied, Nimbus resolves the given path first and chooses the nearest
compatible app root around that explicit location:

- a Firebase project root if the explicit path is that root or a child of it
- a standalone Functions Framework package if the explicit path is that package
  root or a child of it

If the explicit path does not resolve to either shape, Nimbus must fail
clearly instead of silently falling back somewhere else.

### Auto-Discovery

Auto-discovery walks ancestor directories from the current working directory.

Compatible roots are:

- a Firebase project root with `firebase.json`
- a standalone package root with `package.json` and an
  `@google-cloud/functions-framework` dependency

Selection rules:

1. If the working directory is inside a Firebase codebase source directory from
   `firebase.json`, the Firebase project root wins even if there is a nested
   Functions Framework package under that source tree.
2. If the same directory qualifies as both a Firebase project root and a
   standalone framework package, the Firebase project root wins.
3. If Nimbus sees both a Firebase project root and a nested standalone
   framework package that is **not** covered by the Firebase codebase mapping,
   auto-discovery is ambiguous and must fail with guidance to use `--app-dir`.

That keeps the common Firebase migration path automatic without guessing
through mixed monorepos.

## Firebase Layout Coverage

The first slice preserves these `firebase.json` shapes:

### Default source

```json
{}
```

This resolves to one default codebase rooted at `functions/`.

### Single source string

```json
{
  "functions": "functions"
}
```

This resolves to one default codebase named `default`.

### Single object

```json
{
  "functions": {
    "source": "packages/functions",
    "codebase": "default"
  }
}
```

### Multi-codebase array

```json
{
  "functions": [
    { "source": "packages/app-functions", "codebase": "app" },
    { "source": "packages/admin-functions", "codebase": "admin" }
  ]
}
```

Validation rules:

- codebase names must be unique
- source paths must be non-empty directories
- source paths must stay inside the Firebase project root

## Standalone Package Coverage

The first standalone Functions Framework slice recognizes a package root when:

- `package.json` exists
- one of `dependencies`, `devDependencies`, `optionalDependencies`, or
  `peerDependencies` includes `@google-cloud/functions-framework`
- `package.json.main` resolves to a file, or one of the default entrypoints
  exists:
  - `index.js`
  - `index.mjs`
  - `index.cjs`
  - `index.ts`
  - `index.mts`
  - `index.cts`

This is intentionally narrow and source-compatible for the common package-root
patterns. It is not a claim of full standalone runtime parity yet.

## Internal Artifact Ownership

Resolved Cloud Functions app roots always pair with the Cloud Functions sibling
artifact namespace:

```text
.nimbus/firebase/
```

That internal directory is owned by Nimbus and stays separate from the existing
Convex-compatible artifact root:

```text
.nimbus/convex/
```

## Covered `firebase-admin` Matrix

The first covered admin-import subset is:

| Import | Covered methods |
| --- | --- |
| `firebase-admin/app` | `initializeApp()`, `getApp()`, `getApps()`, `deleteApp()` |
| `firebase-admin/firestore` | `getFirestore()`, `Firestore.doc(path)`, `Firestore.collection(path)`, `CollectionReference.doc(path)`, `DocumentReference.get()`, `DocumentReference.set(data)`, `DocumentReference.update(data)`, `DocumentReference.delete()`, `DocumentReference.collection(path)`, `DocumentSnapshot.data()`, `DocumentSnapshot.get(fieldPath)`, `Timestamp.fromMillis()`, `Timestamp.now()`, `Timestamp.toMillis()`, `Timestamp.toDate()`, `Timestamp.isEqual()`, `Timestamp.toJSON()` |

Everything else is out of scope until a later plan item promotes it.

The covered Firestore subset is intentionally narrow:

- document paths must be explicit document-shaped paths
- `CollectionReference.doc()` without an explicit path is still unsupported
- `set(data, options)`, `update(field, value, ...)`, and `delete(options)` are still unsupported
- collection queries, transactions, listeners, and batch helpers are still deferred on the admin surface
- covered document reads and writes route through the shared Firebase document-path and bound-write primitives instead of a Cloud-Functions-local storage shim

Unsupported imports or methods must fail validation explicitly. Nimbus should
not quietly accept `firebase-admin/auth`, `firebase-admin/storage`, or deeper
Firestore helper methods that are not yet documented here.

## Boundary

`T0.8` settles the discovery and covered-admin contract only.

It does **not** yet wire this resolver into:

- `nimbus dev`
- `nimbus deploy`
- runtime artifact loading
- package-surface aliasing for the covered admin methods

Those later phases must reuse this contract instead of re-deciding the layout
and server-SDK boundary inline.
