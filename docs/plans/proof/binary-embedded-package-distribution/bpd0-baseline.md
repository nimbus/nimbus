# BPD0 Baseline — Binary-Embedded Package Distribution

Evidence note for BPD0. Records the current registry-coupled state with
file:line evidence before any BPD implementation lands, and ships the failing
aggregate verifier. All claims verified against the working tree.

Verifier shipped: `scripts/verify-binary-embedded-package-distribution.sh`
(fails on every unimplemented gate; see "Verifier baseline result" below).

## Packages are private; no publish path (gate 3, 4)

- 7/7 `packages/*/package.json` carry `"private": true`
  (`codegen`, `convex`, `dynamodb`, `firebase`, `mongodb`, `nimbus`,
  `nimbus-ui`). Verified: `grep -l '"private": true' packages/*/package.json`
  returns 7 of 7.
- No npm-publish workflow, `publishConfig`, `NODE_AUTH_TOKEN`, or `registry-url`
  anywhere under `.github/workflows/` or the package manifests.

## Defect 1 — `nimbus init` templates declare registry dependencies

- `crates/nimbus-bin/templates/convex/package.json.tmpl:6` —
  `"convex": "^{{CONVEX_VERSION}}"`
- `crates/nimbus-bin/templates/convex/package.json.tmpl:9` —
  `"@nimbus/codegen": "^{{CODEGEN_VERSION}}"`
- `crates/nimbus-bin/templates/cloud-functions/functions/package.json.tmpl:7-8`
  — `firebase-admin@^13.0.0`, `firebase-functions@^6.3.0`
- `crates/nimbus-bin/templates/cloud-functions/functions/package.json.tmpl:11-12`
  — `@nimbus/codegen@^{{CODEGEN_VERSION}}`, `typescript@^5.0.0`
- Versions substitute from `crates/nimbus-bin/build.rs:18-22`
  (`NIMBUS_CONVEX_VERSION`/`NIMBUS_CODEGEN_VERSION`), so production renders
  `^0.1.33` — a registry range for a private, unpublished package.

## Defect 2 — embedded codegen still requires `node_modules/@nimbus/codegen`

- `crates/nimbus-bin/src/codegen.rs:231-238` — `parse_codegen_runner_env`
  defaults to `CodegenRunner::ExternalNode` when the env var is unset.
- `crates/nimbus-bin/src/codegen.rs:240` — `EmbeddedPilot` only via
  `NIMBUS_EXPERIMENTAL_EMBEDDED_CODEGEN` (`1/true/on/embedded/pilot`).
- `crates/nimbus-bin/src/codegen.rs:331` —
  `ensure_embedded_codegen_package_available` still requires an installed
  `node_modules/@nimbus/codegen/package.json` (it changes *where JS executes*,
  not *where the package comes from*).
- `crates/nimbus-bin/src/codegen.rs:345` —
  `ensure_embedded_codegen_layout_supported` rejects Firebase Cloud Functions
  multi-root layouts for the embedded pilot.

## Defect 3 / source-vs-provisioned — convex CLI imports private `@nimbus/codegen`

- `packages/convex/src/cli.mjs:3` —
  `import { runCliFromArgs } from "@nimbus/codegen";`
- The repo's own build depends on this chain via the workspace symlink:
  - `packages/nimbus-ui/package.json:7` —
    `"codegen": "convex codegen --app . && node scripts/generate-routes.mjs"`
  - `crates/nimbus-server/build.rs:14-26` — hard-fails if
    `packages/nimbus-ui/dist/index.html` is missing.
- Implication recorded for BPD1/BPD4: sanitization applies only to the
  *provisioned* payload; the in-repo source `convex`/`@nimbus/codegen` packages
  and the `nimbus-ui` codegen path must stay intact.

## Defect 4 — dependency closure is not offline-safe yet

- Cloud Functions scaffold declares public-registry server SDKs
  `firebase-admin`/`firebase-functions`/`typescript` (above); the entrypoint
  `crates/nimbus-bin/templates/cloud-functions/functions/src/index.ts:1-2`
  imports `firebase-functions/v2/https` and `/firestore`.
- First-party SDK third-party deps:
  - `packages/mongodb/package.json:17` — `mongodb@^6.16.0` declared as a runtime
    dependency, but the shipped surface (`packages/mongodb/src/index.ts`,
    `uri.ts`) only exports `uri()` + `UriOptions` and imports nothing — the
    `mongodb` driver dep is vestigial and must be dropped from the provisioned
    manifest.
  - `packages/dynamodb` — `@aws-sdk/client-dynamodb@^3` is an *optional peer*;
    the shipped surface only exports `clientConfig`/`endpoint` (config helpers),
    importing nothing (`@aws-sdk` appears only in a JSDoc example).
  - `packages/firebase` — runtime deps `@bufbuild/protobuf`,
    `@connectrpc/connect`, `@connectrpc/connect-web` (zero-dep pure ESM; bundle
    cleanly).

## Only `convex` + `nimbus-ui` emit a persistent `dist/`; CI carries only `nimbus-ui`

- Packages with a persistent `dist/` today: `convex` (a browser-only IIFE,
  `packages/convex/build.mjs` bundles only `src/browser.ts`) and `nimbus-ui`.
  `nimbus`, `dynamodb`, `mongodb`, `firebase`, `codegen` build into throwaway
  `mkdtemp` smoke dirs or have no real build.
- `.github/workflows/release.yml:104-105,136,227` — caches/refreshes only
  `packages/nimbus-ui/.nimbus` and `packages/nimbus-ui/dist`.

## Precedents to reuse

- `rust-embed` of JS into the binary: `crates/nimbus-server/build.rs:14` asserts
  `packages/nimbus-ui/dist`, embedded at compile time.
- Version-lock: `crates/nimbus-bin/build.rs:18-22`.
- Scaffold already gitignores `.nimbus/`:
  `crates/nimbus-bin/templates/convex/gitignore:1` and
  `crates/nimbus-bin/templates/cloud-functions/gitignore:1`.

## Stale init-plan reference

- `AGENTS.md:285` references `docs/plans/archive/nimbus-init-plan.md`, which does
  not exist; the real archived file is
  `docs/plans/archive/neovex-init-plan.md`. Routing fix tracked for BPD8
  docs-refs closeout.

## `^0.1.33` vs `^1.0.0` is test-fixture-only (not a production bug)

- `crates/nimbus-bin/src/node.rs:633,636,657,660,690,722,745,780` — the
  `"^1.0.0"` strings are all inside `#[cfg(test)]` fixtures. Production renders
  `^0.1.33` from `build.rs`. BPD3 rewrites the fixtures to assert `file:`
  specifiers.

## Verifier baseline result

`bash scripts/verify-binary-embedded-package-distribution.sh` exits non-zero
with a `N passed, M failed` summary. At BPD0 the genuinely-satisfied conditions
(2 baseline proof, 3 packages-private, 4 no-publish, 15 external-Node fallback
exists) pass; every condition tied to unimplemented BPD1-BPD8 work fails. Run
`BPD_FULL=1 bash scripts/...sh` to additionally execute `cargo fmt --all
--check`, `make clippy`, `npm run docs:validate-refs:strict`, and
`git diff --check` for condition 22 (deferred to closeout otherwise).
