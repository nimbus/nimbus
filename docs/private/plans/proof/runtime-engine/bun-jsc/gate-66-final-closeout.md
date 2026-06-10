# Gate 66: Final Closeout

Date: 2026-05-25

## Decision

`docs/plans/archive/bun-jsc-distribution-and-release-plan.md` is complete through
`BJD9`.

The optional Bun/JSC adapter is now a committed runtime-backend contract beside
the default Deno/V8/Node-compatible lanes. The default Nimbus install remains a
single-binary no-link install that reports Bun/JSC as `not_linked` and fails
closed. Installs that add the verified `nimbus-bun-jsc-adapter` package can
discover the packaged manifest without manual override environment variables.

This closeout records broad repository verification, the installed-package
proofs from Gate 65, release/package helper proof, diagnostics proof, and the
explicit absence of strict docs reference validation.

## Baseline

Closeout started from:

```text
Nimbus HEAD: abcf7570 Prove Bun JSC installed package discovery
Bun source tag: bun-v1.4.0-nimbus.5
Bun source revision: ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57
```

The BJD8 commit already included:

- packaged manifest discovery for macOS Homebrew-style installs
- packaged manifest discovery for Debian/Linux installs
- literal `"use bun";` linked adapter execution from an installed package
- no-link fallback after installed adapter removal
- package archive mode hardening and verifier extraction hardening

## Required Closeout Verification

Passed locally:

```text
cargo fmt --all --check
make check
make clippy
npm run typecheck
npm run test
npm run build
make verify-bun-jsc-runtime-contract
bash scripts/verify-bun-jsc-adapter-package-helper.sh
make proof-helpers
make verify-bun-jsc-linked-adapter
git diff --check
```

Important result details:

- `make check` ran `cargo check --workspace` and finished successfully.
- `make clippy` passed with `-D warnings`.
- `npm run typecheck` passed; TanStack route-file warnings were emitted and are
  existing route-generation noise.
- `npm run test` passed with 42 UI test files and 278 UI tests.
- `npm run build` passed; TanStack route/export warnings, Node
  `module.register()` deprecation, and Vite chunk-size warning were emitted.
- `make verify-bun-jsc-runtime-contract` passed after rerunning outside the
  Codex filesystem sandbox for the known local listener-binding restriction.
  The gate covered runtime policy/memory semantics, Bun/JSC pool scaffold,
  Convex runtime lane registry, runtime diagnostics API, tenant admission, and
  operator UI diagnostics.
- `make proof-helpers` passed after rerunning outside the Codex filesystem
  sandbox for the known proof-helper Unix-socket binding restriction. It covered
  SQLCipher, machine guest/service/Homebrew helpers, Bun/JSC adapter package
  helper, Bun/JSC release asset helper, Linux release package helper, and the
  install helper.
- `make verify-bun-jsc-linked-adapter` passed the default no-link contract,
  linked feature no-library contract, source export audit, source-owned Bun
  shared adapter build, generated build-graph safety policy, shared adapter
  export/native-symbol audit, linked runtime unit suite, linked integration
  suite, server linked-lane diagnostics proof, and whitespace diff checks.

## Installed-Package Proofs

Gate 65 remains the package-discovery proof for this closeout.

macOS proof:

```text
target_triple=aarch64-apple-darwin
installed_manifest=/opt/homebrew/opt/nimbus/libexec/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json
archive_sha256=f61ae60fe6190d95816969630b16cb03064bdb85db986d8e7f1106d2c27e2882
library_sha256=601971d856bfc6cbe067875ab8cf8f644bc04fe53c8a0a076d929dc632897d06
```

Debian 13 `minicloud` proof:

```text
ssh_target=nimbus@192.168.4.29
hostname=minicloud
kernel=Linux minicloud 6.12.88+deb13-amd64
target_triple=x86_64-unknown-linux-gnu
installed_manifest=/usr/libexec/nimbus/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json
archive_sha256=0862f8e2a87a87e5a9f215ad5aff0edf1cf7c010c4482c56238cf0a9340787de
library_sha256=a8a9d0af77758716eed0f0f8a5813b9321d400bd9924760210575ac87c2d600a
```

Both proofs ran with these override variables unset:

```text
NIMBUS_BUN_EMBED_SHARED_LIBRARY
NIMBUS_BUN_JSC_ADAPTER_MANIFEST
```

Both proofs passed archive verification, packaged discovery, literal
`"use bun";` execution, same-process V8 plus Bun/JSC behavior, server
diagnostics, no-link fallback after installed layout removal, and the default
Bun/JSC runtime contract.

## Docs Validation

Strict docs reference validation was attempted:

```text
npm run docs:validate-refs:strict
```

Result:

```text
npm error Missing script: "docs:validate-refs:strict"
```

The repository does not currently expose that npm script, so the absence is
recorded explicitly instead of silently skipping docs validation.

## Extra Non-Gate Evidence

An optional `make test` sweep was started after the required BJD9 gates. That
command is not part of the BJD9 completion contract. It failed in
`nimbus-runtime` Node official compatibility coverage, then a serialized
isolation rerun was stopped after it hung for about an hour waiting on a spawned
child process named `foo`.

Observed failing Node-compat tests included:

```text
runtime::tests::node_compat::node20_loader_context_crypto_dh_and_ecdh_batch_fixture
runtime::tests::node_compat::node20_supported_lane_executes_official_networking_subset
runtime::tests::node_compat::node20_supported_lane_executes_official_streams_and_local_io_subset
```

This is recorded as Node-compat drift/harness evidence, not as a Bun/JSC
distribution failure. The Bun/JSC-specific closeout gates above passed.

## Closeout

The plan ledger, plan index, proof docs, package scripts, runtime discovery,
diagnostics, install/package surfaces, release lanes, and local git history now
agree:

- `BJD0-BJD9` are complete.
- Bun/JSC is optional and fail-closed by default.
- Packaged Bun/JSC adapter discovery works without manual development
  overrides.
- Local macOS and Debian 13 `minicloud` package proofs passed.
- Broad required verification passed, with unavailable docs validation
  recorded.
