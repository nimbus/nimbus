# Story: Native `nimbus/` source roots with `convex/` compatibility

## Summary

As a Nimbus user, I want to place my app source in a first-party `nimbus/`
directory without losing support for upstream-style `convex/` projects, so
that Nimbus-native apps look like Nimbus apps while Convex-compatible apps
still work unchanged.

This story is the first user-facing step in that direction:

- `nimbus/` becomes the preferred native source root
- `convex/` remains a supported compatibility source root
- internal runtime artifacts stay in `.nimbus/convex/` for now

The current repo already ships both a Nimbus-native JS surface
(`nimbus/server`, `nimbus/browser`, `nimbus/react`, `nimbus/values`) and the
Convex compatibility surface (`convex/*`). What is missing is a consistent
source-root contract and generated-code behavior that matches the root the user
chose.

## Product intent

We want two supported authoring modes:

| Mode | Source root | Package surface | Purpose |
| --- | --- | --- | --- |
| Native Nimbus | `{appDir}/nimbus/` | `nimbus/*` | first-party Nimbus apps and future Nimbus-only features |
| Convex compatibility | `{appDir}/convex/` | `convex/*` | upstream-style apps and migration-friendly compatibility |

This story should make those modes explicit instead of only teaching codegen to
look in one more directory.

## Why this story matters

Today the codegen pipeline hardcodes `convex/` as the only source root, which
makes Nimbus-native apps look like compatibility apps even though the repo
already exposes a separate `nimbus/*` package namespace.

That creates three problems:

1. New Nimbus projects cannot use a first-party `nimbus/` folder convention.
2. Generated `_generated/*` files inherit Convex branding even for Nimbus
   source trees.
3. The public contract is muddy: the repo appears to support a Nimbus-native
   JS surface, but the source-root and generated-file flow still imply that
   `convex/` is the only real authoring mode.

## Current behavior

| Concern | Current behavior |
| --- | --- |
| Source root discovery | hardcoded to `{appDir}/convex/` |
| Generated files | emitted into `{appDir}/convex/_generated/` |
| Internal runtime artifacts | emitted into `{appDir}/.nimbus/convex/` |
| Generated package imports | hardcoded to `convex/browser`, `convex/server`, and `convex/values` |
| CLI wording | still describes `convex/_generated` specifically |

## Desired behavior

### Source root resolution

When resolving the app source root:

1. If both `{appDir}/nimbus/` and `{appDir}/convex/` exist, use `nimbus/` and
   emit a user-facing informational message explaining that both were detected
   and `nimbus/` was selected.
2. Else if `{appDir}/nimbus/` exists, use it.
3. Else if `{appDir}/convex/` exists, use it.
4. Else fail with a clear error naming both supported directories.

If both directories exist, `nimbus/` wins. That keeps the native Nimbus path
authoritative without adding another selector flag in this story, while still
giving users explicit feedback about what happened.

### Generated output behavior

Generated files should follow the selected source root:

| Selected root | Generated directory | Generated package imports |
| --- | --- | --- |
| `{appDir}/nimbus/` | `{appDir}/nimbus/_generated/` | `nimbus/*` |
| `{appDir}/convex/` | `{appDir}/convex/_generated/` | `convex/*` |

### Internal artifact behavior

Internal runtime artifacts stay here for this story:

`{appDir}/.nimbus/convex/`

That location is an implementation detail consumed by the current runtime
registry and Rust server. It is intentionally not renamed in this slice.

## In scope

- Add first-class source-root detection for `nimbus/` and `convex/`
- Emit `_generated/*` under the selected source root
- Make generated imports match the selected package namespace
- Add tests for native Nimbus mode, compatibility mode, both-roots-present
  behavior, and missing-root errors
- Update user-facing help text and docs so they describe the new contract

## Out of scope

- Renaming `.nimbus/convex/` internal artifacts
- Renaming the Rust `--convex-app-dir` flag
- Adding a top-level `nimbus codegen` command
- Adding a manual `--source-dir` selector flag
- Shipping Nimbus-only runtime capabilities beyond the directory and generated
  code contract

