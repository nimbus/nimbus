# BPD8 — Closeout and archive

Final state of the Binary-Embedded Package Distribution plan. All ledger rows
BPD0–BPD8 are `done`; the contract is internally consistent; the full gate is
green.

## Final verifier (full gate)

```
BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh
→ 27 passed, 0 failed
```

Condition 22 (run only under `BPD_FULL=1`) executes the heavy toolchain checks
inline: `cargo fmt --all --check`, `make clippy`, `npm run docs:validate-refs:strict`,
and `git diff --check` — all clean. Condition 1 (every ledger row `done` at
closeout) passes now that BPD8 is `done`. The verifier resolves the plan through
its `PLAN_ARCHIVED` fallback after the archival move below.

## Supporting verification

- `cargo test -p nimbus-bin` → 568 passed, 0 failed (incl. `provision::*`,
  `codegen::*`, both Cloud Functions preflight tests, and
  `embedded_pilot_rejects_cloud_functions_layout_with_clear_message`).
- `@nimbus/codegen` selftest → exit 0 (includes the auth-config suite proving
  in-binary evaluation of `auth.config.{ts,js}`).
- `node scripts/check-package-closure.mjs` → OK (5 Nimbus + 3 co-provisioned
  third-party roots, all dependencies closed).
- `make deny` → advisories/bans/licenses/sources ok.

## Contract (final, internally consistent)

The whole default Convex authoring surface — schema, server, http, and
`auth.config.{ts,js}` — runs codegen **in-binary and offline**. `auth.config` is
evaluated by the compile-time TypeScript AST interpreter
(`compile_time_interpreter.mjs` `evaluateModuleDefaultExport`), not esbuild.
**Cloud Functions is the one out-of-contract surface**: its codegen runs on the
external Node.js runner as the *supported* path for that surface (its runtime
bundling needs esbuild plugins; its Firebase SDKs are developer-supplied). The
external runner is otherwise only a `diagnostic/transition-only` opt-out for the
in-contract Convex surface via `NIMBUS_CODEGEN_RUNNER=external-node`. Verifier
condition 15 enforces this consistency and was adversarially proven to fail if
the contract is reintroduced as contradictory. Verifier condition 16 was
hardened with a paragraph-aware negative gate that fails on any stale
`npx convex codegen` / `convex codegen --app` / `nimbus-codegen --app`
instruction in user docs (negative disclaimers allowed).

## Archival

- `mv` the plan into `docs/plans/archive/` (it was an uncommitted working-tree
  file, so a plain `mv`, not `git mv`).
- `docs/plans/README.md`: brief moved into the `## Current Reference Baselines`
  archived list, pointing at the archived path; status `archived 2026-05-31`.
- `docs/adapters/cloud-functions/README.md` and the BPD0/BPD7 proof files:
  routing references repointed to the archived path.
- The proof directory `docs/plans/proof/binary-embedded-package-distribution/`
  is intentionally **not** moved (proof artifacts stay where the verifier's
  `PROOF_DIR` resolves them; conditions 2/17/21/23 still pass).

## Post-Closeout Audit Remediation

The final audit found and fixed seven issues after initial closeout:

- release builds now stage the embedded package payload target-locally and set
  `NIMBUS_EMBEDDED_ESBUILD_PLATFORM` so native `@esbuild/*` matches each release
  target, rather than reusing a shared Linux payload;
- the external Node codegen runner materializes the binary-embedded tooling
  closure, so Cloud Functions uses external Node as a process runner without
  requiring an app-installed `@nimbus/codegen`;
- provisioning verifies the requested closure, rewrites corrupt provisioned
  bytes even when the stamp matches, rejects missing/empty `packages verify`
  roots, and runs from `codegen`/`deploy` as well as `init`/`dev`;
- embedded tooling files are checksum-verified before materialization and during
  binary integrity verification;
