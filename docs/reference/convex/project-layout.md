---
title: Convex project layout
description: Files that nimbus init convex creates, files the toolchain recognizes, and artifacts codegen generates.
sidebar:
  order: 1
---

The directory layout of a Convex-style Nimbus project: what `nimbus init
convex` scaffolds, what the toolchain recognizes, and what codegen writes.

## Directory tree

A project after its first `nimbus dev` run:

```text
my-app/
├── convex/
│   ├── _generated/          # written by codegen — do not edit
│   │   ├── api.ts
│   │   ├── server.ts
│   │   ├── scheduled_functions.ts
│   │   └── dataModel.d.ts
│   ├── schema.ts            # scaffolded; optional
│   └── messages.ts          # scaffolded example functions
├── .nimbus/                 # local state — gitignored
│   ├── convex/              # codegen build artifacts
│   ├── dev/                 # dev-server data
│   └── packages/            # provisioned npm packages
├── .env.local               # written by nimbus dev — gitignored
├── .gitignore
├── package.json
└── tsconfig.json
```

## Scaffolded files

| File | Contents |
| --- | --- |
| `convex/schema.ts` | A `messages` table with `author` and `body` string fields and a `by_author` index. |
| `convex/messages.ts` | Example `list` query and `send` mutation. |
| `package.json` | Declares `"convex": "file:./.nimbus/packages/convex"`. No codegen dependency — codegen is embedded in the Nimbus binary. |
| `tsconfig.json` | `target`/`module` `esnext`, `moduleResolution` `bundler`, `strict`, `noEmit`, `include: ["convex"]`. |
| `.gitignore` | Ignores `.nimbus/`, `.env.local`, and `node_modules/`. |

## Recognized files

| File | Role |
| --- | --- |
| `convex/schema.ts` | Table definitions. Optional: with no schema file the project compiles with zero tables, and tables accept documents of any shape. Must `export default defineSchema(...)`. |
| `convex/*.ts` | Function modules. Exports registered with `query`, `mutation`, `action`, `httpAction`, or their `internal` variants become callable functions. |
| `convex/http.ts` | The only file HTTP routes are read from. Must `export default` a router initialized with `httpRouter()`. |
| `convex/auth.config.ts` or `.js` | Auth provider list. Exactly one of the two extensions may exist. `process.env` reads resolve at codegen time. |
| `convex.json` | Project configuration (see below). |

## `convex.json`

| Key | Values |
| --- | --- |
| `node.nodeVersion` | `"20"`, `"22"`, `"24"`, or `"26"`. Default `"24"`. Applies to `"use node"` modules. |
| `node.externalPackages` | Array of package names left external to the bundle, or `["*"]` (which must appear alone) for all packages. |

## Generated files

`convex/_generated/` is rewritten by codegen on every run:

| File | Contents |
| --- | --- |
| `api.ts` | The `api` and `internal` function-reference trees. |
| `server.ts` | Re-exports the function registrars (`query`, `mutation`, `action`, `httpAction`, `internalQuery`, `internalMutation`, `internalAction`, `paginatedQuery`, `internalPaginatedQuery`, plus `defineSchema`, `defineTable`, `httpRouter`) and the `QueryCtx`, `MutationCtx`, and `ActionCtx` types. `paginatedQuery` and `internalPaginatedQuery` are a Nimbus extension — not in upstream Convex. |
| `scheduled_functions.ts` | References for schedulable (mutation) functions. |
| `dataModel.d.ts` | `Doc`, `Id`, `TableNames`, and `DataModel` types derived from the schema, including the `_id`, `_creationTime`, and `_updateTime` system fields. `_updateTime` is a Nimbus extension — not in upstream Convex. |

`.nimbus/convex/` holds the build artifacts the server executes:

| File | Contents |
| --- | --- |
| `functions.json` | Function manifest. |
| `schema.json` | Compiled schema. |
| `http_routes.json` | Routes parsed from `convex/http.ts`. |
| `auth.config.json` | Compiled auth providers. |
| `node_external_packages.json` | Resolved external-package list. |
| `bundle.mjs` | The executable function bundle. |
| `bundle.sha256` | Bundle digest, verified before every invocation. |

## Local state

| Path | Contents |
| --- | --- |
| `.nimbus/dev/` | Dev-server data directory (override with `nimbus dev --data-dir`). |
| `.nimbus/packages/` | The Nimbus-provisioned `convex` npm package targeted by the `file:` dependency. |
| `.env.local` | Written by `nimbus dev`: `NIMBUS_DEPLOYMENT=local:<slug>` identifying the local deployment. |

## Environment variables

| Variable | Meaning |
| --- | --- |
| `NIMBUS_DEPLOYMENT` | Local deployment marker written to `.env.local` by `nimbus dev`. |
| `NIMBUS_CODEGEN_RUNNER` | Set to `external-node` to run codegen through an external Node.js toolchain instead of the in-binary default (diagnostic use). |