## Implementation plan

### 1. Add one source-root resolver in `packages/codegen/src/app.mjs`

Add a helper that resolves the source root once and returns structured data
instead of only a path. The important thing is to stop leaking the name
`convexDir` through code that now supports two modes.

Suggested return shape:

```js
{
  sourceDirName: "nimbus" | "convex",
  sourceDirPath: string,
  packageNamespace: "nimbus" | "convex",
  detectedBothRoots: boolean,
}
```

Suggested behavior:

```js
async function resolveSourceRoot(appDir) {
  const nimbusDir = path.join(appDir, "nimbus");
  const convexDir = path.join(appDir, "convex");
  const nimbusExists = await pathExists(nimbusDir);
  const convexExists = await pathExists(convexDir);

  if (nimbusExists && convexExists) {
    return {
      sourceDirName: "nimbus",
      sourceDirPath: nimbusDir,
      packageNamespace: "nimbus",
      detectedBothRoots: true,
    }
  }

  if (nimbusExists) {
    return {
      sourceDirName: "nimbus",
      sourceDirPath: nimbusDir,
      packageNamespace: "nimbus",
      detectedBothRoots: false,
    }
  }

  if (convexExists) {
    return {
      sourceDirName: "convex",
      sourceDirPath: convexDir,
      packageNamespace: "convex",
      detectedBothRoots: false,
    }
  }

  throw new Error(
    `No nimbus/ or convex/ directory found in ${appDir}. ` +
    `Create one of those directories and place your app functions there.`,
  );
}
```

Export that helper from `app.mjs`.

When both roots are detected, the resolver should report that fact as data and
the CLI layer should own the user-facing message. That keeps programmatic
consumers of `@nimbus/codegen` from getting unexpected console output while
still making the CLI behavior explicit.

### 2. Thread the selected source root through `packages/codegen/src/main.mjs`

Replace the current hardcoded `convexDir` lookup with the structured source
root result.

The codegen flow should then derive:

- `sourceDir` from `sourceRoot.sourceDirPath`
- `generatedDir` from `sourceDir`
- `internalDir` from `{appDir}/.nimbus/convex`

Pass both `sourceDir` and `packageNamespace` through the generation flow rather
than reconstructing them later.

If `detectedBothRoots` is true, the CLI entrypoint should emit a short message
to stderr before generation continues. Suggested wording:

```text
Detected both nimbus/ and convex/ in <appDir>; using nimbus/.
```

This should be informational, not an error, and generation should continue.

### 3. Make generated files respect the selected package namespace

`packages/codegen/src/emit/generated_files.mjs` currently hardcodes
`convex/browser`, `convex/server`, and `convex/values`.

Change the emit helpers so they accept `packageNamespace` and use it for:

- `generateApiFile()`
- `generateServerFile()`
- `generateScheduledFunctionsFile()`
- `generateDataModelFile()`

Examples:

```js
import { makeQueryReference } from "nimbus/browser";
export { query, mutation } from "nimbus/server";
import type { GenericId } from "nimbus/values";
```

This is what makes a `nimbus/` source tree feel genuinely native instead of
just "convex with a different folder name".

### 4. Update codegen selftest helpers

In `packages/codegen/src/selftest/helpers.mjs`:

- let `createAppFixture()` accept `{ sourceDir = "convex" }`
- let `readGeneratedFile()` accept `{ sourceDir = "convex" }`

Keep the default at `"convex"` so existing tests continue to validate
compatibility mode unchanged.

### 5. Add explicit source-root fixtures

Add focused tests in `packages/codegen/src/selftest/core_fixtures.mjs` for:

1. `nimbus/` root only
   - codegen succeeds
   - `_generated/*` lands in `nimbus/_generated/`
   - internal artifacts still land in `.nimbus/convex/`
   - generated files import from `nimbus/*`

2. `convex/` root only
   - existing tests already cover this, so no broad rewrite is needed