- root Makefile/package scripts no longer retain stale `npx convex codegen` or
  retired `serve` commands, and the Makefile embedded payload graph tracks
  `package.json`, `package-lock.json`, closure, build, and staging scripts;
- docs/control-plane text uses the archived plan path and the full
  `BPD_FULL=1` gate;
- Cloud Functions docs now state that external Node runs the
  binary-materialized embedded codegen bundle, not an app-installed package.

Verification for the remediation:

- `npm run build:embedded-packages` -> staged 8 packages / 717 files.
- `NIMBUS_EMBEDDED_ESBUILD_PLATFORM=darwin-arm64 node scripts/stage-embedded-packages.mjs`
  -> staged 8 packages / 717 files.
- `node scripts/check-package-closure.mjs` -> OK.
- `cargo test -p nimbus-bin provision::tests -- --nocapture` -> 12 passed.
- `cargo test -p nimbus-bin embedded_packages::tests -- --nocapture` -> 5 passed.
- `cargo test -p nimbus-bin codegen::tests -- --nocapture` -> 6 passed.
- `cargo test -p nimbus-bin` -> 568 unit tests and 2 integration tests passed.
- `cargo run -p nimbus-bin -- packages verify --app-dir /private/tmp/nonexistent-bpd-review-confirm`
  -> exit 1 with the expected missing `.nimbus/packages` error.
- `npm run test --workspace @nimbus/codegen` -> exit 0.
- `actionlint .github/workflows/release.yml .github/workflows/ci.yml .github/workflows/coverage.yml`
  -> clean.
- `BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh`
  -> 27 passed, 0 failed.

## Follow-Up Audit Remediation

A follow-up audit confirmed and fixed four additional guardrail/portability
issues:

- the Makefile embedded-payload graph now discovers package-level `build.mjs`
  files dynamically, including `packages/convex/build.mjs`, and c7 proves that
  input through Make's own database;
- `docs/runtimes/nodejs/index.md` no longer tells users to run
  `npx nimbus-codegen --app .`; c16 now includes `docs/runtimes` in the
  user-facing stale-codegen sweep;
- staged runtime manifests strip installer-active `optionalDependencies`,
  bundled-dependency metadata, and lifecycle `scripts`, and
  `check-package-closure.mjs` fails if any of those fields survive staging;
- the esbuild tooling materializer test derives the native binary path from the
  embedded platform manifest (`bin/esbuild` or `esbuild.exe`) instead of
  assuming a Unix-only path.

Verification for the follow-up remediation:

- `bash -n scripts/verify-binary-embedded-package-distribution.sh` -> clean.
- `npm run build:embedded-packages` -> staged 8 packages / 717 files.
- `node scripts/check-package-closure.mjs` -> OK.
- `make -pn build-packages` contains
  `EMBEDDED_PKG_BUILD_SCRIPTS := packages/convex/build.mjs packages/codegen/build.mjs`.
- `cargo test -p nimbus-bin embedded_packages::tests -- --nocapture` ->
  5 passed.
- `npm run docs:validate-refs:strict` -> pass (241 files).
- `git diff --check` -> clean.
- `BPD_FULL=1 bash scripts/verify-binary-embedded-package-distribution.sh`
  -> 27 passed, 0 failed.

## Known follow-ups (recorded, out of BPD scope)

- The `ci.yml`/`coverage.yml` Linux `embedded-packages` artifact wiring is
  `actionlint`-clean and proven by a full-graph local `make build`, but the
  cross-job upload/download + mtime refresh is **not yet validated on real
  GitHub runners** — needs a push to confirm. Release jobs intentionally do not
  download a shared package artifact: they stage the payload target-locally so
  the embedded tooling closure contains the correct native `@esbuild/*` binary
  for each release target.
- Lifting Cloud Functions into the in-binary contract requires a `nimbus/deno`
  `child_process`/`worker_threads` IPC fix so esbuild's plugin path runs
  in-binary — a separate plan.
