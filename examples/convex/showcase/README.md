# Convex showcase

A small Convex app used to exercise the Nimbus developer console's **function
source visibility** (FSV): source view, code navigation, and type-hover.

Modules (`convex/`):

- `messages.ts` — `list` / `send` / `summary`; a locally-typed `MAX_PAGE` const
  for a simple type-hover.
- `users.ts` — `getByEmail` (query) / `touch` (internal mutation).
- `notifications.ts` — `announce` cross-references `internal.users.touch`,
  `api.messages.send`, and `api.messages.list`, so the console shows **CALLS**
  navigation edges.
- `schema.ts` — `messages` + `users` tables.

## Deploy

Start the local server with `nimbus dev`, then deploy to it (omit TARGET for
local; pass a URL or configured target name to deploy elsewhere):

```bash
nimbus deploy --app-dir examples/convex/showcase --convex-silo demo
```

Then open the console (`nimbus auth url`) → Developer → Compute and open a
function's **Source** tab to see:

- syntax-highlighted source from the content-addressed source-package store
  (with the source-package digest shown as provenance),
- a navigable **DEFINES / CALLS** symbols strip (oxc structural index),
- **type-hover** tooltips on identifiers (the TypeScript compiler's inferred
  types, captured at deploy).

> Type-hover needs the project's `typescript` resolvable from the app dir at
> deploy time (here, `node_modules/typescript`). Without it, source + navigation
> still work; type info is simply omitted.
