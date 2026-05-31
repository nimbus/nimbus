# @nimbus/codegen

The code generation engine for [Nimbus](../../README.md) apps. It reads your
app's function source root (`nimbus/` or `convex/`), and produces:

- the `_generated/*` files (`api`, `dataModel`, `server`, `scheduled`) that the
  [`nimbus`](../nimbus/README.md) / [`convex`](../convex/README.md) clients
  import for typed function references, and
- the verified runtime artifacts under `.nimbus/convex/`
  (`functions.json`, `bundle.mjs`, `bundle.sha256`) that the Nimbus V8 runtime
  loads and SHA-256 checks before every invocation.

Codegen does **not** execute your source in the Node process. Schema, server
definitions, and resolver planning run through a restricted TypeScript AST
interpreter with an explicit supported subset, which rejects unsafe globals and
prototype-constructor access. See the
[Convex codegen security boundary](../../docs/adapters/convex/compatibility.md#codegen-security-boundary)
for details.

## CLI

```bash
nimbus-codegen --app <dir>   # generate _generated/* and the runtime bundle
```

`<dir>` is the app root. The source root is resolved automatically: `nimbus/`
(native) when present, otherwise `convex/` (compatibility). This is the same
engine invoked by `convex codegen` in the [`convex`](../convex/README.md)
package.

## Programmatic API

```js
import { runCliFromArgs } from "@nimbus/codegen";

await runCliFromArgs(["--app", "./my-app"], {
  onInfo: (message) => console.error(message),
});
```

| Import | What it is |
| --- | --- |
| `@nimbus/codegen` | The library entry (`runCliFromArgs` and generation helpers) |
| `@nimbus/codegen/cli` | The executable CLI module (also exposed as the `nimbus-codegen` bin) |

## What it emits

| Output | Location | Purpose |
| --- | --- | --- |
| `_generated/api`, `_generated/server`, `_generated/dataModel`, `_generated/scheduled` | under the source root | Typed references the client SDKs import |
| `functions.json` | `.nimbus/convex/` | Function manifest with per-function runtime metadata |
| `bundle.mjs` | `.nimbus/convex/` | Runtime handler bundle materialized by the V8 runtime |
| `bundle.sha256` | `.nimbus/convex/` | Integrity hash verified before every invocation |

## Scripts

```bash
npm run test --workspace @nimbus/codegen       # selftest suite
npm run typecheck --workspace @nimbus/codegen   # type-only selftest pass
```

## Related

- [`nimbus`](../nimbus/README.md) / [`convex`](../convex/README.md) — consume the generated files
- [Convex compatibility reference](../../docs/adapters/convex/compatibility.md)
