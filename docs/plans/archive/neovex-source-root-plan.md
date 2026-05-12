# Plan: Native `nimbus/` Source Roots with `convex/` Compatibility

Canonical execution plan for making `nimbus/` the first-party JS app source
root while preserving upstream-style `convex/` projects as a supported
compatibility mode. This plan owns source-root selection, generated-code
namespace alignment, CLI feedback when both roots are present, and the docs
and test contract around that behavior.

Reviewed against:

- `docs/stories/support-nimbus-source-directory.md` — user-facing contract and
  acceptance target for this workstream
- `packages/codegen/src/app.mjs` — current app-dir parsing and file collection
- `packages/codegen/src/main.mjs` — hardcoded `appDir/convex` source-root and
  `_generated` emission path
- `packages/codegen/src/emit/generated_files.mjs` — hardcoded `convex/*`
  imports in generated files
- `packages/codegen/src/parser/compile_bindings.mjs` — existing partial
  `nimbus/server` import awareness
- `packages/codegen/src/selftest/helpers.mjs` and
  `packages/codegen/src/selftest/core_fixtures.mjs` — current test helpers and
  fixture coverage
- `packages/convex/src/cli.mjs` — current help text that still names
  `convex/_generated`
- `docs/reference/cli.md` and `docs/convex/compatibility.md` — current public
  wording for app-dir and generated-artifact behavior
- `crates/nimbus-server/src/adapters/convex/registry/loading.rs` and
  `crates/nimbus-bin/src/serve/mod.rs` — current `.nimbus/convex/` runtime
  artifact contract and `--convex-app-dir` surface

---

## Status

- **Status:** `done`
- **Primary owner:** this plan
- **Activation gate:** none; promoted from the source-root story on
  `2026-04-22`
- **Related plans and docs:**
  - `docs/stories/support-nimbus-source-directory.md` — product story this
    plan executes
  - `docs/convex/compatibility.md` — compatibility-surface reference that must
    stay accurate as the native Nimbus root lands
  - `docs/reference/cli.md` — user-facing CLI contract for `--convex-app-dir`

## Current Assessed State

- The repo already ships both `packages/nimbus` and `packages/convex`, so the
  package-level namespace split exists today even though the source-root
  contract does not.
- `packages/codegen/src/main.mjs` still hardcodes `{appDir}/convex/` as the
  only source root, emits `_generated/*` into `convex/_generated/`, and always
  writes internal runtime artifacts under `.nimbus/convex/`.
- `packages/codegen/src/emit/generated_files.mjs` still hardcodes
  `convex/browser`, `convex/server`, and `convex/values` in every generated
  file, so a future `nimbus/` root would still produce Convex-branded outputs
  unless the generator becomes namespace-aware.
- `packages/codegen/src/parser/compile_bindings.mjs` already recognizes
  `nimbus/server` for imported pagination validators, which shows the parser
  has begun to accommodate the native namespace even though the broader root
  and generation flow has not.
- `packages/codegen` selftest helpers still create only `convex/` fixtures and
  read only `convex/_generated/*`, so there is no current test coverage for a
  native Nimbus root, both-roots-present selection, or missing-root errors.
- `packages/convex/src/cli.mjs`, `docs/reference/cli.md`, and
  `docs/convex/compatibility.md` still read as if `convex/` is the only app
  layout, even though the repo now aims to support a first-party `nimbus/`
  source root.
- The Rust runtime boundary remains intentionally Convex-namespaced internally:
  the server registry loads manifests from `.nimbus/convex/`, and the `serve`
  CLI still exposes `--convex-app-dir`. That internal artifact contract must
  remain stable throughout this plan.

## Control Plan Rules

1. `nimbus/` is the native authoring mode and wins when both `nimbus/` and
   `convex/` are present.
2. When both roots exist, the CLI must emit an informational message to
   `stderr`, but programmatic `@nimbus/codegen` consumers must not receive
   surprise console output from library internals.