3. both roots present
   - `nimbus/` wins
   - the CLI emits a short informational message saying both roots were found
     and `nimbus/` will be used
   - generated files land under `nimbus/_generated/`

4. neither root present
   - codegen fails with the explicit error message

At least one native-mode fixture should author user code with `nimbus/server`
and `nimbus/values` imports so we prove the mode end to end rather than only
testing folder detection.

### 6. Keep Convex compatibility tests targeted

`packages/convex/src/selftest.mjs` should keep using `convex/` fixtures. Those
tests are validating the compatibility surface, not the Nimbus-native mode.

No broad rewrite is needed there, but the story should preserve those tests as
an explicit compatibility guardrail.

### 7. Update user-facing wording

Update text that currently implies `convex/` is the only valid root:

- `packages/convex/src/cli.mjs`
- `docs/operating/cli.md`
- `docs/adapters/convex/compatibility.md`

Recommended wording pattern:

- describe `_generated` files generically unless the text is specifically about
  Convex compatibility mode
- describe `--convex-app-dir` as an app directory whose runtime artifacts are
  generated under `.nimbus/convex/`, while the user source root may be either
  `convex/` or `nimbus/`

## Files to modify

| File | Change |
| --- | --- |
| `packages/codegen/src/app.mjs` | add source-root resolver and export it |
| `packages/codegen/src/main.mjs` | use resolved source root instead of hardcoded `convex/` |
| `packages/codegen/src/emit/generated_files.mjs` | emit `nimbus/*` or `convex/*` imports based on selected mode |
| `packages/codegen/src/selftest/helpers.mjs` | parameterize fixture source root |
| `packages/codegen/src/selftest/core_fixtures.mjs` | add native, both-roots, and missing-root fixtures |
| `packages/convex/src/cli.mjs` | relax help text so it is accurate for both roots |
| `docs/operating/cli.md` | document the dual-root contract |
| `docs/adapters/convex/compatibility.md` | explain `convex/` as compatibility mode, not the only app layout |

## Files likely not to modify

| File | Reason |
| --- | --- |
| `packages/codegen/src/schema.mjs` | already loads from the selected source directory once that directory is threaded through |
| `packages/codegen/src/auth_config.mjs` | same as schema loading |
| `packages/codegen/src/parser.mjs` | same as parser root handling |
| `packages/codegen/src/parser/http_routes.mjs` | same as parser root handling |
| `crates/**/*.rs` | still consume `.nimbus/convex/`, which stays unchanged in this story |
| `packages/convex/src/selftest.mjs` | should remain a compatibility-focused test suite |

## Verification

Run:

```sh
npm run test --workspace @nimbus/codegen
```

and:

```sh
npm run test --workspace convex
```

If docs or wording are updated outside those packages, no extra runtime
verification is required beyond making sure the package selftests still pass.

## Acceptance criteria

1. `convex codegen --app <dir>` succeeds when `<dir>/nimbus/` exists and
   `<dir>/convex/` does not.
2. `convex codegen --app <dir>` succeeds when `<dir>/convex/` exists and
   `<dir>/nimbus/` does not.
3. `nimbus-codegen --app <dir>` succeeds with either supported source root.
4. When both roots exist, `nimbus/` is selected.
5. When both roots exist, the CLI emits a short informational message saying
   both roots were detected and `nimbus/` was selected.
6. When neither root exists, the CLI fails with a clear error naming both
   supported directories.
7. `_generated/*` files land under the selected source root.
8. A `nimbus/` source root produces generated files that import from
   `nimbus/*`.
9. A `convex/` source root produces generated files that import from
   `convex/*`.
10. Internal runtime artifacts continue to land under `.nimbus/convex/`
   regardless of source root.
11. Existing Convex compatibility selftests remain green without rewriting them
    to the Nimbus-native mode.

## Follow-on work, not part of this story

After this lands, separate stories can decide whether to:

- add a first-party `nimbus codegen` command
- support mixed import styles more deliberately during migration
- rename the internal `.nimbus/convex/` artifact namespace
- add Nimbus-only generated helpers or features that do not exist in the
  Convex compatibility surface
