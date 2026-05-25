# Gate 55: BJA8 Final Verification Progress

Date: 2026-05-24

## Purpose

`BJA8` closes the Bun/JSC linked-adapter plan only when broad local gates,
linked-adapter proof, Debian source-backed proof, source ownership, and final
documentation are all current. This progress record captures the broad local
verification completed after the `BJA7` product-metadata checkpoint and the
`BJA8` disk/cache preflight.

This gate is not the closeout. It is superseded by Gate 56, which installs the
local WebKit source prerequisite, fixes the macOS shared-adapter link/export
issues in the Nimbus Bun fork, and records passing local plus Debian linked
gates.

## Baseline

Current local checkpoint sequence:

```text
87625247 Promote Bun JSC product metadata
3f73b586 Record Bun JSC final verification preflight
```

Unrelated local dirty files remain outside the Bun/JSC commits:

- generated Convex demo files under `demos/convex/*/convex/_generated/`
- `package-lock.json`
- `docs/plans/node-compat-cron-greening-plan.md`
- untracked desktop-auth proof screenshots

## Passing Local Broad Gates

The following gates passed on the local Mac after the BJA8 preflight:

- `make check`
  - `cargo check --workspace` finished successfully.
- `make clippy`
  - `cargo clippy --workspace --all-targets -- -D warnings` finished
    successfully.
- `npm run typecheck`
  - passed with existing route-generator warnings about helper files that do
    not export routes.
- `npm run test`
  - passed, including 42 UI test files / 278 UI tests.
- `npm run build`
  - passed with existing route-generator warnings, TanStack route export
    warnings, and the existing Vite chunk-size warning.
- `cargo fmt --all --check`
  - passed.
- `git diff --check`
  - passed.

Docs reference validation remains unavailable:

```text
npm error Missing script: "docs:validate-refs:strict"
```

This matches the earlier BJA4L6 broad-gate finding: the repository does not
currently define that npm script.

## Local Linked-Adapter Gate Result

`make verify-bun-jsc-linked-adapter` was run locally outside the Codex sandbox.
It did not pass, but it failed at the expected local environment prerequisite,
not at Nimbus runtime behavior:

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
- the gate stopped at `[5/11] Bun native shared adapter build`.

Failure:

```text
error: local dep "WebKit" source not found at /Users/jack/src/github.com/nimbus/bun/vendor/WebKit
  hint: Clone oven-sh/WebKit to vendor/WebKit/, or set $BUN_WEBKIT_PATH to an existing clone (useful for worktrees)
```

This matches Gate 54's preflight:

```text
/Users/jack/src/github.com/nimbus/bun/vendor/WebKit: absent
BUN_WEBKIT_PATH: unset
```

## Debian Linked Proof

The current implementation still has source-backed Debian evidence from Gate
53. On Debian 13 `minicloud`, using home-backed proof caches, the full linked
verifier passed after applying the BJA7 code patch:

- default no-link contract
- 11 Bun exports
- shared adapter build/export/leak/simdutf audit
- 12 linked unit tests
- 7 loaded shared-adapter integration tests
- linked server diagnostics proof
- Nimbus/Bun whitespace checks

No product code changed after that Debian proof; the later BJA8 preflight
commit is documentation only.

## Decision

`BJA8` was not complete at this checkpoint. Broad local gates were green, and
the Debian source-backed linked proof was green. Gate 56 supersedes this result
by resolving the local linked-adapter gate requirement and closing BJA8.
