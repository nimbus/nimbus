# Binary-Embedded Package Distribution Plan (BPD)

- **Status:** `archived 2026-05-31` — completed baseline. All rows BPD0–BPD8
  `done`; `BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh`
  → 27 passed, 0 failed. See the closeout proof at
  `docs/plans/proof/binary-embedded-package-distribution/bpd8-closeout.md`.
- **Primary owner:** this plan (closed).
- **History:** kept every `packages/*` workspace package private and distributed
  them through the `nimbus` binary instead of npm.
- **Related plans / references:**
  - `docs/plans/distribution-plan.md` — parent binary/OCI/install distribution
    plan; this plan adds the JS-package channel that rides inside the binary
  - `docs/plans/archive/nimbus-init-plan.md` — completed `nimbus init`
    scaffolding baseline this plan retemplates (historical filename)
  - `docs/plans/node-default-runtime-support-hardening-plan.md` — owns the
    in-binary Node/V8 runtime that executes embedded codegen
  - `docs/adapters/convex/compatibility.md`,
    `docs/adapters/convex/ai-guidelines.md` — codegen + runtime-bundle contract
  - `docs/operating/cli.md` — `nimbus init` / `nimbus dev` / `nimbus codegen`
    user-facing contract

---

## Why this plan exists

Every package under `packages/*` is marked `"private": true` and there is no
npm-publish workflow anywhere in `.github/workflows/`. The product intent is to
keep it that way: the `nimbus` binary is the single distribution artifact, and
it should provision and update the JavaScript surfaces a developer app needs,
rather than the developer fetching them from the public npm registry.

The current authoring contract contradicts that intent in four concrete
places:

1. **`nimbus init` templates declare registry dependencies.**
   `crates/nimbus-bin/templates/convex/package.json.tmpl` renders
   `"convex": "^{{CONVEX_VERSION}}"` plus
   `"@nimbus/codegen": "^{{CODEGEN_VERSION}}"`, and the cloud-functions template
   renders `"@nimbus/codegen": "^{{CODEGEN_VERSION}}"`. `nimbus init --install`
   and `nimbus dev` shell out to `npm install`
   (`crates/nimbus-bin/src/init.rs`, `crates/nimbus-bin/src/node.rs`), which
   resolves those names from the public registry. `@nimbus/codegen` is private
   and unpublished, so a scaffolded `npm install` cannot succeed as templated.

2. **The "embedded" codegen pilot does not actually remove the npm dependency.**
   `crates/nimbus-bin/src/codegen.rs` has a real `CodegenRunner::EmbeddedPilot`
   path behind `NIMBUS_EXPERIMENTAL_EMBEDDED_CODEGEN`, but
   `ensure_embedded_codegen_package_available` still requires an installed
   `node_modules/@nimbus/codegen/package.json`. It only changes *where the
   codegen JS executes* (in-binary V8 vs external `node`), not *where the
   package comes from*. The bytes are still expected to arrive via npm.

3. **Adapter docs instruct registry installs.** `docs/adapters/firebase/*` and
   `docs/adapters/mongodb/*` say `npm install @nimbus/firebase` /
   `@nimbus/mongodb`, which cannot work while those packages stay private.

4. **The package dependency closure is not offline-safe yet.** A `file:`
   specifier only moves the first package root to disk; it does not make that
   package installable without its transitive dependencies. Today the Cloud
   Functions scaffold still declares public-registry packages
   (`firebase-functions`, `firebase-admin`, `typescript`), `convex` exposes a
   CLI that imports private `@nimbus/codegen`, and the first-party DynamoDB /
   MongoDB packages declare public-registry peer/runtime dependencies for the
   official third-party SDKs. The no-network proof is not meaningful until every
   Nimbus-owned or scaffold-owned package dependency needed by the supported
   flow is either bundled into the embedded payload, provisioned from the binary,
   or deliberately removed from the scaffold contract.

This plan closes the gap by making the binary embed the JS payload and
provision it into the developer app, so the entire authoring and runtime flow
works offline with zero registry access, and a binary upgrade updates the
provisioned packages in lockstep.

## Decision

- Keep **all** `packages/*` packages `"private": true`. Add **no** npm-publish
  workflow. Publishing stays explicitly out of scope.
- The binary **embeds** the built JS surfaces (version-locked to the binary)
  and **provisions** them into each developer app under a binary-owned,
  gitignored `.nimbus/packages/` directory.
- Scaffolded `package.json` files reference provisioned packages with `file:`
  specifiers, never registry version ranges.
- Codegen runs **in-binary** by default and consumes the embedded payload, so
  `@nimbus/codegen` disappears from the developer app entirely and remains a
  private internal build input only.
- Provisioned package roots are **dependency-closed for the supported Nimbus
  flow**: they must not require private/unpublished packages from npm, and they
  must not require a public-registry fetch for any dependency that Nimbus itself
  introduced into a scaffold or first-party SDK. Runtime dependencies are
  bundled into `dist/`, provisioned as additional binary-owned package roots, or
  removed from the committed/provisioned manifest. Arbitrary third-party
  packages a developer adds to their own app remain developer-owned and outside
  this plan's offline guarantee.
- Existing client-only apps get an explicit provisioning command (final CLI name
  chosen in BPD2, for example
  `nimbus packages provision firebase|mongodb|dynamodb|all`).
  `init`/`dev`/`codegen` may call the same reconciler automatically when an
  adapter is known, but docs must not rely on magical import scanning as the
  only way to obtain a package.
- The provisioned `convex` package must not expose a broken `convex codegen`
  path. Either its CLI delegates to the installed `nimbus` binary's embedded
  codegen path, or the `convex`/`nimbus-codegen` npm-style entrypoints are
  intentionally retired from docs and tests.
- **Separate the in-repo workspace package from the provisioned package.** The
  repo's own build depends on the *source* `convex` package: `nimbus-ui`'s
  `codegen` script runs `convex codegen` (`packages/nimbus-ui/package.json:7`),
  whose CLI imports `@nimbus/codegen` via the workspace symlink
  (`packages/convex/src/cli.mjs:3`), and `nimbus-server`'s build hard-fails
  without `packages/nimbus-ui/dist/` (`crates/nimbus-server/build.rs:21-29`).
  Dependency-closure/sanitization (BPD1) applies to the *provisioned* payload
  shipped to developer apps; it must not remove `@nimbus/codegen` or the CLI
  from the in-repo workspace package, or the `nimbus-server` build breaks.
  `nimbus-ui` and `demos/**` keep resolving `convex`/`nimbus` through the root
  npm workspace and are exempt from `file:`/provisioning.
- A reconcile step on `nimbus init`/`dev`/`codegen`/`deploy` detects
  binary-version drift and re-provisions, so "upgrade the binary" is the one and
  only way the developer's Nimbus packages move. Re-provisioning must invalidate
  the Node dependency-install fingerprint and force a reinstall so
  `node_modules` is not left stale after `nimbus upgrade`.

## Offline contract boundaries

The offline guarantee covers the **Nimbus-owned client/authoring surface**, not
arbitrary server-side SDKs. A dependency-closure trace (every `packages/*` +
both scaffold templates + the installed lockfile) sets these explicit
boundaries; BPD1 must encode them as a per-package disposition table in the
sanitized manifests, and BPD7 must prove only what is in-contract:

- **In-contract, offline-clean:** `nimbus` (pure-TS client SDK; `react`/
  `react-dom` stay developer-provided peers), the `@nimbus/firebase` client
  once its `@connectrpc/*` + `@bufbuild/protobuf` runtime dependencies are
  actually co-provisioned as embedded third-party roots, the `@nimbus/mongodb`
  URI-helper surface (`uri()` + types only), and `convex` once its dist is
  rebuilt (below).
