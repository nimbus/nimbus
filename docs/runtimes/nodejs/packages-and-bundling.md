# Node.js Packages And Bundling

Nimbus supports staged local packages for Node action modules. Package support
is explicit so the runtime can stay deterministic and avoid fetching or
installing dependencies during invocation.

## External Packages

Use `node.externalPackages` in `convex.json` to tell codegen which package
imports should be treated as external Node packages:

```json
{
  "node": {
    "nodeVersion": "22",
    "externalPackages": ["@aws-sdk/client-s3", "sharp"]
  }
}
```

Convex-style wildcard externalization is also supported for packages imported
by `"use node"` action modules:

```json
{
  "node": {
    "externalPackages": ["*"]
  }
}
```

## Local Package Requirement

Externalized packages must resolve from local `node_modules` during codegen.
Nimbus stages the resolved package roots under `.nimbus/convex/node_modules/`
and records package evidence metadata in
`.nimbus/convex/node_external_packages.json`.

Runtime invocation does not:

- run `npm install`
- fetch packages from the network
- discover new packages outside the generated bundle metadata
- silently bundle unresolved package imports

## Current Limits

Nimbus records Convex cloud-style package size references, but it does not yet
enforce the same zipped or unzipped deployment thresholds. Unsupported or
unresolved package imports should fail with precise diagnostics rather than
falling back to ambiguous runtime errors.

## Canary Coverage

Current package canaries cover application networking packages and common
tooling loaders. The checked-in dashboard reports the active canary set and
required lane coverage.

Generated public evidence pages will summarize canary status under
`docs/runtimes/nodejs/evidence/`.
