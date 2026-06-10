# Gate 53: BJA7 Product Metadata And Linked Diagnostics Proof

Date: 2026-05-24

## Purpose

`BJA7` promotes Bun/JSC from proof-only runtime policy into explicit product
metadata. The goal is not to make Bun/JSC the default JavaScript runtime. The
goal is to let Convex-compatible artifacts select Bun/JSC deliberately with
`"use bun";`, keep default builds fail-closed, keep V8/Node artifacts separate,
and expose linked or not-linked adapter state through diagnostics.

## Source

The verified Bun source remains the Nimbus-owned fork:

```text
Repository: https://github.com/nimbus/bun
Branch: nimbus/bja4l2-simdutf-namespace
Tag: bun-v1.4.0-nimbus.4
Revision: 7c6dd4312e437c67a6c4c8cbb252f0d7ae898db8
```

No Bun source change was required for this gate.

## Implementation

Codegen now recognizes top-level `"use bun";` modules and emits Bun/JSC
runtime metadata:

- `runtime_environment: "bun"`
- `runtime_engine: "bun_jsc"`
- `runtime_javascript_evaluation_format: "program_wrapper"`
- `runtime_compatibility_target: "bun_jsc"`
- `runtime_package_resolution: "bun_self_contained"`

Bun-selected functions are packaged into
`.nimbus/convex/bun_program_bundle.js` plus
`.nimbus/convex/bun_program_bundle.sha256`. The default
`.nimbus/convex/bundle.mjs` is now generated from non-Bun functions only, so a
Bun handler is not duplicated into the V8/Node ESM artifact.

The Convex registry loads the optional Bun/JSC program bundle and selects the
required bundle per function. Default and Node functions keep using
`bundle.mjs`; Bun/JSC functions require `bun_program_bundle.js`. Default
no-link builds still reject Bun-selected functions before guest execution with
the adapter-not-linked error.

The runtime crate now exposes `bun_jsc_execution_adapter_state()`, and the
server/bin facade features forward `bun-jsc-linked-adapter` so diagnostics can
reflect the actual linked adapter state. The linked verifier now includes a
server-side diagnostics proof under that feature.

## Passing Proof

Local focused checks:

- `npm run test --workspace @nimbus/codegen` passed, including the
  `"use bun";` fixture that proves the Bun program bundle exists, has a
  SHA-256 sidecar, contains `runtime_environment: "bun"`, and does not leak
  the Bun function into `bundle.mjs`.
- `npm run typecheck --workspace @nimbus/codegen` passed.
- `cargo test -p nimbus-server registry_and_license::registry --lib` passed
  15 tests, including Bun/JSC program-bundle loading and linked-state
  diagnostics.
- `cargo test -p nimbus-server
  adapters::convex::registry::resolution::runtime_access::tests::bun_jsc_function_fails_closed_when_adapter_is_not_linked
  --lib` passed 1 test.
- `cargo check -p nimbus-bin --features bun-jsc-linked-adapter` passed.
- `cargo fmt --all --check` passed.
- `bash -n scripts/verify-bun-jsc-linked-adapter.sh` passed.
- `git diff --check` passed.
- `make verify-bun-jsc-runtime-contract` passed outside the Codex sandbox
  after the sandboxed run hit the expected local listener permission wall:
  11 runtime policy tests, 9 Bun/JSC no-link scaffold tests, 15 Convex
  registry tests, 2 runtime diagnostics tests, 1 tenant-admission test, and
  2 operator UI files / 5 tests.

Debian 13 `minicloud` checks used:

```text
Nimbus repo: /home/nimbus/src/github.com/nimbus/nimbus-worktrees/bja5-hostbridge
Bun repo: /home/nimbus/src/github.com/nimbus/bun-worktrees/bja5-hostbridge
Bun proof root: /home/nimbus/.cache/nimbus-bun-proof
```

The BJA7 code patch was applied on top of the existing BJA6 proof worktree,
and remote `git diff --check` passed before running the verifier.

The full `bash scripts/verify-bun-jsc-linked-adapter.sh` gate passed against
the `.4` source revision. Evidence:

- default no-link runtime contract passed:
  - 11 runtime policy tests
  - 9 Bun/JSC no-link scaffold tests
  - 15 Convex registry tests
  - 2 runtime diagnostics tests
  - 1 tenant-admission test
  - 2 operator UI files / 5 tests
- linked no-shared-library unit contract passed 12 tests.
- Bun source export check found all 11 required Nimbus ABI symbols.
- Bun Rust format passed.
- native shared adapter build reused the home-backed
  `/home/nimbus/.cache/nimbus-bun-proof/shared-adapter-release-local-namespaced`
  artifact and passed.
- generated build graph safety policy passed.
- shared adapter export audit found exactly the 11 Nimbus C ABI exports.
- leaked native defined symbol count was 0.
- ELF audit found no `STATIC_TLS`.
- simdutf namespace audit still separates Bun/WebKit from V8/rusty_v8.
- linked same-process unit lane passed 12 tests, including pure program-wrapper
  execution and V8 plus Bun/JSC coexistence.
- `tests/bun_jsc_linked_adapter.rs` passed 7 loaded shared-adapter integration
  tests:
  - HostBridge allow
  - HostBridge deny
  - forged tenant/context rejection
  - HostBridge cancellation
  - microtask progress
  - fresh/discard guest state reset
  - same-process V8 plus Bun/JSC coexistence
- the new server linked-lane diagnostics proof passed:
  `convex_registry_bun_jsc_lane_diagnostics_reflect_runtime_adapter_state`.
- Nimbus and Bun whitespace diff checks passed.

The linked server diagnostics proof had to compile the `nimbus-server` test
target with `bun-jsc-linked-adapter`, which took 33m50s on `minicloud` before
the single diagnostics test ran and passed. This is acceptable BJA7 evidence,
but it is also a BJA8 cache/wall-clock concern.

## Decision

`BJA7` is complete. Bun/JSC is now selectable through explicit product
metadata and a separate program-wrapper artifact while default builds remain
fail-closed. Diagnostics reflect the real linked or not-linked adapter state,
and the linked server feature path is proven on Debian 13.

Next gate: `BJA8` must start with disk/cache verification preflight, then run
the final broad local and Debian verification baseline before marking the plan
complete.
