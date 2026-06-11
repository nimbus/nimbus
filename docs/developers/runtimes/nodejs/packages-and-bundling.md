---
title: Packages and bundling
description: Use npm packages in Node action modules with externalPackages and staged local node_modules.
sidebar:
  order: 3
---

Node action modules can use npm packages through a staged package pipeline.
Package support is explicit so the runtime stays deterministic: dependencies
are resolved and staged during codegen, never fetched or installed during
invocation.

The [package reference](/reference/runtimes/packages/) is the support matrix
for packages. It separates positive in-process support from packages that
require service or microVM routing.

## Externalize packages

Use `node.externalPackages` in `convex.json` to tell codegen which package
imports should be treated as external Node packages:

```json
{
  "node": {
    "nodeVersion": "24",
    "externalPackages": ["@aws-sdk/client-s3", "sharp"]
  }
}
```

Convex-style wildcard externalization is also supported for packages
imported by `"use node"` action modules. `"*"` must appear by itself:

```json
{
  "node": {
    "externalPackages": ["*"]
  }
}
```

## Packages must resolve locally

Externalized packages must resolve from your local `node_modules` during
codegen. Nimbus stages the resolved package roots under
`.nimbus/convex/node_modules/` and records package metadata in
`.nimbus/convex/node_external_packages.json`.

Runtime invocation does not:

- run `npm install`
- fetch packages from the network
- discover packages outside the generated bundle metadata
- silently bundle unresolved package imports

If a module externalizes a package that is not resolvable from local
`node_modules`, codegen fails with a diagnostic naming the importing module
and the missing package.

## Current limits

Nimbus records Convex cloud-style package size references (45 MiB zipped,
240 MiB unzipped) in the package metadata, but does not yet enforce those
thresholds as deployment limits. Unsupported or unresolved package imports
fail with precise diagnostics rather than ambiguous runtime errors.

Native addons, package-owned binaries, child-process tooling, raw server
listeners, and persistent host filesystem assumptions are not in-process
support claims. They require a service or microVM route unless the
[package reference](/reference/runtimes/packages/) says otherwise for a
specific package.

## What is verified

Package support is canary-backed: real packages are exercised against the
supported Node versions, covering application networking packages, common
HTTP and SaaS SDKs, real Convex-compatible `"use node"` actions, and
host-heavy diagnostic boundaries. See the
[package reference](/reference/runtimes/packages/) for the current matrix
and the [compatibility contract](/reference/runtimes/node-compat/) for what
the support statuses mean.