- **`@nimbus/firebase`: deps co-provisioned as additional roots.** BPD1 emits a
  real provisioned dist (per-file `tsc` js+dts) and **keeps** `@connectrpc/connect`,
  `@connectrpc/connect-web`, and `@bufbuild/protobuf` in the sanitized manifest,
  co-provisioning those three as additional binary-owned package roots rather
  than bundling them. This is the canonical BPD implementation path unless the
  plan is deliberately revised again: **do not** also claim they are bundled or
  absent from the manifest. Rationale (decided during BPD1 on evidence): the
  toolchain has no `.d.ts` bundler (tsc 6.0.3 can't bundle declarations), the
  firebase public surface does not expose `@connectrpc`/`@bufbuild` types (they
  are used only by internal transport + generated protos), and all three are
  zero-dep pure ESM -- so co-provisioning reuses the proven per-file builder and
  avoids fragile bundled-declaration tooling. BPD1 is not done until those three
  roots are staged, embedded, checksummed, provisioned, and covered by the G4
  third-party-attribution gate (`make verify-third-party-attribution`). If that
  cannot be made true, the fallback is a plan update plus a bundling/inlining
  implementation that drops them from the sanitized manifest; the verifier must
  fail in the meantime.
- **`convex` is not a usable baseline today.** Its persistent `dist/` is a
  browser-only IIFE (`build.mjs` bundles only `src/browser.ts`); the `./server`,
  `./values`, and `./react` exports an app actually imports are not emitted.
  BPD1 must emit a multi-entry, dependency-closed `convex` dist (all four
  exports + `.d.ts`, inlining the `nimbus` surfaces it re-exports). `esbuild` is
  also miscategorized as a `convex` runtime dependency (only
  `differential.mjs`/`selftest.mjs` use it) and must be dropped from the
  sanitized manifest.
- **`@nimbus/mongodb`: helper-only, driver developer-supplied.** Nimbus
  provisions the helper package offline, but does not embed the official
  `mongodb` driver or its dependency graph (`bson`, `@mongodb-js/saslprep`,
  `mongodb-connection-string-url`, optional native peers, and related driver
  integrations). BPD1 keeps the provisioned package dependency-closed by
  emitting only the `uri()` helper surface and a sanitized manifest with no
  `mongodb` runtime dependency; docs/tests must make the official driver a
  developer-supplied/preinstalled dependency when an app actually uses it.
- **`@nimbus/dynamodb`: developer-supplied, out-of-contract.**
  `@aws-sdk/client-dynamodb` is an optional **peer** with a ~50–100-package
  `@aws-sdk`/`@smithy` graph that is not even in the repo lockfile. Nimbus does
  not embed the AWS SDK; the package ships only config helpers offline and the
  SDK stays developer-installed. BPD7 must prove the supported npm baseline does
  not fetch or fail on the absent optional peer when installing the provisioned
  helper offline; if it does, the sanitized manifest must drop the peer and move
  the SDK requirement fully into docs/tests.
- **Cloud Functions scaffold: external-Node / preinstall fallback.**
  `firebase-admin` and `firebase-functions` are large server-side packages,
  absent from the lockfile, and required by the generated CF scaffold:
  `firebase-functions` is imported by the entrypoint
  (`templates/cloud-functions/functions/src/index.ts`), while `firebase-admin`
  is declared by the generated functions package manifest. They are **not** part
  of the no-network claim (gate condition 11); CF authoring requires a
  registry/preinstall step or the external-Node runner, documented truthfully.

If a dependency is neither bundled, provisioned, nor in one of the
developer-supplied/fallback buckets above, the supported offline flow is
narrowed to exclude it explicitly — never claimed and silently broken.

## Resolved product decisions

- **Managed-service SDK distribution.** Decision on 2026-05-31: BPD's
  `"private": true` / no-publish invariant applies to the self-hosted binary
  distribution channel and to the current `packages/*` workspace packages.
  Managed-cloud SDK distribution is a deliberately separate future product
  channel, owned by a later launch/distribution plan. BPD must not publish
  `packages/nimbus` or `packages/convex`, add an npm-publish lane, or turn the
  current workspace packages into public SDK artifacts. BPD6 still must edit
  `docs/private/managed-service-launch-plan.md:112-116`, the generated
  `docs/private/managed-service-launch-plan.html:85`, and
  `docs/private/generate-launch-plan-pdf.py:455` so the launch plan records this
  deferred managed-cloud SDK decision instead of instructing npm publication.

## Resolved technical decisions

- **`esbuild` in embedded codegen.** Decision on 2026-05-31: do not frame
  `esbuild` as impossible inside V8. The precise boundary is profile and grant
  based. Production/application V8 profiles do not get subprocess/run authority,
  native-addon authority, or package-owned binary execution. The embedded
  codegen path, however, already uses `RuntimeLimits::tooling_node22()` and the
  tooling profile grants `$discovered_tooling` run targets; the focused runtime
  proof `cargo test -p nimbus-runtime tooling_node22_executes_esbuild_style_staged_binary`
  passes. Therefore BPD4 may run `esbuild` inside the Nimbus V8 tooling runtime
  only when `@nimbus/codegen`, `typescript`, `esbuild`, and the matching
  platform `@esbuild/*` package/binary are embedded or provisioned as
  checksummed tooling roots under approved paths, discovered as exact run
  targets, and proven with tests. The external-Node runner is **not** a BPD
  success path for Nimbus-owned codegen. For code we own, the enterprise-trust
  answer is to make the embedded tooling path work again, not to route around it.
  If an external-Node runner remains in the tree during or after BPD4, it must be
  diagnostic/transition-only, not documented as the supported path and not
  counted as proof for any in-contract offline surface. Do not broaden
  application-runtime grants to make `esbuild` work.

## Guiding Strategy

- **Extend established repo patterns, do not invent new ones.**
  - `rust-embed` of JS into the binary already exists: `nimbus-server` embeds
    `packages/nimbus-ui/dist/` at compile time. Reuse that mechanism for the
    package payload.
  - Build-input wiring already exists: the Makefile UI dependency graph
    (`UI_PKG`, `UI_DIST_INDEX`, codegen sentinels) is the model for making
    `packages/*/dist` a binary build input.
  - Version-locking already exists: `crates/nimbus-bin/build.rs` reads
    `packages/{convex,codegen}/package.json` into `NIMBUS_CONVEX_VERSION` /
    `NIMBUS_CODEGEN_VERSION`. The reconcile step reuses this.
  - The `.nimbus/` app-internal directory is already gitignored by the scaffold
    and already holds binary-generated artifacts (`.nimbus/convex/bundle.*`).
    `.nimbus/packages/` is the same shape.
- **Offline is the gate, not a nice-to-have.** Every phase proof that claims an
  in-contract offline surface must run `nimbus init` → install →
  `nimbus dev`/`codegen` with no network access and no registry, and show
  generated `_generated/*` plus a verified runtime bundle. The proof must cover
  dependency closure, not merely first-level `file:` specifiers. Any fallback
  surface (Cloud Functions, official MongoDB driver, AWS SDK) must be tested as
  preinstalled/developer-supplied and must not be counted as a no-network
  success.
- **`file:` specifiers over raw `node_modules` writes.** The binary owns
  `.nimbus/packages/` contents; `package.json` points at them declaratively so
  a later `npm install` does not prune them and all package managers resolve
  them offline.
- **Truthful privacy.** The verifier must fail if any package loses
  `"private": true` or if an npm-publish workflow appears.

## Control Plane Protocol

Treat this document as the durable control plane for BPD execution. The plan
file, proof artifacts, verifier script, and git history are the progress state;
chat history is not.

- **Status values:** `pending`, `in_progress`, `done`, `blocked`.
- **Single active row:** at most one BPD row may be `in_progress`.
- **Ordering:** execute BPD0 through BPD8 in order unless the plan is updated with
  an explicit dependency-safe reason to split or reorder work.
- **Before code changes:** mark the active row `in_progress`, inspect dirty files,
  and identify the row's proof artifact and verifier conditions.
- **Before marking done:** all row success criteria, the row's verification-matrix
  checks, and the relevant aggregate verifier conditions must pass. Record exact
  commands, results, counts, and any residual risk in the Execution Log first.
- **Blocked work:** use `blocked` only for a concrete product/security/external
  decision that cannot be resolved from repo context. Record the blocker, the
  failed attempts, and the smallest next decision needed.
- **Verifier ownership:** BPD0 creates
  `scripts/verify-binary-embedded-package-distribution.sh` as a failing control
  gate. Each later BPD row makes its own verifier conditions pass without
  deleting, weakening, or bypassing earlier conditions.
- **Evidence artifacts:** each BPD row writes a concise proof note under
  `docs/plans/proof/binary-embedded-package-distribution/` before it is marked
  `done`. Proof notes must include file:line evidence for structural claims,
  exact command output summaries, and any intentional fallback classification.
- **Git operations:** do not stage, commit, push, or open a PR unless the user
  explicitly asks. Keep the worktree inspectable through ordinary file changes
  and Execution Log checkpoints.

## Provisioning model

```
BUILD TIME (nimbus/nimbus CI)
  packages/* (private) --esbuild/tsc--> dependency-closed packages/*/dist
                           + sanitized provisioned package manifests
                         --rust-embed--> nimbus binary
                                           (version-locked via build.rs;
                                            checksummed manifest)

DEVELOPER-APP TIME
  nimbus init   -> scaffold app source + package.json (file: specifiers, no @nimbus/codegen)
                -> provision embedded payload into  <app>/.nimbus/packages/*  + .version stamp
  existing app  -> nimbus packages provision <adapter|all>
                -> writes the same binary-owned .nimbus/packages/* payload
  npm install   -> resolves file: deps from disk (offline, no registry)
  nimbus dev    -> reconcile: if binary version != .nimbus/packages/.version, re-provision
                -> in-binary codegen reads convex/*.ts -> emits _generated/* + .nimbus/convex/bundle.*
                -> serve: SHA-256 verify bundle -> run in embedded V8 (HostBridge)

UPGRADE LOOP
  nimbus upgrade -> new binary, new embedded payload
                 -> next nimbus dev reconciles version drift -> re-provisions .nimbus/packages/*
```

## In Scope

- Embedding the built JS surfaces (`convex`/`nimbus` client+server surfaces,
  and the `@nimbus/firebase`/`@nimbus/dynamodb`/`@nimbus/mongodb` adapter
  helpers) into the binary, version-locked.
- Closing the package dependency graph for every embedded/provisioned
  Nimbus-owned package and every package the scaffold introduces. This includes
  removing or bundling `convex` -> `@nimbus/codegen`, bundling/removing
  first-party SDK runtime dependencies where they would otherwise require the
  registry, and deciding whether the Cloud Functions scaffold gets
  binary-provisioned authoring shims or drops registry-owned dependencies from
  the offline scaffold path.
- A `nimbus`-owned provisioning step that materializes embedded packages into
  `<app>/.nimbus/packages/*` idempotently, with a version stamp, plus an
  explicit CLI entrypoint for existing client-only apps.
- Retemplating `nimbus init` scaffolds to `file:` specifiers and removing
  `@nimbus/codegen` from app dependencies. The core defect is structural:
  templates declare *registry version ranges for private packages* at all
  (`"convex": "^{{CONVEX_VERSION}}"`, `"@nimbus/codegen": "^{{CODEGEN_VERSION}}"`).
  Production renders `^0.1.33` from `build.rs`; the `^1.0.0` strings are
  `#[cfg(test)]` fixtures in `crates/nimbus-bin/src/node.rs` only, so the
  "version inconsistency" is a test-fixture alignment, not a production bug —
  the tests must be rewritten to assert the new `file:` specifiers.
- Promoting in-binary codegen to the default runner and sourcing its package
  from the embedded payload instead of `node_modules`; external Node must not be
  the supported fallback for Nimbus-owned codegen, though a diagnostic-only
  runner may remain if it is not counted as offline/in-binary proof. Preserve,
  delegate, or deliberately retire the npm-style `convex codegen` /
  `nimbus-codegen` entrypoints so no documented path imports a missing private
  package.
- A reconcile-on-version-drift step shared by `init`/`dev`/`codegen`/`deploy`.
- On-demand provisioning of adapter SDKs only when an app uses them.
- CI, release, and coverage workflow updates so package payload artifacts are
  present wherever Rust builds consume `nimbus-bin`: CI/coverage may share the
  Linux payload artifact like the existing UI artifacts, while release jobs
  build the payload target-locally because the embedded tooling closure contains
  a native `@esbuild/*` binary.
- Doc rewrite removing registry-install instructions and documenting the
  binary-provisioned flow across README, adapter docs, CLI docs, package READMEs,
  demos, compatibility docs, and private launch-plan sources.
- Full offline + integrity proof of the end-to-end flow.

## Goal Control Plane Objective

When this plan is activated as a goal, use this objective:

Complete `docs/plans/archive/binary-embedded-package-distribution-plan.md` autonomously
end to end. Success means every `packages/*` package stays `"private": true`
with no npm-publish workflow added; the `nimbus` binary embeds the built JS
surfaces version-locked to the binary; `nimbus init` provisions them into
`<app>/.nimbus/packages/*` with a version stamp and scaffolds `package.json`
with `file:` specifiers and no `@nimbus/codegen` app dependency; generated-app
install and `nimbus dev`/`codegen` complete with no network or registry access
for every in-contract scaffold surface because the provisioned package
  dependency closure is complete; in-binary codegen is the default and consumes
  the embedded payload for all Nimbus-owned in-contract codegen paths, without
  relying on external Node as the supported fallback;
documented `convex codegen` / `nimbus-codegen` paths either work through the
binary or are removed; a reconcile step re-provisions on binary-version drift
and forces Node dependency reinstall when needed; `@nimbus/firebase`,
`@nimbus/mongodb` helper-only, and `@nimbus/dynamodb` helper-only packages are
provisioned through an explicit command and on demand from known adapters;
official MongoDB / AWS / Firebase server SDKs stay truthfully documented as
developer-supplied or fallback-only unless preinstalled; CI and release
workflows carry the package payload artifacts wherever Rust builds need them;
docs describe the binary-provisioned flow with no Nimbus-package
registry-install instructions; and
`BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh` prints
`N passed, 0 failed`. Each phase proof must demonstrate the offline path for
the surface it claims: scaffold, install, generate, and serve with the network
disabled or the relevant third-party dependency explicitly preinstalled and
outside the no-network claim.

## Goal Prompt

Paste the concise command below when starting the autonomous goal. The detailed
startup, execution, and stop rules intentionally live in this plan's Control
Plane Protocol, Verification Matrix, Completion Gate, and Execution Log instead
of being repeated inside the `/goal` body.

```text
/goal Complete docs/plans/archive/binary-embedded-package-distribution-plan.md in /Users/jack/src/github.com/nimbus/nimbus until BPD0-BPD8 are done, the Execution Log records evidence for every row, and BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh prints N passed, 0 failed. Use the plan itself as the control plane; follow its Control Plane Protocol, Verification Matrix, and Completion Gate.
```

## Out Of Scope

- Publishing any `packages/*` package to npm or any registry.
- Adding an npm-publish CI lane.
- Changing the V8 runtime bundle execution or its SHA-256 verification
  contract (owned by the Convex compatibility + Node runtime plans).
- Replacing npm as the dev-app package manager; `file:` deps must still work
  through the developer's existing npm/pnpm/yarn.
- Shipping a Nimbus-native client that diverges from the current
  `convex`/`nimbus` surfaces.
- Embedding application user code or third-party SDKs the developer adds
  themselves. Third-party packages that Nimbus introduces into its own
  scaffolded flow or first-party SDK payload are not covered by this exclusion;
  BPD1 must either bundle/provision/remove them or narrow the supported offline
  flow explicitly.

## Ledger

| BPD | Work | Verifiable success criteria | Status |
| --- | --- | --- | --- |
| BPD0 | Baseline and verifier scaffold. Record current state: 7/7 packages private, no publish workflow, init templates declare `convex` + `@nimbus/codegen` registry deps, Cloud Functions scaffold declares public-registry deps, embedded-codegen pilot still requires `node_modules/@nimbus/codegen`, provisioned-package dependency closure is currently incomplete (`convex` -> `@nimbus/codegen`, first-party SDK public deps), `convex` CLI imports `@nimbus/codegen`, CI/release/coverage only carry `nimbus-ui` artifacts, `rust-embed` + build.rs version-lock precedents, the stale init-plan reference, and the `^0.1.33`/`^1.0.0` template/test inconsistency. Ship the aggregate verifier failing on every unimplemented gate. | `docs/plans/proof/binary-embedded-package-distribution/bpd0-baseline.md` exists and records the facts above with file:line evidence; `scripts/verify-binary-embedded-package-distribution.sh` exists and exits non-zero with a `N passed, M failed` summary; docs refs and `git diff --check` pass. | done |
| BPD1 | Build dependency-closed JS payloads and embed them in the binary. **Prerequisite:** today only `convex` (a browser-only IIFE bundle) and `nimbus-ui` emit a persistent `dist/`; `nimbus`, `dynamodb`, `mongodb`, `firebase`, and `codegen` either have no real `build` script or build into a throwaway `mkdtemp` smoke dir (`packages/*/src/selftest.mjs`). First give each package a real build that emits the full surface to `packages/<name>/dist`, plus a sanitized provisioned `package.json` whose runtime/install deps are closed for the supported no-network flow. Then make `packages/*/dist` and provisioned manifests binary build inputs via the Makefile dependency-graph pattern; add `rust-embed` to `crates/nimbus-bin/Cargo.toml` (not currently a dependency); embed via `rust-embed`/`include_*`; emit an embedded manifest of `{package, version, checksums}` locked to the binary; wire CI/release/coverage artifact upload/download + mtime refresh anywhere Rust builds consume `nimbus-bin`. | Each required package emits a stable dependency-closed `dist/` containing the full embedded surface (not a smoke build); provisioned manifests contain no unsupported private package dependency and no unsupported registry dependency needed by the supported offline proof; a per-package disposition table encodes the Offline contract boundaries (clean / embedded dependency root / allowed peer / developer-supplied / fallback); the provisioned `@nimbus/firebase` dist keeps `@connectrpc/connect`, `@connectrpc/connect-web`, and `@bufbuild/protobuf` only because those three third-party package roots are staged, embedded, checksummed, provisioned, and covered by attribution; if implementation switches to bundling/inlining, those deps must instead be absent from the sanitized manifest and the plan updated in one place; the provisioned `convex` dist emits all four declared exports (`server`/`values`/`react`/`browser`), not the browser-only IIFE, and drops `esbuild` from its sanitized manifest; the provisioned `@nimbus/mongodb` package emits only the helper surface and drops `mongodb` from its sanitized runtime manifest, with the official MongoDB driver marked developer-supplied/preinstalled; optionally remove or peer-ify the vestigial source `packages/mongodb` driver dependency only after tests prove no shipped import needs it; `@aws-sdk/client-dynamodb` and CF `firebase-admin`/`firebase-functions` are marked developer-supplied/fallback rather than bundled; the in-repo `nimbus-ui` `convex codegen` build still passes (the source workspace package is untouched); the provisioned `convex` package has no broken codegen bin; a focused test proves the binary exposes each embedded package's bytes, version, and checksums; the embedded version equals the source `packages/<name>/package.json` version; `make build`, CI, coverage, and release rebuild/consume the payload like UI artifacts; binary size delta is recorded. | done |
| BPD2 | Provision into developer apps through a shared reconciler and explicit CLI surface. Add a `nimbus`-owned step that writes the embedded payload to `<app>/.nimbus/packages/<name>` plus a `.nimbus/packages/.version` stamp, idempotently and atomically. Add an explicit command for existing client-only apps (final name chosen here, for example `nimbus packages provision firebase|mongodb|dynamodb|convex|all`), and have `init`/`dev`/`codegen` call the same reconciler when the adapter is known. | Provisioning into an empty app writes every required package with a valid sanitized `package.json`; re-running with the same binary is a no-op; a partial/corrupt provision is fully rewritten; the stamp records the binary version and embedded manifest digest; `.nimbus/packages/` is gitignored by the scaffold; existing Firebase/MongoDB apps have a documented command that provisions the right package without depending on import scanning. **Ordering + lockfile policy:** once generated apps switch to `file:./.nimbus/packages/*` specifiers, `.nimbus/` remains gitignored and the provisioned bytes are not committed, so a clean checkout or app CI runner for that generated app must provision *before* `npm install`/`npm ci` or the `file:` targets are absent and install fails. Decide and document the committed-lockfile policy (either do not commit the scaffolded lockfile, or commit it and require `nimbus packages provision` before `npm ci`); a generated-app fresh-clone-then-install proof must pass. | done |
| BPD3 | Retemplate the scaffold. `package.json` templates use `file:./.nimbus/packages/<name>` specifiers; remove `@nimbus/codegen` from app deps; either close the Cloud Functions scaffold dependency graph so the supported scaffold installs with the registry unreachable, or explicitly mark Cloud Functions as a preinstalled-dependency / external-Node fallback outside the no-network proof; add tsconfig path mapping as the typecheck fallback; resolve the version-string inconsistency. | Rendered convex + cloud-functions templates contain zero registry version ranges for Nimbus packages and no `@nimbus/codegen` dependency; the Cloud Functions template has no public-registry dependency required for the no-network proof unless that dependency is provisioned from the binary; if CF stays fallback-only, the rendered template and docs say so clearly and tests assert the fallback path instead of claiming registry-unreachable install; `init.rs`/`node.rs` agree on the rendered specifiers; `npm install` in freshly scaffolded Convex apps succeeds with the registry unreachable; Cloud Functions does too only if kept in the no-network contract; tests assert the rendered output. | done |
| BPD4 | Default in-binary codegen sourced from the embedded payload. Make `CodegenRunner` default to the in-binary path (`codegen.rs:227-247`); source the codegen package from the embedded/provisioned payload rather than requiring `node_modules/@nimbus/codegen` (`ensure_embedded_codegen_package_available` / `codegen_package_manifest_path` / `resolve_installed_codegen_entry`); remove external Node from the supported Nimbus-owned codegen path (or quarantine it as diagnostic-only if retained); make the documented `convex codegen` / `nimbus-codegen` paths either delegate to the binary or disappear from docs/tests. **Cloud Functions resolution (BPD7):** CF runtime bundling needs esbuild plugins that do not run in the in-binary V8 tooling runtime, so **Cloud Functions is a deliberate out-of-contract surface** and runs codegen on the external Node.js runner as its *supported* path (CF's Firebase server SDKs are developer-supplied/preinstall anyway). `ensure_embedded_codegen_layout_supported` (`codegen.rs`) is retained to reject *explicit* in-binary requests for CF with a clear message; CF apps auto-select the external runner on the default path. This is the one authoring surface outside the contract. **auth.config stays in-contract:** rather than bundle it with esbuild, auth.config is evaluated in-binary by the compile-time AST interpreter (`evaluateModuleDefaultExport`), so the whole default Convex surface — schema, server, http, auth.config — is in-binary/offline with no external Node. **Tooling-profile `esbuild` line:** `@nimbus/codegen` imports `typescript` (used in-binary by the AST interpreter) and `esbuild` (used only on the external-Node CF path). When esbuild is staged in the tooling profile it is allowed only with staged package roots, the matching platform `@esbuild/*` binary, checksum/attribution coverage, and exact `$discovered_tooling` run-target permission. Do not run `esbuild` in application/untrusted profiles, do not broaden app runtime grants, and do not route any in-contract Convex surface (including auth.config) to external Node. | `nimbus codegen` and `nimbus dev` generate `_generated/*` and a verified `.nimbus/convex/bundle.*` with no `@nimbus/codegen` in `node_modules` and the registry unreachable for every in-contract Convex surface, including auth-config paths; Cloud Functions codegen is out-of-contract and intentionally auto-routes to external Node (esbuild bundling is unsupported in the in-binary runtime — see `## Offline contract boundaries`), and CF package install/runtime is developer-supplied/preinstall; every documented npm-style codegen command either delegates successfully to the binary or is removed; any retained external-Node runner is diagnostic-only and not counted as support for the BPD offline/in-binary claim; the experimental env flag is retired or repurposed away from normal support; tests cover embedded tooling and both adapter layouts. | done |
| BPD5 | Reconcile on version drift. `init`/`dev`/`codegen`/`deploy` (`deploy.rs:146` also runs codegen) compare the binary's embedded version against `.nimbus/packages/.version` and re-provision when they differ. A re-provision must invalidate the Node dependency-install fingerprint (`node.rs:299-309` keys only on `package.json`+lockfile, which `file:` specifiers leave unchanged) and force a reinstall, or `node_modules` stays stale after `nimbus upgrade`. | Simulating a binary upgrade (changed embedded version) triggers a re-provision AND a dependency reinstall on the next `nimbus dev`, with `node_modules` content observably updated; a matching version is a no-op; reconcile is atomic and logged; a test drives the upgrade→re-provision→reinstall→no-op sequence. | done |
| BPD6 | Adapter SDKs on demand + privacy assertion + doc rewrite. Provision `@nimbus/firebase`/`dynamodb`/`mongodb` only for apps that use them or when the explicit package command asks for them; rewrite Nimbus-package registry-install and external-node-default instructions to the binary-provisioned flow at least in `README.md`, `docs/adapters/firebase/README.md:33`, `docs/adapters/firebase/migration.md:61`, `docs/adapters/mongodb/README.md:80,128`, `docs/adapters/mongodb/drivers.md:29`, `docs/adapters/mongodb/examples.md`, `docs/adapters/convex/README.md:29,51`, `docs/adapters/convex/compatibility.md:45-52`, `docs/adapters/cloud-functions/README.md:25-27`, `docs/operating/cli.md:481-528,576,583,588-592,637-642`, and package READMEs that advertise `@nimbus/codegen` as an app-installed package. Keep third-party driver instructions truthful: MongoDB docs may still require installing/preinstalling the official `mongodb` driver, while Nimbus only provisions `@nimbus/mongodb`. Apply the resolved managed-cloud SDK decision in `docs/private/managed-service-launch-plan.md:112-116`, generated `docs/private/managed-service-launch-plan.html:85`, and generator `docs/private/generate-launch-plan-pdf.py:455`: BPD keeps current `packages/*` private for self-hosted binary distribution, and managed-cloud SDK distribution is deferred to a separate future channel rather than implemented by publishing these packages. Optionally align `demos/**` package.json from `"*"`/pinned workspace deps to the scaffold's `file:` shape for consistency; assert no package lost `"private": true` and no npm-publish workflow exists. | An existing client app can run the documented provisioning command and get the requested Nimbus adapter helper package; one that does not request/import an adapter is unaffected; no public docs instruct a registry install of a Nimbus package or an npm-style codegen command that no longer works; third-party driver/SDK install requirements are documented separately from Nimbus package provisioning; the launch plan no longer says to publish these workspace packages to npm and instead records the deferred managed-cloud SDK channel; all 7 packages remain private; the verifier fails if any becomes public or a publish workflow is added. | done |
| BPD7 | Offline + integrity end-to-end. Prove scaffold→install→generate→serve with the network disabled for every in-contract scaffold surface; checksum-verify provisioned package contents against the embedded manifest; prove the explicit client-adapter provisioning command offline. | A no-network integration proof runs `nimbus init`, `npm install`, `nimbus dev`, queries a generated function, and shows a SHA-256-verified runtime bundle for Convex and any Cloud Functions path kept in-contract; if CF is fallback-only, the proof instead verifies the documented preinstalled-dependency / external-Node path and does not count CF as a no-network success; a separate no-network proof provisions Firebase, MongoDB helper-only, and DynamoDB helper-only Nimbus packages into an existing app without registry access, while official `mongodb` / AWS SDK driver use is proven only when preinstalled; provisioned package bytes match the embedded manifest checksums; tamper detection is proven by a negative test. | done |
| BPD8 | Closeout and archive. Finish all rows, prove final state, move plan to archive, point routing at the archived baseline and the `distribution-plan.md` parent. | Every row `done`; execution log records commands and results; final verifier prints `N passed, 0 failed`; `cargo fmt --all --check`, `make clippy`, strict docs refs, and `git diff --check` pass; plan archived and `docs/plans/README.md` updated. | done |

## Verification Matrix

| BPD | Proof artifact | Aggregate verifier coverage | Minimum verification before `done` |
| --- | --- | --- | --- |
| BPD0 | `docs/plans/proof/binary-embedded-package-distribution/bpd0-baseline.md` | Conditions 1-4 plus the intentional failing scaffold for all unimplemented conditions. | `bash scripts/verify-binary-embedded-package-distribution.sh` exits non-zero with a counted `N passed, M failed` summary; `npm run docs:validate-refs:strict`; `git diff --check`. |
| BPD1 | `docs/plans/proof/binary-embedded-package-distribution/bpd1-embedded-payloads.md` | Conditions 5-7, 23-24. | `npm run build`; focused package build/typecheck tests for `convex`, `nimbus`, `@nimbus/firebase`, `@nimbus/mongodb`, `@nimbus/dynamodb`, and `@nimbus/codegen`; manifest/staging tests prove every kept runtime dependency is either an embedded package root (`nimbus` for `convex`; `@connectrpc/*` + `@bufbuild/protobuf` for Firebase), an allowed peer/optional peer, or an explicit developer-supplied/fallback boundary; Convex `esbuild` and MongoDB `mongodb` are absent from sanitized runtime manifests; focused Rust tests proving embedded package manifest bytes/checksums/version equality; `make build`; verifier conditions for dependency closure and CI/release artifact wiring pass. |
| BPD2 | `docs/plans/proof/binary-embedded-package-distribution/bpd2-provisioning-reconciler.md` | Conditions 12-13, 18, 25-26. | Focused CLI tests cover empty app provisioning, idempotent re-run, corrupt partial rewrite, `.version` stamp, explicit adapter command, generated-app clean checkout provision-before-`npm ci`, and binary-version drift re-provision plus dependency reinstall; verifier conditions pass. |
| BPD3 | `docs/plans/proof/binary-embedded-package-distribution/bpd3-scaffold-template.md` | Conditions 8-11, 25. | Focused init/template tests prove rendered Convex and Cloud Functions package manifests use `file:` for Nimbus packages, drop `@nimbus/codegen`, and classify CF as in-contract or fallback; generated-app install proof runs with registry unreachable for every in-contract scaffold; verifier conditions pass. |
| BPD4 | `docs/plans/proof/binary-embedded-package-distribution/bpd4-embedded-codegen-default.md` | Conditions 14-16, 23-24. | Focused `nimbus codegen` / `nimbus dev` tests prove in-binary default uses embedded/provisioned bytes with no `node_modules/@nimbus/codegen`; if an in-contract path imports `esbuild`, tests prove the embedded tooling runtime stages and executes the matching platform binary under exact tooling run grants; external Node is removed from the supported path or retained only as diagnostic-only and never as the proof for in-contract offline success; documented npm-style commands either delegate successfully or are removed with tests/docs updated; Convex and Cloud Functions codegen adapter-layout tests pass on the embedded tooling path when claimed in-contract. |
| BPD5 | `docs/plans/proof/binary-embedded-package-distribution/bpd5-version-drift-reconcile.md` | Conditions 18, 25-26. | Tests simulate binary embedded-manifest drift across `init`/`dev`/`codegen`/`deploy`, prove atomic re-provision, prove no-op on matching version, and prove dependency-state fingerprint/reinstall changes when provisioned `file:` payload bytes change. |
| BPD6 | `docs/plans/proof/binary-embedded-package-distribution/bpd6-docs-and-privacy.md` | Conditions 3-4, 13, 19-20, 23, 27. | Docs/package README/demo/private-launch-plan scan proves no Nimbus-package registry install or npm publish instruction remains for the current workspace packages; all 7 packages stay private; no publish workflow or `publishConfig` exists; the resolved managed-cloud SDK decision is recorded in the launch-plan sources as a deferred separate channel; third-party driver/SDK instructions are separated from Nimbus package provisioning; strict docs refs pass. |
| BPD7 | `docs/plans/proof/binary-embedded-package-distribution/bpd7-offline-integrity.md` | Conditions 6, 11, 17, 19, 21, 23. | End-to-end offline proof runs generated-app scaffold -> provision -> install -> codegen/dev -> query/invocation for every in-contract surface with registry unreachable; DynamoDB helper install proves the absent optional AWS SDK peer does not fetch or fail under the supported npm baseline, or the sanitized manifest drops the peer; CF fallback, MongoDB driver, and AWS SDK paths are proven only with preinstalled dependencies when classified out of contract; SHA-256 manifest verification and tamper negative test pass. |
| BPD8 | `docs/plans/proof/binary-embedded-package-distribution/bpd8-closeout.md` | All conditions 1-27. | `BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh` prints `N passed, 0 failed`; `cargo fmt --all --check`; `make clippy`; `npm run docs:validate-refs:strict`; `git diff --check`; plan moved to archive and `docs/plans/README.md` routes to the archived baseline. |

## Completion Gate

`BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh` exits
0 with a summary line `N passed, 0 failed`. The verifier must check at least:

1. Plan is active or archived and every ledger row is `done` at closeout.
2. BPD0 baseline proof exists and records the current registry-coupled state
   with file:line evidence.
3. All `packages/*/package.json` retain `"private": true` (7/7).
4. No npm-publish workflow exists under `.github/workflows/` and no
   `publishConfig`/registry publish step is present.
5. The binary embeds each required package with a version equal to the source
   `packages/<name>/package.json` version.
6. Provisioned package manifests are dependency-closed for every supported
   no-network flow. The verifier enumerates every provisioned manifest and
   staged package root (`nimbus`, `convex`, `@nimbus/firebase`,
   `@nimbus/mongodb`, `@nimbus/dynamodb`, and any third-party Firebase roots).
   Any runtime dependency must be one of: another embedded/provisioned root, an
   allowed peer/optional peer with proven offline install behavior, or a
   documented developer-supplied/fallback dependency outside the no-network
   claim. No unsupported registry dependency, missing private package, or broken
   `convex` codegen bin may survive.
7. `make build`, CI, coverage, and release treat `packages/*/dist` plus
   provisioned manifests as binary build inputs. CI/coverage may pass the Linux
   payload through artifact upload/download + mtime refresh, but release builds
   must stage target-local payloads and assert the expected `@esbuild/*`
   platform for each release target.
8. Rendered `nimbus init` templates contain `file:` specifiers for Nimbus
   packages and zero registry version ranges for them.
9. Rendered templates contain no `@nimbus/codegen` dependency.
10. Rendered templates contain no registry version range for any Nimbus
   package, and the `node.rs` `#[cfg(test)]` fixtures (the `^1.0.0` strings)
   are rewritten to assert the new `file:` specifiers.
11. Cloud Functions scaffold install succeeds with the registry unreachable, or
    Cloud Functions is explicitly documented and tested as an external-Node /
    preinstalled-dependency fallback outside the no-network success claim.
12. Provisioning writes `<app>/.nimbus/packages/*` and a `.version` stamp; the
    scaffold gitignores `.nimbus/`.
13. An explicit package-provisioning command exists for existing client-only
    apps and docs use it for Firebase/MongoDB/DynamoDB.
14. In-binary codegen is the default and does not require
    `node_modules/@nimbus/codegen`.
15. External Node is not required for any supported Nimbus-owned codegen path;
    any retained external-Node runner is diagnostic-only and not counted as
    offline/in-binary success.
16. Documented `convex codegen` / `nimbus-codegen` paths either work through
    the binary/provisioned payload or are removed from docs and tests.
17. A no-network proof shows `nimbus init` → `npm install` → `nimbus dev`
    succeeding with the registry unreachable for every in-contract scaffold
    surface.
18. Reconcile re-provisions on binary-version drift and is a no-op on match.
19. Adapter SDKs are provisioned only for apps that request/import them.
20. No public docs, package READMEs, demos, or private launch-plan sources
    instruct a registry install or npm publish of a Nimbus package; docs
    describe the binary-provisioned flow.
21. Provisioned package bytes verify against the embedded manifest checksums,
    with a proven tamper-detection negative test.
22. `cargo fmt --all --check`, `make clippy`, strict docs refs, and
    `git diff --check` pass.
23. The Offline contract boundaries are documented and BPD7 proves only the
    in-contract surfaces; Firebase's `@connectrpc/*` + `@bufbuild/protobuf`
    dependencies are in-contract only after they are embedded/provisioned
    third-party roots with attribution, while the official MongoDB driver,
    `@nimbus/dynamodb`'s AWS SDK peer, and Cloud Functions
    (`firebase-admin`/`firebase-functions`) are explicitly excluded from the
    no-network claim unless preinstalled rather than silently broken; the
    DynamoDB helper's absent optional peer either installs offline under the
    supported npm baseline or is dropped from the sanitized manifest.
24. The provisioned `convex` dist emits all declared exports
    (`server`/`values`/`react`/`browser`), and the in-repo `nimbus-ui`
    `convex codegen` build still passes (the source workspace package is
    untouched).
25. A generated-app fresh-clone-then-install
    (provision-before-`npm ci`) path is handled and proven, and the
    committed-lockfile policy is documented.
26. Re-provision on binary-version drift forces a Node dependency reinstall
    (no stale `node_modules`), proven by test.
27. The managed-service SDK-distribution decision is resolved and recorded:
    BPD keeps the current `packages/*` private for the self-hosted binary
    distribution channel, managed-cloud SDK distribution is deferred to a
    separate future product channel, and BPD6's launch-plan edit reflects that
    branch without publishing these packages.

## Offline contract boundaries

The in-binary/offline contract covers the **default Convex scaffold**: `nimbus
init convex` → `nimbus packages provision` → `npm install` → in-binary codegen →
serve, all with the registry unreachable. Each provisioned dependency falls into
one disposition:

| Package | Disposition | Rationale |
| --- | --- | --- |
| `convex` (dist: server/values/react/browser) | **clean** | Sanitized manifest; only dep is `nimbus` (rewritten to `file:../nimbus`). |
| `nimbus` (dist: server/values/react/browser/rest) | **clean** | Sanitized manifest; no runtime deps. |
| `@nimbus/firebase` | **embedded root + co-provisioned roots** | Keeps `@connectrpc/connect`, `@connectrpc/connect-web`, `@bufbuild/protobuf`; all three are staged, embedded, checksummed, provisioned, and attributed in `NOTICE`. |
| `@bufbuild/protobuf`, `@connectrpc/connect`, `@connectrpc/connect-web` | **embedded third-party roots** | Zero-runtime-dep pure ESM. Staging strips `devDependencies` + `scripts` (npm installs the devDependencies of `file:` links — e.g. protobuf's `upstream-protobuf` — and would otherwise fetch them). `check-package-closure.mjs` fails if any survive. |
| `@nimbus/mongodb` | **clean (helper-only)** | The official `mongodb` driver is **developer-supplied** (preinstalled), not provisioned. |
| `@nimbus/dynamodb` | **clean (helper-only)** | `@aws-sdk/client-dynamodb` is an **optional, developer-supplied** peer (`peerDependenciesMeta.optional`). |
| `react`, `react-dom` (peers of `convex`/`nimbus`) | **allowed developer-supplied peers** | Marked `peerDependenciesMeta.optional` (matching upstream Convex), so an offline `npm install` of a backend scaffold does not fetch them. |

The **entire default Convex authoring surface is in-contract and runs
in-binary**: schema, server definitions, http routes, and **`auth.config.{ts,js}`**.
auth.config is evaluated by the compile-time TypeScript AST interpreter
(`compile_time_interpreter.mjs` `evaluateModuleDefaultExport`) — the same path
used for schema/server extraction — which statically evaluates the module's
`export default` (literals, hoisted `const`s, template strings, and
`process.env.*` reads via an opt-in interpreter global) with **no esbuild and no
dynamic import**. It runs in the in-binary V8 tooling runtime with the registry
unreachable.

**Out of the in-binary/offline contract** — exactly one authoring surface:

- **Cloud Functions** (both the `firebase.json` layout and the
  `@google-cloud/functions-framework` framework variant). This is a deliberate
  product boundary, not a temporary gap: CF runtime bundling needs esbuild
  plugins (dynamic virtual modules) that do not run in the in-binary V8 tooling
  runtime today, and CF's server SDKs (`firebase-admin`, `firebase-functions`)
  are developer-supplied registry/preinstall. A detected Cloud Functions app
  therefore runs codegen on the **external Node.js runner** (with a one-line
  `info:` notice). For Cloud Functions, the external runner is the *supported*
  path for that surface — not a diagnostic fallback. The external runner is also
  available as a `diagnostic/transition-only` opt-out for the in-contract Convex
  surface via `NIMBUS_CODEGEN_RUNNER=external-node`, but that opt-out is never
  the supported Convex path and is never counted as the BPD offline/in-binary
  proof.

  Lifting Cloud Functions into the in-binary contract would require a
  `nimbus/deno` `child_process`/`worker_threads` IPC fix so esbuild's plugin
  path runs in-binary; that is tracked as a separate follow-up and is explicitly
  out of scope for this plan.

**Lockfile / clone-then-install policy.** The scaffold gitignores `.nimbus/`
(provisioned bytes are not committed) and does not commit a `package-lock.json`.
On a fresh clone the `file:./.nimbus/packages/*` targets are therefore absent, so
**provisioning must run before `npm install`/`npm ci`** — otherwise npm links a
dangling symlink (it exits 0 but the target is missing, so imports fail at
runtime). `nimbus init` provisions right after scaffolding, and `nimbus dev`
provisions (via `provision::ensure`) before its install loop, so the supported
flow never needs a manual `nimbus packages provision`. Apps that wire their own
CI must run `nimbus packages provision` before `npm ci`.

## Execution Log

| Date | BPD | Status | Files touched | Verification | Notes |
| --- | --- | --- | --- | --- | --- |
| 2026-05-31 | BPD0 | done | `docs/plans/proof/binary-embedded-package-distribution/bpd0-baseline.md` (new); `scripts/verify-binary-embedded-package-distribution.sh` (new, +x) | verifier → `4 passed, 23 failed` (exit 1, intended failing control gate); `npm run docs:validate-refs:strict` → pass (241 files); `git diff --check` → clean | Baseline recorded with file:line evidence for all defects + precedents. 27-condition aggregate verifier scaffolded; baseline PASS = conds 2/3/4/15. Fixed an initial C12 false-pass (bare "provision" matched unrelated machine-config code). No commits per Control Plane Protocol. |
| 2026-05-31 | BPD1 | in_progress | `scripts/build-js-package.mjs` (new); `packages/mongodb/package.json` (build script) | `npm run build -w @nimbus/mongodb` → exit 0, emits dist (index/uri .js+.d.ts + sanitized package.json); `npm run test -w @nimbus/mongodb` → green; `git check-ignore` confirms dist untracked | Toolchain finding (evidence): tsc 6.0.3 supports `--rewriteRelativeImportExtensions`; **JS** emit rewrites `./uri.ts`→`./uri.js`, but **declaration** emit keeps `.ts`, so the builder post-processes `dist/**/*.d.ts` specifiers `.ts`→`.js`. Shared dependency-closed builder established (tsc per-file js+dts + sanitized manifest). DONE + verified (build+selftest green via npm): `@nimbus/mongodb` (drops vestigial `mongodb`), `@nimbus/dynamodb` (keeps optional AWS-SDK peer), `nimbus` (5 exports, react/react-dom peers kept), `convex` (4 exports + `.d.ts`; drops `@nimbus/codegen`+`esbuild`, keeps `nimbus`; preserves the demo browser-IIFE + `demos/convex/vendor` copy; source package untouched so the in-repo `nimbus-ui` codegen chain is intact). Verifier advanced **4→6 passed** (conds 6 dependency-closed manifests + 24 convex multi-entry dist flipped). `@nimbus/firebase` also built+verified (5/6 packages done) via the same per-file builder, **co-provisioning** `@connectrpc/*`+`@bufbuild/protobuf` as additional roots (evidence-based disposition; plan firebase bullet updated with rationale + G4 attribution implication). REMAINING in BPD1: `codegen` embed/provisioning is a BPD4 sub-problem because `@nimbus/codegen` imports `typescript` and can import `esbuild`; BPD4 must either close those tooling dependencies inside the embedded V8 tooling profile or route explicitly out-of-contract paths to external Node; `rust-embed` + checksummed embedded `{package,version}` manifest incl. the firebase third-party roots + G4 attribution (cond 5); Makefile + release/coverage/apt-repo/linux-packages artifact wiring (cond 7); focused JS+Rust tests. |
| 2026-05-31 | BPD1/BPD6 | in_progress | `docs/plans/binary-embedded-package-distribution-plan.md` | `npm run docs:validate-refs:strict` → pass (241 working-tree Markdown files); `git diff --check` → clean | Control-plane correction from review: Condition 27 decision recorded as self-hosted binary distribution for current `packages/*`, with managed-cloud SDK distribution deferred to a separate future channel. Firebase closure made canonical: co-provision `@connectrpc/*` + `@bufbuild/protobuf` as embedded third-party roots, or revise the plan and bundle/drop them; BPD1 cannot be marked done until staging, embedding, checksums, provisioning, attribution, and verifier coverage prove that closure. Condition 6/23/27 criteria tightened so unsupported registry dependencies cannot pass by omission. |
| 2026-05-31 | BPD1 | in_progress | `crates/nimbus-bin/{Cargo.toml,build.rs,src/main.rs,src/embedded_packages.rs (new)}`; `scripts/{stage-embedded-packages,check-package-closure}.mjs` (new); `scripts/verify-binary-embedded-package-distribution.sh`; `Makefile`; `.gitignore`; `packages/{firebase,dynamodb,nimbus,convex}/package.json`; `docs/private/managed-service-launch-plan.md` | `cargo test -p nimbus-bin embedded_packages` → **4 passed** (manifest, version-lock, integrity, tamper +/-); `node scripts/check-package-closure.mjs` → **OK (5 Nimbus + 3 third-party roots, all deps closed)**; `make build-packages` → builds 5 pkgs + stages 8-pkg/717-file checksummed manifest; `cargo build -p nimbus-bin` → ok; verifier → **7 passed, 20 failed**; docs-refs pass; `git diff --check` clean | Embed keystone (cond 5): `rust-embed =8.11.0` embeds the staged `embedded-packages/` tree via `src/embedded_packages.rs`; checksummed `{package,version,sha256}` manifest version-locked to source; Makefile `EMBEDDED_PKG` graph stages before cargo (build/check/clippy/test/release prereq) + `build.rs` actionable-error assertion (mirrors `nimbus-ui/dist`). **Firebase closure (cond 6) made real, not documented:** extended staging to embed the 3 zero-dep third-party roots (`@bufbuild/protobuf`, `@connectrpc/connect`, `@connectrpc/connect-web`); hardened C6 into a per-package closure PROOF (`check-package-closure.mjs`) that fails if any provisioned/co-provisioned `dependencies` entry is not an embedded root (peers must be embedded or in the documented developer-supplied allowlist react/react-dom/@aws-sdk). Integrity-guard: caught + fixed 2 spurious verifier flips (C12 doc-comment match; C21 → requires BPD7 proof). **C27 (branch a):** updated `managed-service-launch-plan.md` SDK item to defer managed-cloud SDK publishing to a separate future channel (not deleted). **BPD4** ledger initially gained an over-broad `esbuild` line; later BPD4 correction narrows it to the tooling-profile grant boundary. **Third-party attribution — DONE + proven:** the 3 roots ship no LICENSE file but their embedded `dist/*.js` retain per-file Apache-2.0 SPDX/copyright headers (`Buf Technologies, Inc.` / `The Connect Authors`); added auditable `NOTICE` entries (Apache-2.0, with the BSD-3-Clause note for protobuf-es); `check-package-closure.mjs` now also asserts every embedded third-party root is attributed in `NOTICE` (negative-tested: removing an entry fails the check). **REMAINING BPD1 (stays in_progress):** CI artifact wiring for cond 7 (`release`/`coverage`/`apt-repo`/`linux-packages` must stage + carry `embedded-packages/` like the `nimbus-ui` artifacts); `codegen` in-binary embed (per the BPD4 tooling-profile esbuild line). Worktree hygiene: `node-default-runtime-support-hardening-plan.md` churn is a parallel agent's work — excluded from BPD scope, do not commit with BPD. |
| 2026-05-31 | BPD1 | in_progress | `.github/workflows/release.yml`, `.github/workflows/coverage.yml` | `actionlint` → clean (both); `make build` (build-ui → build-packages → `cargo build --workspace`) → **Finished, ok**; verifier → **8 passed, 19 failed** (cond 7 flipped); docs-refs pass; `git diff --check` clean | **CI/release artifact wiring (cond 7) done.** rust-embed requires `embedded-packages/` at compile, so the node-less release/coverage compile jobs would otherwise fail — wired them like the `nimbus-ui` artifact: leader jobs build+stage + upload an `embedded-packages` artifact; each `nimbus-bin` compile job (release: `build-linux-arm64` + matrix `build`; coverage: `warm-sccache` + `coverage` shard + `coverage-reduce`) downloads + mtime-refreshes it before cargo. `apt-repo`/`linux-packages` consume prebuilt tarballs (no compile) → out of scope. Full-graph `make build` proves the embed + Makefile wiring is sound. **Remaining BPD1:** the `codegen` in-binary embed is coupled to the BPD4 staged-tooling dependency closure (codegen runs in V8 tooling and is not provisioned to apps) — scoped to BPD4. NOTE: cond 23 (offline boundaries proven in-contract) is shared with BPD7 and cannot pass until the BPD7 no-network proof exists, so BPD1 stays `in_progress` pending that forward-reference. CI run is the only un-local-verifiable piece (validated statically with actionlint; true proof needs a push). |
| 2026-05-31 | BPD2 | in_progress | `crates/nimbus-bin/src/provision.rs` (new); `crates/nimbus-bin/src/{main.rs,embedded_packages.rs}`; `scripts/verify-binary-embedded-package-distribution.sh` | `cargo test -p nimbus-bin provision` → **8 passed** (provision-all+stamp, idempotent no-op, adapter dependency-closure, corrupt-partial rewrite, drift re-provision + match no-op, empty no-op); CLI smoke: `nimbus packages provision all` → 8 pkgs + stamp; re-run → "up to date"; `provision firebase` → firebase + 3 closure roots only; verifier → **12 passed, 15 failed**; docs-refs pass; `git diff --check` clean | **BPD2 core done.** New `provision.rs`: `provision_packages(app_dir, Selection)` materializes the embedded payload into `<app>/.nimbus/packages/<dir>/` with a `.version` stamp (= embedded manifest digest), idempotent (no-op on stamp-match + dirs present) and atomic (stamp written last). Transitive dependency `closure()` so `provision firebase` pulls its 3 third-party roots but not unrelated adapters. `reconcile()` re-provisions on stamp drift. Explicit `nimbus packages provision <all|adapter>` CLI wired into `Command::Packages`. Flipped conds 12 (provision+stamp), 13 (explicit command), 18 (reconcile-on-drift), 19 (adapter-on-request). Integrity-guard: corrected C13 (false-negative — probe looked for naming I didn't use; now detects `enum PackagesCommand` + `Command::Packages`). **Remaining BPD2 (forward-coupled):** cond 25 (committed-lockfile policy + clone-then-install proof) lands with the BPD3 scaffold (recommended policy: scaffold gitignores `package-lock.json`; `nimbus dev` provisions-before-install) and the BPD7 no-network proof; cond 26 (reconcile forces Node dependency reinstall) is the BPD5 dev/codegen wiring. `reconcile()` carries `#[allow(dead_code)]` until BPD5 calls it. |
| 2026-05-31 | BPD3 | in_progress | `crates/nimbus-bin/templates/convex/package.json.tmpl`, `crates/nimbus-bin/templates/cloud-functions/functions/package.json.tmpl`, `crates/nimbus-bin/src/{init.rs,node.rs}`, `scripts/build-js-package.mjs`, `docs/adapters/cloud-functions/README.md` | `cargo test -p nimbus-bin init::` → **25 passed**; `node::tests` → **22 passed**; closure proof OK; verifier → **16 passed, 11 failed**; docs-refs pass; `git diff --check` clean | **BPD3 scaffold retemplate done (conds 8/9/10/11).** Convex template now `"convex": "file:./.nimbus/packages/convex"`, `@nimbus/codegen` dropped (codegen in-binary); CF template drops `@nimbus/codegen` and is documented as the **external-Node / preinstall fallback** (its `firebase-admin`/`firebase-functions` are developer-supplied registry deps, outside the no-network claim — cond 11). **Offline-closure fix:** the builder now rewrites kept inter-package deps to relative `file:` siblings (`convex`→`file:../nimbus`; `firebase`→`file:../@connectrpc/*`+`@bufbuild`), so an offline `npm install` of the scaffold never reaches the registry; closure proof still passes. `node.rs` `#[cfg(test)]` fixtures rewritten from `^1.0.0` to `file:` (cond 10; 0 `^1.0.0` remain); init.rs tests assert `file:` + absence of `@nimbus/codegen`/registry ranges. **Remaining BPD3:** cond 25 (committed-lockfile policy + clone-then-install proof) is genuinely coupled to BPD5 (`nimbus dev` provision-before-install ordering) + BPD7 (no-network proof) — recommended policy recorded (commit lockfile; `nimbus dev`/`nimbus packages provision` runs before `npm ci`); optional tsconfig path-mapping fallback deferred (file: + npm install is the primary resolution). |
| 2026-05-31 | BPD4 | in_progress | `packages/codegen/src/auth_config.mjs`, `packages/codegen/src/cloud_functions/bundle.mjs` | `npm run test --workspace @nimbus/codegen` → exit 0; `npm run build -w convex` → exit 0 (codegen pipeline still works) | **BPD4 decision corrected after runtime audit.** Evidence: `esbuild` is imported in exactly two codegen paths — `auth_config.mjs` (auth-config bundling) and `cloud_functions/bundle.mjs` (CF) — while the core `runtime_bundle.mjs` is pure-JS. Additional evidence: the embedded codegen path uses `RuntimeLimits::tooling_node22()`, tooling grants include `$discovered_tooling`, and `cargo test -p nimbus-runtime tooling_node22_executes_esbuild_style_staged_binary` proves an esbuild-style staged package binary executes in the tooling profile. Decision: in-binary V8 codegen is the default for every in-contract Convex path; any in-contract path that needs `esbuild` must stage/embed `typescript`, `esbuild`, and the matching platform `@esbuild/*` binary under the tooling-profile closure and prove execution there. External Node is NOT the supported path for Nimbus-owned codegen — a retained `ExternalNode` is classified **diagnostic/transition-only** (never documented as supported, never counted as BPD offline/in-binary proof); verifier cond 15 was reframed to this gate. **Step done this turn:** made `esbuild` a **lazy `await import`** in both files so the codegen module loads before the esbuild path is invoked (verified: codegen selftest + convex build green). Added `packages/codegen/build.mjs` + a `build` script: esbuild prebundles codegen → pure-JS `dist/codegen.bundle.mjs` (typescript inlined, `esbuild` external, with `createRequire`/`__filename`/`__dirname` shims for the bundled CJS typescript); verified the bundle loads under node and exposes `generateConvexArtifacts`/`runCliFromArgs` (9.7M, gitignored). Per the corrected decision, the runner-flip must additionally stage `esbuild` + the platform `@esbuild/*` binary under the tooling-profile closure (not leave esbuild external) for in-contract paths. **Remaining BPD4 (flips conds 14/16):** embed `@nimbus/codegen` plus required tooling deps in the staged payload; flip the runner default to in-binary sourcing the codegen package from the embedded payload (not `node_modules/@nimbus/codegen`); route only explicitly out-of-contract/preinstall paths to external Node; retire/repurpose `NIMBUS_EXPERIMENTAL_EMBEDDED_CODEGEN`; update `cli.md`/`compatibility.md` so `convex codegen`/`nimbus-codegen` delegate to the binary. |
| 2026-05-31 | BPD4 | in_progress | `docs/plans/binary-embedded-package-distribution-plan.md` | `cargo test -p nimbus-runtime tooling_node22_executes_esbuild_style_staged_binary` → **1 passed**; docs-refs and `git diff --check` must be rerun after this checkpoint | Control-plane correction: external Node is not a BPD success path for Nimbus-owned codegen. For code we own, fix the embedded tooling path. BPD4 must stage/embed the required `@nimbus/codegen` tooling closure (`typescript`, `esbuild`, matching platform `@esbuild/*` binary) and prove it under `RuntimeLimits::tooling_node22()` / `$discovered_tooling`. If an external-Node runner remains, it is diagnostic/transition-only and must not be documented or counted as offline/in-binary proof. Supersedes earlier BPD4 log wording that framed external Node as a fallback solution. |
| 2026-05-31 | BPD4 | in_progress | `docs/plans/binary-embedded-package-distribution-plan.md` | `npm run docs:validate-refs:strict` → pass (241 working-tree Markdown files); `git diff --check` → clean | Post-correction lightweight verification completed for the BPD4 control-plane wording. |
| 2026-05-31 | BPD4+BPD6 | in_progress | `scripts/stage-embedded-packages.mjs`, `Makefile`, `packages/codegen/{build.mjs,src/auth_config.mjs,src/cloud_functions/bundle.mjs}`, `scripts/verify-binary-embedded-package-distribution.sh`, `docs/adapters/{firebase/README.md,firebase/migration.md,mongodb/README.md,mongodb/drivers.md}`, `docs/private/managed-service-launch-plan.md` | `make build-packages` → stages 8 pkgs + tooling closure; `cargo test -p nimbus-bin -- embedded_packages:: provision::` → **10 passed**; closure proof OK; verifier → **17 passed, 10 failed**; docs-refs pass; `git diff --check` clean | **Correction applied + tooling closure staged.** Replaced the wrong "esbuild-cannot-run-in-V8 / external-Node fallback" wording in `auth_config.mjs`, `cloud_functions/bundle.mjs`, `build.mjs`. Reframed verifier **cond 15** to the new gate (external Node not the supported runtime; retained `ExternalNode` is diagnostic/transition-only) — it now honestly FAILS pending the runner-flip (18→17). **Staged the complete tooling closure** (`.tooling/`): codegen prebundle (typescript inlined) + `esbuild` JS + host `@esbuild/<platform>` native binary, checksummed in `manifest.tooling` (Makefile now builds `@nimbus/codegen` before staging). Closure proof unaffected (tooling ≠ app packages). **BPD6 cond 20 + 27 flipped:** rewrote the 4 adapter-doc `npm install @nimbus/*` instructions to `nimbus packages provision <adapter>` (accurate now that the command exists), and reworded the managed-service launch-plan SDK item so it no longer reads as an active publish-to-npm step (C27 self-hosted decision recorded). **Remaining BPD4 (flips 14/15/16):** the runner-flip — codegen.rs sources the embedded codegen bundle + runs it in the `tooling_node22` V8 runtime resolving the staged `@esbuild` tooling binary; reclassify `ExternalNode` diagnostic-only; retire `NIMBUS_EXPERIMENTAL_EMBEDDED_CODEGEN`; prove auth-config bundling in V8; update `cli.md`/`compatibility.md` codegen delegation. **Runner-flip foundation built + tested:** `embedded_packages::materialize_tooling(dest)` lays the embedded closure out exactly as the proof test's layout — codegen prebundle at `<dest>/codegen.bundle.mjs`, `esbuild` + platform `@esbuild/<platform>` under `<dest>/node_modules/`, native binary chmod +x — returning the bundle path for use as `codegenSpecifier` (`cargo test -p nimbus-bin embedded_packages::` → 5 passed incl. the new materializer test). Precise remaining wiring in `run_embedded_codegen_for_app_dir`: call `materialize_tooling(temp)`, set `codegenSpecifier` to the temp bundle, pass the app dir as an **absolute** `--app` (so codegen reads the app while `import("esbuild")` resolves module-relative from `temp/node_modules`), drop `ensure_embedded_codegen_package_available`, flip `resolve_codegen_runner` default to in-binary, reclassify `ExternalNode`, retire the env flag. The one empirical unknown to verify with a V8 codegen run: that the tooling runtime lets the temp-located bundle read/write the absolute app dir while resolving esbuild from the temp `node_modules`. |
| 2026-05-31 | BPD4 | **done** | `crates/nimbus-bin/src/{codegen.rs,embedded_packages.rs}`, `docs/adapters/convex/compatibility.md`, `scripts/verify-binary-embedded-package-distribution.sh` | `cargo test -p nimbus-bin codegen::tests` → **9 passed** incl. `embedded_codegen_generates_convex_artifacts_without_app_node_modules`; verifier → **20 passed, 7 failed** (conds 14/15/16 flipped); docs-refs pass; `git diff --check` clean | **BPD4 runner-flip COMPLETE + empirically proven.** `run_embedded_codegen_for_app_dir` now materializes the embedded tooling closure into `<app>/.nimbus/tmp/` (an allowed runtime read root — a `/tmp` dir was capability-denied, settled empirically) and runs codegen in the `tooling_node22` V8 runtime with an absolute `--app`; `import("esbuild")` resolves the staged `node_modules/esbuild` + spawns the staged native `@esbuild` binary. A new test proves codegen produces `functions.json` + `bundle.mjs` + `_generated/api.ts` with **no app `node_modules/@nimbus/codegen`** (cond 14). Flipped the default to in-binary, retired `NIMBUS_EXPERIMENTAL_EMBEDDED_CODEGEN` → `NIMBUS_CODEGEN_RUNNER` (default in-binary; `external-node` is the opt-in **diagnostic/transition-only** runner, cond 15), and documented in-binary delegation in `compatibility.md` (cond 16). Removed the now-obsolete `ensure_embedded_codegen_package_available` + the workspace-staging test helpers. **Remaining:** BPD5 (cond 26 + clone-then-install half of 25 — wire `reconcile()` into dev/codegen + force reinstall), BPD7 (conds 17/21/23/25 — no-network end-to-end proof), BPD8 (conds 1/22 — closeout + `BPD_FULL` fmt/clippy). NOTE: `docs/operating/cli.md` still describes external-`node`-default codegen — stale after the flip; fix in the BPD6/BPD7 doc sweep. |
| 2026-05-31 | BPD5 | in_progress | `crates/nimbus-bin/src/{dev.rs,provision.rs,node.rs}` | `cargo test -p nimbus-bin 'provision::'` → **7 passed** incl. `reconcile_on_drift_forces_node_reinstall`; verifier → **21 passed, 6 failed** (cond 26 flipped); docs-refs pass; `git diff --check` clean | **BPD5 reconcile→reinstall wiring done (cond 26).** Wired `provision::reconcile()` into `run_dev_command` (`dev.rs`) ahead of the Node dependency install loop, so a binary upgrade (stale `.nimbus/packages/.version` stamp) re-provisions the embedded payload before install. On re-provision, new `provision::force_node_reinstall` removes the installed `node_modules/<name>` copy of every provisioned package and calls new `node::clear_node_dependency_state` to delete the `.nimbus/cache/node/dependency-state.json` fingerprint — the `file:` specifiers leave `package.json`/lockfile unchanged so the install would otherwise Skip and keep stale copies. New test `reconcile_on_drift_forces_node_reinstall` drives provision → simulate prior install (node_modules copy + state file) → stamp drift → `reconcile` and asserts both the stale copy and the fingerprint are removed (forcing a clean reinstall). `reconcile()`'s `#[allow(dead_code)]` removed now that `dev` calls it. **Remaining BPD5 (forward-coupled):** cond 25 (committed-lockfile policy + clone-then-install proof) lands with the BPD7 proof dir; deploy/codegen reconcile call sites are covered transitively by `dev`'s preflight, but the BPD7 offline proof will exercise the full drift→reinstall sequence end-to-end. |
| 2026-05-31 | BPD7 | done | `crates/nimbus-bin/src/{provision.rs,node.rs,dev.rs,init.rs,codegen.rs,deploy.rs}`; `packages/{convex,nimbus}/package.json`; `packages/codegen/src/auth_config.mjs`; `scripts/{stage-embedded-packages,check-package-closure}.mjs`; `docs/plans/proof/binary-embedded-package-distribution/bpd7-offline-integrity.md` (new); `docs/plans/binary-embedded-package-distribution-plan.md` | Real no-network e2e (`NPM_CONFIG_REGISTRY=http://127.0.0.1:1` + `npm install --offline`): init→provision→install→in-binary codegen→`dev` serve→query/mutate/query all 200; `bundle.sha256` MATCH; `nimbus packages verify` 717 files ✓ + tamper → exit 1; firebase/mongodb/dynamodb offline-installed+verified; clone-then-install dangling→resolved; `cargo test -p nimbus-bin` → **568 passed, 0 failed**; closure OK; verifier → **25 passed, 2 failed** (17/21/23/25 flipped; only closeout 1+22 remain); fmt/git-diff/docs-refs clean | **BPD7 offline + integrity proven end-to-end.** Added `nimbus packages verify` (`verify_provisioned` re-hashes provisioned bytes vs the embedded manifest; subset-aware; tamper → non-zero exit) + `node::clear_node_dependency_state`. **Three real defects found + fixed while proving offline:** (1) `convex`/`nimbus` declared **non-optional** react/react-dom peers → offline `npm install` tried to fetch them; marked `peerDependenciesMeta.optional` (matches upstream Convex). (2) staged `@bufbuild/protobuf` carried `devDependencies: upstream-protobuf` → npm installs devDeps of `file:` links → offline fetch; staging now strips `devDependencies`+`scripts` from third-party manifests and `check-package-closure.mjs` fails if any survive. (3) the supported flow needed a *manual* provision — `init`/`dev` did not provision when `.nimbus/packages` was absent; replaced drift-only `reconcile` with `provision::ensure` (provision-if-absent + drift) and wired it into `init` (after scaffold) and `dev` (before install) + `Adapter::provision_target`. **Significant runtime finding:** in-binary esbuild bundling does NOT work in the V8 tooling runtime — both esbuild IPC paths fail there (async service → `child_process` `unref` on undefined handle; `buildSync` worker-thread → message-port `could not deserialize value`). This affects **Convex auth-config** and **Cloud Functions** (the two esbuild-using codegen paths). First-pass resolution routed *both* to external Node; this was corrected in the BPD7-fix entry below so that **auth-config stays in-contract** (evaluated in-binary by the AST interpreter, no esbuild) and **only Cloud Functions** is out-of-contract. See that entry for the final contract. |
| 2026-05-31 | BPD7-fix | done | `packages/codegen/src/{auth_config.mjs,compile_time_interpreter.mjs}`, `crates/nimbus-bin/src/codegen.rs`, `scripts/verify-binary-embedded-package-distribution.sh`, `docs/operating/cli.md`, `docs/adapters/convex/compatibility.md`, `packages/codegen/README.md`, `docs/plans/proof/binary-embedded-package-distribution/bpd7-offline-integrity.md`, `docs/plans/binary-embedded-package-distribution-plan.md` | `@nimbus/codegen` selftest → **219 passed, 0 failed**; `cargo test -p nimbus-bin` → **568 passed, 0 failed** (incl. CF preflight + `embedded_pilot_rejects_cloud_functions_layout_with_clear_message`); default-runner `nimbus codegen` on an `auth.config.ts` app (env + const + OIDC + customJwt) with registry unreachable → exit 0, in-binary (no external-node notice), `auth.config.json` emitted; verifier c15 adversarially proven (FAILs if the plan carves auth.config out, or if codegen.rs re-adds auth-config auto-routing) | **Contract made internally consistent (Codex review).** The first-pass "auto-route auth-config to external Node" approach conflicted with "ExternalNode is diagnostic/transition-only." Corrected to the **preferred** contract: the entire default Convex surface — schema, server, http, **auth.config** — is in-binary/offline; **only Cloud Functions** is out-of-contract. **auth.config now runs in-binary** via a new `evaluateModuleDefaultExport` in `compile_time_interpreter.mjs` (statically evaluates the module's `export default` — literals, hoisted `const`s, template strings, `process.env.*` via an opt-in interpreter global — using the same TS AST interpreter as schema/server; **no esbuild, no dynamic import**); removed esbuild from `auth_config.mjs`. **codegen.rs:** removed `app_has_auth_config`/`requires_external_node_runner`; the default runner auto-selects external Node **only** for `is_cloud_functions_app`; `ExternalNode` enum doc rewritten to two explicit roles (supported runner for out-of-contract CF; diagnostic/transition-only opt-out for the in-contract Convex surface). **ExternalNode classification is now consistent:** CF uses it as the *supported* path (CF is explicitly out-of-contract), and it is diagnostic-only for Convex. **Verifier c15 hardened** to fail on the contradiction (if the default path auto-routes to ExternalNode, the plan must classify CF out-of-contract + "not a diagnostic fallback" + keep "auth.config stays in-contract", and codegen.rs must not auto-route auth-config) — adversarially verified to FAIL on reintroduced contradictions. **Docs corrected:** `cli.md` (no @nimbus/codegen in scaffolds; no `npx convex codegen`/`npx nimbus-codegen` equivalence; whole Convex surface in-binary), `compatibility.md`, and `packages/codegen/README.md` (private/embedded; not `npm install`-ed). |
| 2026-05-31 | Closeout state (SUPERSEDED — see BPD8 row below) | interim snapshot | `docs/plans/binary-embedded-package-distribution-plan.md` | (interim) `BPD_FULL=1` verifier → **26 passed, 1 failed** (only c1, the closeout gate) | **SUPERSEDED by the BPD8 `done` row below.** This row captured the moment when BPD1–BPD7 were `done` but BPD8 (closeout/archive) had been intentionally deferred by developer instruction, so the verifier was 26/1 by design. The developer subsequently approved closeout: BPD8 is now `done`, the plan **is** archived to `docs/plans/archive/`, `docs/plans/README.md` is updated, and the verifier is **27 passed, 0 failed**. Read this row as history only — the "BPD8 left pending / read 26/1 as closeout pending" wording no longer reflects reality. |
| 2026-05-31 | BPD8 | done | `docs/plans/binary-embedded-package-distribution-plan.md` → `docs/plans/archive/` (plain `mv`; the plan was an uncommitted working-tree file, so `git mv` does not apply); `docs/plans/README.md`; `docs/adapters/cloud-functions/README.md`; `docs/plans/proof/binary-embedded-package-distribution/bpd8-closeout.md` (new) | `BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh` → **27 passed, 0 failed** (verifier resolves the archived plan via its `PLAN_ARCHIVED` fallback; c22 runs `cargo fmt --all --check` + `make clippy` + `npm run docs:validate-refs:strict` + `git diff --check` inline, all clean); `cargo test -p nimbus-bin` → 568/0; `@nimbus/codegen` selftest → exit 0 | **Closeout + archive (final state).** All ledger rows BPD0–BPD8 `done`. Plan `mv`-ed to `docs/plans/archive/`; status header flipped to `archived 2026-05-31`; `docs/plans/README.md` brief moved into the `## Current Reference Baselines` archived list pointing at the archived path; `docs/adapters/cloud-functions/README.md` routing reference + the `docs/private/managed-service-launch-plan.md` BPD link repointed to the archived path; the proof dir is intentionally NOT moved so the verifier's `PROOF_DIR` still resolves (c2/c17/c21/c23 still pass). Verifier `PLAN_ACTIVE` → `PLAN_ARCHIVED` fallback confirmed resolving the moved plan. Closeout proof: `docs/plans/proof/binary-embedded-package-distribution/bpd8-closeout.md`. **Known follow-ups (out of BPD scope):** the `release.yml`/`coverage.yml` `embedded-packages` artifact wiring is `actionlint`-clean + proven by a full-graph local `make build` but not yet validated on real GitHub runners (needs a push); lifting Cloud Functions into the in-binary contract needs a `nimbus/deno` `child_process`/`worker_threads` IPC fix (separate plan). Per the standing rule nothing is staged/committed. |
| 2026-05-31 | Audit | done | `Makefile`; `.github/workflows/ci.yml`; `scripts/verify-binary-embedded-package-distribution.sh` | `BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh` → **27 passed, 0 failed**; `cargo test -p nimbus-bin` → 568/0; `actionlint ci.yml coverage.yml release.yml` → clean; `make -n test-rust-workspace` (manifest hidden) now stages packages before cargo; fmt/git-diff/docs-refs clean | **Post-closeout full code review + audit (4 parallel review agents + self-verification).** Fixed one real **CI blocker** the prior rounds missed: the merge-gating `ci.yml` `rust-workspace-tests` job runs `make test-rust-workspace`/`make test-rust-docs`, but those Makefile targets lacked the `$(EMBEDDED_PKG_MANIFEST)` prerequisite that `check`/`clippy`/`test` got — so they'd compile `nimbus-bin` without staging and panic in `build.rs` on a fresh checkout. Added the prerequisite to both targets (parallel to the others). Also wired `ci.yml`'s `ui-artifacts` leader to stage+upload the `embedded-packages` artifact and its `warm-sccache` job (`cargo check --workspace`, no Node toolchain) to download+mtime-refresh it, mirroring `coverage.yml`. **Hardened six verifier conditions** that under-verified (found by the adversarial-completeness agent): c24 (was a filename-count heuristic that ignored `values`/`browser`; now asserts the convex dist manifest declares all four exports + each target file exists); c19 (matched only a doc-comment + test code; now anchors to the live `Selection::Adapter` + `fn closure` adapter-on-request wiring); c25 (was a proof-keyword grep only; added a code half requiring `provision::ensure` wired into `init` + `dev`); c9/c10/c14 (pure-`absent` gates that passed vacuously if their target file were deleted; added existence preconditions, and broadened c10's node.rs check from the hyper-specific `^1.0.0` to any `^X.Y.Z` while leaving the CF template's legitimate developer-supplied Firebase/typescript caret ranges alone). Each hardened condition adversarially re-proven: PASS on the real tree, FAIL when its underlying feature/file is removed (mutate→verify→restore, all files byte-restored). Two agents independently confirmed the prior round's Rust + doc fixes were correct and complete. |
| 2026-05-31 | Audit-fix | done | `crates/nimbus-bin/src/{provision.rs,embedded_packages.rs,codegen.rs,deploy.rs}`; `scripts/{stage-embedded-packages.mjs,verify-binary-embedded-package-distribution.sh}`; `.github/workflows/release.yml`; `Makefile`; `package.json`; `docs/operating/cli.md`; `docs/adapters/cloud-functions/README.md`; `packages/codegen/README.md`; `docs/plans/{README.md,archive/binary-embedded-package-distribution-plan.md}`; `docs/plans/proof/binary-embedded-package-distribution/bpd8-closeout.md`; `NOTICE` | `npm run build:embedded-packages` → staged 8 packages / 717 files; `NIMBUS_EMBEDDED_ESBUILD_PLATFORM=darwin-arm64 node scripts/stage-embedded-packages.mjs` → staged 8 packages / 717 files; `node scripts/check-package-closure.mjs` → OK; `cargo test -p nimbus-bin provision::tests` → 12/0; `cargo test -p nimbus-bin embedded_packages::tests` → 5/0; `cargo test -p nimbus-bin codegen::tests` → 6/0; `cargo test -p nimbus-bin` → 568 unit + 2 integration tests passed; `cargo run -p nimbus-bin -- packages verify --app-dir /private/tmp/nonexistent-bpd-review-confirm` → exit 1 with missing `.nimbus/packages` error; `npm run test --workspace @nimbus/codegen` → exit 0; `actionlint .github/workflows/{release,ci,coverage}.yml` → clean; `npm run docs:validate-refs:strict` → pass (241 files); `cargo fmt --all --check` → clean; `git diff --check` → clean; `BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh` → **27 passed, 0 failed** | **Enterprise-trust remediation from the final audit.** Fixed seven confirmed issues: release no longer reuses a shared Linux-x64 embedded payload across target binaries; release jobs stage target-local payloads and assert `NIMBUS_EMBEDDED_ESBUILD_PLATFORM` so native `@esbuild/*` matches Linux x64, Linux arm64, macOS arm64, or Windows x64. External Node codegen no longer resolves workspace/app-installed/bare `@nimbus/codegen`; it materializes the binary-embedded tooling closure, so Cloud Functions uses external Node as a process runner without making `@nimbus/codegen` an app distribution dependency. Provisioning no longer trusts a global stamp alone: it verifies the requested closure, rewrites corrupt bytes, rejects missing/empty `packages verify` targets, and provisions known app closures from `codegen` and `deploy` as well as `init`/`dev`. Embedded tooling bytes are checksum-verified before materialization and during binary integrity verification. Root Makefile/package scripts no longer retain stale `npx convex codegen` or retired `serve` commands, and the Makefile embedded payload graph now tracks `package.json`, `package-lock.json`, `check-package-closure.mjs`, and staging/build scripts. Docs/proof/control-plane text now uses the archived plan path and `BPD_FULL=1` goal gate and describes Cloud Functions external Node as running binary-materialized embedded tooling, not app-installed codegen. |
| 2026-05-31 | Follow-up audit-fix | done | `Makefile`; `scripts/{stage-embedded-packages.mjs,check-package-closure.mjs,verify-binary-embedded-package-distribution.sh}`; `crates/nimbus-bin/src/embedded_packages.rs`; `docs/runtimes/nodejs/index.md`; `docs/plans/{archive/binary-embedded-package-distribution-plan.md,proof/binary-embedded-package-distribution/bpd8-closeout.md}` | `bash -n scripts/verify-binary-embedded-package-distribution.sh` → clean; `npm run build:embedded-packages` → staged 8 packages / 717 files; `node scripts/check-package-closure.mjs` → OK; `make -pn build-packages` contains `EMBEDDED_PKG_BUILD_SCRIPTS := packages/convex/build.mjs packages/codegen/build.mjs`; `cargo test -p nimbus-bin embedded_packages::tests -- --nocapture` → 5 passed; `npm run docs:validate-refs:strict` → pass (241 files); `git diff --check` → clean; `BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh` → **27 passed, 0 failed** | **Follow-up verifier + portability hardening.** Confirmed and fixed four non-blocking but real audit findings: the Makefile payload graph now discovers every package-level `build.mjs` (including `packages/convex/build.mjs`) instead of relying on a one-off explicit `packages/codegen/build.mjs` edge, and c7 proves that edge through Make's own database; stale user-facing `npx nimbus-codegen --app .` prose in `docs/runtimes/nodejs/index.md` is corrected to the binary-owned `nimbus codegen --app .` path, and c16 now scans `docs/runtimes`; staged runtime manifests now strip and `check-package-closure.mjs` fails on installer-active `optionalDependencies`, bundled-dependency metadata, and lifecycle `scripts`, not only `dependencies`/`devDependencies`; and the esbuild tooling materializer test now derives the native binary path from the embedded manifest (`bin/esbuild` or `esbuild.exe`) so the Windows release payload is not falsely judged by a Unix-only path assumption. |

## Risks

| Risk | Mitigation |
| --- | --- |
| Embedding all package surfaces inflates binary size. | Embed built `dist/` (esbuild-minified) rather than source trees; record the size delta in BPD1; provision adapter SDKs on demand so only used surfaces land in the app. |
| `file:` dependency resolution differs across npm/pnpm/yarn. | Treat `npm` as the supported baseline (already the `nimbus init --install` path); add a tsconfig path-mapping fallback for typecheck; prove the offline install in BPD3 and BPD7. |
| A later `npm install` prunes or overwrites provisioned packages. | Use declarative `file:` deps the package manager tracks, not raw `node_modules` writes; reconcile on every `nimbus dev`/`codegen` to repair drift. |
| Binary and provisioned package versions silently diverge. | Single source of truth is the binary's embedded version (from `build.rs`); the `.version` stamp + reconcile make drift self-healing and observable. |
| Removing the registry path breaks an existing developer expectation. | Pre-launch: no production users (per repo policy). Make the breaking change directly, delete the registry instructions, and document the binary-provisioned flow as the only path. |
| In-binary codegen default regresses a path that previously worked only through external Node. | Pre-launch: fix the embedded tooling path for Nimbus-owned codegen instead of preserving external Node as the supported answer. If an external-Node runner remains, keep it diagnostic-only and outside BPD success. |
| Switching scaffold deps to `file:` could break the release version contract. | `scripts/verify-release-version-contract.sh` (run by `release.yml:46`) asserts every intra-`packages/*` local dependency equals the release tag version. Change only the *scaffold template* specifiers to `file:`; keep the committed `packages/*/package.json` intra-workspace pins as exact versions, or update `verify-release-version-contract.sh:113-129` in lockstep. |
| In-binary V8 tooling runtime cannot run esbuild bundling. | BPD7 proved esbuild bundling does not run in the in-binary V8 tooling runtime (async-service `child_process` `unref`; `buildSync` worker message-port deserialization). Two distinct resolutions: (1) **auth.config** — instead of esbuild, evaluate it in-binary with the compile-time AST interpreter (`evaluateModuleDefaultExport`), so the whole default Convex surface stays in-binary/offline. (2) **Cloud Functions** — its runtime bundling genuinely needs esbuild plugins, so CF is a deliberate out-of-contract surface whose *supported* codegen path is the external Node.js runner (CF Firebase SDKs are developer-supplied anyway). CF apps auto-select that runner on the default path; the `ensure_embedded_codegen_layout_supported` gate is retained to reject explicit in-binary CF requests. Lifting CF in-binary needs a `nimbus/deno` `child_process`/`worker_threads` IPC fix (separate follow-up). See `## Offline contract boundaries`. |
| First-level `file:` specs hide registry-backed transitive dependencies. | BPD1 must emit sanitized, dependency-closed provisioned manifests and bundle/provision/remove every Nimbus-introduced dependency needed by BPD7 before any offline claim can pass. |
| Existing frontend-only apps have no natural `nimbus dev` trigger. | BPD2 adds an explicit package-provisioning CLI command and BPD6 rewrites adapter docs around that command instead of assuming import scanning. |
| New binary build inputs break CI or release fresh checkouts. | BPD1 updates Makefile, CI, coverage, and release build inputs together. CI/coverage mirror the `nimbus-ui` artifact contract; release stages target-local package payloads so native tooling binaries match the release target. |
| `convex` package CLI imports a removed private dependency. | BPD4 must either delegate the CLI to binary-owned codegen or remove the npm-style command from documented support and tests. |
| Sanitizing the provisioned `convex` package breaks the repo's own build. | The in-repo `nimbus-ui` `codegen` step runs `convex codegen` → `@nimbus/codegen` via workspace symlink, and `nimbus-server`'s build hard-fails without `nimbus-ui/dist`. BPD1 sanitization applies to the *provisioned* payload only; the source workspace `convex`/`@nimbus/codegen` packages and `nimbus-ui` codegen path stay intact and are exempt from `file:`/provisioning. |
| Generated-app clean checkout / CI `npm ci` fails before provisioning. | After BPD3 switches generated app manifests to `file:./.nimbus/packages/*`, `.nimbus/` stays gitignored, so those `file:` targets are absent on a clean checkout. BPD2 fixes the provision-before-install ordering and decides the committed-lockfile policy; the offline proof must include a generated-app clone-then-install case, not only the one-shot `init→install→dev` path. |
| Offline third-party drivers / server SDKs are assumed bundleable but are not. | The official `mongodb` driver graph is intentionally developer-supplied even though `@nimbus/mongodb` is provisioned helper-only; `@aws-sdk/client-dynamodb`, `firebase-admin`, and `firebase-functions` are absent from the lockfile and resist esbuild bundling; the CF template imports `firebase-functions` and declares `firebase-admin`; the package builds already mark `mongodb`/AWS SDK `external`. The Offline contract boundaries explicitly move these to developer-supplied / external-Node fallback rather than claiming them. |
| New binary build inputs miss CI lanes or the Windows matrix. | BPD1 artifact plumbing must cover every workflow that builds/consumes `nimbus-bin` — `release.yml` (incl. the `x86_64-pc-windows-msvc` matrix entry), `coverage.yml`, `apt-repo.yml`, `linux-packages.yml` — and provisioning must write/resolve `file:` specifiers correctly on Windows. Release staging must assert the target `@esbuild/*` platform instead of reusing a Linux payload across macOS/Windows/ARM binaries. |
| Demo `file:` alignment trips the release version contract. | `verify-release-version-contract.sh:118` only exempts `"*"` local deps. If BPD6 converts `demos/**` deps to `file:`, extend that exemption or leave the demos on `"*"`; `demos/mongodb/node` already pins `0.1.33` and is contract-governed. |

## References

- `crates/nimbus-bin/src/init.rs` — scaffold + version substitution
- `crates/nimbus-bin/src/codegen.rs` — `CodegenRunner` external/embedded paths
- `crates/nimbus-bin/src/node.rs` — npm bootstrap + dependency checks
- `crates/nimbus-bin/build.rs` — package-version env wiring
- `crates/nimbus-bin/templates/**` — init templates
- `crates/nimbus-bin/src/dev.rs`, `crates/nimbus-bin/src/deploy.rs` — codegen / dependency-bootstrap call sites that also need reconcile
- `crates/nimbus-server` `rust-embed` of `packages/nimbus-ui/dist/` — embedding precedent
- `packages/nimbus-ui/package.json`, `packages/convex/src/cli.mjs`, `crates/nimbus-server/build.rs` — the in-repo `convex codegen` → `@nimbus/codegen` → `nimbus-ui/dist` build chain that must stay intact
- `packages/convex/build.mjs` — current browser-only IIFE build to replace with a multi-entry dist
- `Makefile` UI dependency graph (`UI_PKG`, `UI_DIST_INDEX`) — build-input precedent
- `scripts/verify-release-version-contract.sh` — local-dep version contract (`"*"`-only exemption)
- `.github/workflows/{release,coverage,apt-repo,linux-packages}.yml` — Rust builds that consume `nimbus-bin` and need the package payload artifacts
- `docs/private/managed-service-launch-plan.md` — the SDK-publish open decision
- `docs/plans/distribution-plan.md` — parent distribution plan