3. Source-root selection, `_generated/*` output location, and generated package
   imports move together as one contract. This plan must not land a mixed mode
   where `nimbus/` emits `convex/*` imports or vice versa.
4. `.nimbus/convex/` remains the only internal runtime artifact namespace in
   this plan. Do not retarget Rust registry loading, `--convex-app-dir`, or
   runtime manifest paths here.
5. Convex compatibility remains first-class. Existing `convex/`-root tests stay
   authoritative compatibility guardrails rather than being rewritten into the
   native Nimbus mode.
6. Prefer a single resolver-owned source-root record passed through the codegen
   flow over repeated path checks or ad hoc `"convex"` string branching.

## Verification Contract

Each roadmap item must satisfy before closing:

- `npm run test --workspace @nimbus/codegen` — green
- `npm run test --workspace convex` — green
- manual verification described per item

## Target Contract

### Authoring modes

| Mode | Source root | Generated imports | Purpose |
| --- | --- | --- | --- |
| Native Nimbus | `{appDir}/nimbus/` | `nimbus/*` | first-party Nimbus apps and future Nimbus-only features |
| Convex compatibility | `{appDir}/convex/` | `convex/*` | upstream-style apps and migration-friendly compatibility |

### Source-root resolution

Source-root selection returns one resolver-owned record:

```js
{
  sourceDirName: "nimbus" | "convex",
  sourceDirPath: string,
  packageNamespace: "nimbus" | "convex",
  detectedBothRoots: boolean,
}
```

Selection rules:

1. if both `nimbus/` and `convex/` exist, choose `nimbus/` and mark
   `detectedBothRoots: true`
2. else if `nimbus/` exists, choose it
3. else if `convex/` exists, choose it
4. else fail with an error that explicitly names both supported directories

### CLI feedback boundary

When `detectedBothRoots` is `true`, the CLI entrypoint emits an informational
message to `stderr` and continues:

```text
Detected both nimbus/ and convex/ in <appDir>; using nimbus/.
```

The resolver reports this as data; it does not print directly.

### Internal artifact boundary

User-facing source roots may vary, but internal runtime artifacts stay fixed:

- source roots: `{appDir}/nimbus/` or `{appDir}/convex/`
- generated files: `{appDir}/{selectedRoot}/_generated/*`
- runtime artifacts: `{appDir}/.nimbus/convex/*`

## Roadmap

### NSR1 — Resolver-owned source-root selection and CLI feedback

Implement a single `resolveSourceRoot()` helper in `packages/codegen/src/app.mjs`
that detects `nimbus/` and `convex/`, returns the structured selection record,
and surfaces `detectedBothRoots` to the codegen entrypoint. Replace the
hardcoded `appDir/convex` lookup in `packages/codegen/src/main.mjs` with this
resolver-owned data, and make the CLI layer own the informational
both-roots-detected message.

**Verification:** (a) `convex codegen --app <dir>` succeeds when only
`nimbus/` exists, (b) succeeds when only `convex/` exists, (c) when both
exist, the CLI emits the informational message to `stderr` and still
completes, (d) when neither exists, the CLI fails with the explicit
dual-directory error.

**Status:** `done`

### NSR2 — Namespace-aware generated file emission

Thread `packageNamespace` through codegen and update
`packages/codegen/src/emit/generated_files.mjs` so generated files import from
`nimbus/*` or `convex/*` based on the selected source root. This item owns
`api.ts`, `server.ts`, `scheduled_functions.ts`, and `dataModel.d.ts`
alignment. The internal runtime artifact directory remains `.nimbus/convex/`.

**Verification:** (a) a `nimbus/` root emits `nimbus/browser`,
`nimbus/server`, and `nimbus/values` imports, (b) a `convex/` root still emits
the current `convex/*` imports, (c) internal artifacts still land under
`.nimbus/convex/`.

**Status:** `done`

### NSR3 — Selftest coverage for native, compatibility, and ambiguity cases

