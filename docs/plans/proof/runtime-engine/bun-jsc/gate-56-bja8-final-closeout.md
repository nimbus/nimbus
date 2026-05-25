# Gate 56: BJA8 Final Closeout

Date: 2026-05-24

## Purpose

`BJA8` closes the Bun/JSC linked-adapter plan with reproducible source
ownership and final local plus Debian verification. This gate supersedes Gate
55's local WebKit environment stop: the local WebKit source prerequisite was
installed, the macOS shared-adapter link issues found by the full local gate
were fixed in the Nimbus Bun fork, and the same `.5` Bun source checkpoint
passed locally and on Debian 13 `minicloud`.

## Source Ownership

Current Bun proof source:

```text
Repository: https://github.com/nimbus/bun
Branch: nimbus/bja4l2-simdutf-namespace
Tag: bun-v1.4.0-nimbus.5
Revision: ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57
```

The `.5` checkpoint is pushed to `nimbus/bun`. It preserves the HostBridge
adapter ABI from `.4` and adds the macOS shared-adapter link fixes needed for
local source-backed verification:

- remove the executable-only `-Wl,-stack_size,...` flag from the shared
  adapter link
- remove Bun's executable `symbols.txt` export-list pair from the shared
  adapter before adding the Nimbus-only ABI export list
- keep the existing Linux shared-adapter path and source-owned simdutf
  namespace isolation intact

Local WebKit source was installed at the Bun-expected revision:

```text
/Users/jack/src/github.com/oven-sh/WebKit
782504c968e2ae06a511c9e7a4d48318b2a23263
```

## Local Mac Verification

Command:

```sh
make verify-bun-jsc-linked-adapter
```

Result: passed.

Evidence:

- default no-link runtime contract passed:
  - 11 runtime policy tests
  - 9 Bun/JSC pool/scaffold tests
  - 15 Convex registry tests
  - 2 runtime diagnostics tests
  - 1 tenant-admission test
  - 2 operator UI files / 5 tests
- linked no-shared-library unit contract passed 12 tests.
- Bun source export check found all 11 required Nimbus ABI exports.
- Bun Rust format passed.
- native macOS shared adapter built from local WebKit/JSC source.
- generated build graph safety policy passed.
- shared adapter export audit found exactly 11 Nimbus ABI exports and 0 leaked
  native defined symbols.
- macOS platform symbol audit was intentionally skipped by the verifier because
  the strict native-symbol audit is required on Linux.
- linked runtime unit lane passed 12 tests, including pure Bun/JSC program
  invocation through the pool and same-process V8 plus Bun/JSC coexistence.
- loaded shared-adapter integration lane passed 7 tests:
  - microtask progress
  - HostBridge allow
  - HostBridge deny
  - forged tenant/context rejection
  - HostBridge cancellation
  - fresh/discard guest state reset
  - same-process V8 plus Bun/JSC coexistence
- linked server diagnostics proof passed 1 test.
- Nimbus whitespace diff check passed.
- Bun whitespace diff check passed.

The local gate found and fixed two real macOS product issues before passing:

1. `ld: -stack_size option can only be used when linking a main executable`
2. Bun's normal executable export list leaked V8/N-API/libuv symbols from
   `libnimbus_bun_jsc_embedder.dylib`

Both fixes are source-owned in `bun-v1.4.0-nimbus.5`.

## Debian 13 Verification

Command:

```sh
ssh nimbus@192.168.4.29 'set -e; export PATH=/home/nimbus/.bun/bin:/home/nimbus/.cargo/bin:$PATH; cd /home/nimbus/src/github.com/nimbus/nimbus-worktrees/bja5-hostbridge; NIMBUS_BUN_REPO=/home/nimbus/src/github.com/nimbus/bun-worktrees/bja5-hostbridge NIMBUS_BUN_EXPECTED_REF=bun-v1.4.0-nimbus.5 NIMBUS_BUN_EXPECTED_REV=ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57 bash scripts/verify-bun-jsc-linked-adapter.sh'
```

Result: passed.

Evidence:

- default no-link runtime contract passed:
  - 11 runtime policy tests
  - 9 Bun/JSC pool/scaffold tests
  - 15 Convex registry tests
  - 2 runtime diagnostics tests
  - 1 tenant-admission test
  - 2 operator UI files / 5 tests
- linked no-shared-library unit contract passed 12 tests.
- Bun source export check found all 11 required Nimbus ABI exports.
- Bun Rust format passed.
- native Linux shared adapter built from source-backed WebKit/JSC cache.
- generated build graph safety policy passed.
- ELF shared adapter audit found exactly 11 Nimbus ABI exports and 0 leaked
  native defined symbols.
- strict Linux simdutf namespace audit passed:
  - `libWTF.a` has 526 `nimbus_bun_simdutf::` definitions and 0 plain
    `simdutf::` definitions
  - `libJavaScriptCore.a` has 0 definitions for both simdutf families
  - `bun-simdutf.cpp.o` has 60 `nimbus_bun_simdutf__*` definitions and 0
    plain `simdutf__*` definitions
  - V8/rusty_v8 artifacts keep plain simdutf definitions and contain 0
    Nimbus Bun namespace definitions
- linked runtime unit lane passed 12 tests, including pure Bun/JSC program
  invocation through the pool and same-process V8 plus Bun/JSC coexistence.
- loaded shared-adapter integration lane passed 7 tests:
  - microtask progress
  - HostBridge allow
  - HostBridge deny
  - forged tenant/context rejection
  - HostBridge cancellation
  - fresh/discard guest state reset
  - same-process V8 plus Bun/JSC coexistence
- linked server diagnostics proof passed 1 test.
- Nimbus whitespace diff check passed.
- Bun whitespace diff check passed.

## Broad Local Baseline

Gate 55 recorded the broad local baseline before the final linked closeout:

- `make check` passed.
- `make clippy` passed with `-D warnings`.
- `npm run typecheck` passed with existing route-generator warnings.
- `npm run test` passed, including 42 UI test files / 278 UI tests.
- `npm run build` passed with existing route-generator, TanStack route, and
  Vite chunk-size warnings.
- `cargo fmt --all --check` passed.
- `git diff --check` passed.

Docs reference validation remains unavailable because `package.json` does not
define `docs:validate-refs:strict`.

## Final Lightweight Checks

After updating the `.5` source contract and closeout evidence:

- `cargo fmt --all --check` passed.
- `npm run typecheck` passed with the existing route-generator warnings about
  helper files that do not export routes.
- `git diff --check` passed.

## Decision

`BJA8` is complete. Bun/JSC is still optional and fail-closed by default, but
the linked in-process Bun/JSC backend now has source-owned Bun fork/tag
ownership, a macOS source-backed linked gate, a Debian 13 source-backed linked
gate with strict ELF/symbol audits, product diagnostics, operator diagnostics,
HostBridge containment tests, cancellation/teardown tests, and broad local
baseline evidence.