Extend `packages/codegen/src/selftest/helpers.mjs` so fixture helpers can
target either source root without breaking the current `convex/` defaults.
Add focused fixtures in `core_fixtures.mjs` that cover native Nimbus mode,
both-roots-present selection, and missing-root errors. Keep
`packages/convex/src/selftest.mjs` rooted in `convex/` as the compatibility
baseline.

**Verification:** (a) new native-mode fixture proves `_generated/*` lands
under `nimbus/_generated/`, (b) native-mode fixture proves generated files
import from `nimbus/*`, (c) both-roots fixture asserts selection and info
message behavior, (d) missing-root fixture asserts the exact error contract,
(e) existing Convex selftests remain green unchanged.

**Status:** `done`

### NSR4 — Public docs and help-text alignment

Update the user-facing contract so docs and help text describe dual-root
support accurately. This item owns `packages/convex/src/cli.mjs`,
`docs/reference/cli.md`, and `docs/convex/compatibility.md`. The result
should describe `convex/` as compatibility mode and `nimbus/` as the native
mode, while still documenting the stable internal `.nimbus/convex/` artifact
path and `--convex-app-dir` flag.

**Verification:** (a) CLI help no longer claims `convex/_generated` is the
only output location, (b) CLI reference explains that the user source root may
be `nimbus/` or `convex/` while runtime artifacts stay under
`.nimbus/convex/`, (c) compatibility docs explicitly frame `convex/` as the
compatibility path rather than the only supported app layout.

**Status:** `done`

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-22 | Plan authored | — | Promoted the native `nimbus/` source-root story into a canonical active control plane with explicit ownership over resolver behavior, generated imports, tests, and docs/help alignment. |
| 2026-04-22 | NSR1 | `in_progress` | Began resolver-owned source-root selection work. Current focus is `packages/codegen` source-root detection, CLI-owned both-roots feedback, and focused coverage for dual-root and missing-root behavior before namespace-aware generated imports land in `NSR2`. |
| 2026-04-22 | NSR1 | `done` | Added a resolver-owned source-root record in `packages/codegen`, threaded it through codegen startup, moved both-roots feedback to CLI-owned `stderr` callbacks in both entrypoints, and added focused smoke coverage for `nimbus/`-only, dual-root, missing-root, and Convex-CLI native-root paths. Verification: `npm run test --workspace @nimbus/codegen`; `npm run test --workspace convex`. |
| 2026-04-22 | NSR2 | `in_progress` | Starting namespace-aware generated-file emission so the selected source root controls `nimbus/*` versus `convex/*` imports without changing the internal `.nimbus/convex/` runtime artifact contract. |
| 2026-04-22 | NSR2 | `done` | Made generated `api.ts`, `server.ts`, `scheduled_functions.ts`, and `dataModel.d.ts` namespace-aware so `nimbus/` roots emit `nimbus/*` imports and `convex/` roots keep `convex/*` imports. Internal runtime artifacts remain under `.nimbus/convex/`. Verification: `npm run test --workspace @nimbus/codegen`; `npm run test --workspace convex`. |
| 2026-04-22 | NSR3 | `done` | Parameterized codegen selftest helpers for either source root, converted the native smoke into a true `nimbus/server` / `nimbus/values` authoring case, added a native `auth.config.ts` fixture via `nimbus/server`, and kept the Convex CLI smoke green as the compatibility guardrail. Verification: `npm run test --workspace @nimbus/codegen`; `npm run test --workspace convex`. |
| 2026-04-22 | NSR4 | `in_progress` | Aligning help text and public docs with the landed dual-root behavior while preserving the documented `.nimbus/convex/` runtime artifact path and `--convex-app-dir` flag. |
| 2026-04-22 | NSR4 | `done` | Updated the Convex CLI help text, CLI reference, and compatibility docs to describe dual-root support accurately while keeping `.nimbus/convex/` and `--convex-app-dir` explicit. Final verification: `npm run test --workspace @nimbus/codegen`; `npm run test --workspace convex`; `npm run test --workspaces --if-present`; `npm run build --workspaces --if-present`. Workstream complete; plan archived. |
