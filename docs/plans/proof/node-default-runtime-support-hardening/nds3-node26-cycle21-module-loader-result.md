# NDS3 node26 cycle 21 - module loader and regexp inspect promotion

Date: 2026-06-15
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

This wave drained the Node26 module-loader required-surface selector.
Node26 `v8_isolate_required` posture moved from `215` gaps / `89.79%`
to `167` gaps / `92.06%`.

Movement came from:

- 46 dynamically promoted Node26 module-loader / ESM / CJS / module-hooks
  fixtures.
- 2 honest structural classifications out of `v8_isolate_required`:
  `test/parallel/test-module-loading-error.js` and
  `test/parallel/test-util-styletext.js`.

Deno fork tag: `v2.8.3-nimbus.54`
Commit: `2391848066fb29b7748aee4a018566630b001e43`

Nimbus was temporarily pinned to the canonical local Deno worktree while
proving the fork-owned fixes, then repinned to the immutable published
`v2.8.3-nimbus.54` tag before the final promoted and drained-selector proofs.

## Root Cause

The module-loader cluster exposed a composed set of Deno fork and Nimbus
harness gaps:

- Deno's CommonJS loader needed Node26-compatible package-scope handling across
  nested `node_modules` boundaries, including `--preserve-symlinks-main`
  behavior.
- Builtin module loading needed to bypass user resolve/load hooks when Node
  requires that native builtins remain authoritative.
- `module.register()` needed Node26 `DEP0205` deprecation behavior and option
  parsing for `--no-deprecation`, `--throw-deprecation`, and
  `--no-throw-deprecation`.
- CJS named-export analysis needed to trim object-literal exports that Node's
  lexer does not expose while keeping identifier, class, function, and method
  exports.
- The Nimbus test bundle path rewriter was canonicalizing symlink fixture
  source paths too early. It now first tries the lexical source-root path before
  falling back to canonicalized target matching.
- `util.inspect()` regexp colorization needed the Node26 component-level regexp
  highlighter. The first immutable tag, `v2.8.3-nimbus.53`, omitted those two
  Deno console/inspect files and failed only
  `test/parallel/test-util-inspect-regexp.js`; the corrective
  `v2.8.3-nimbus.54` tag includes them and passes the full promoted batch.

The sandbox boundary stayed intact. This wave did not add host process exit,
OS signal handlers, subprocess execution, global host-cwd mutation, native
addon loading, or wider host filesystem grants.

## Structural Classifications

`test/parallel/test-module-loading-error.js` requires
`../fixtures/module-loading-error.node`, a native `.node` addon fixture. Loading
that fixture would require host-native FFI / dlopen outside the V8 isolate, so
it is classified as `requires_native_addon_harness` under
`loader-context/native-addon-host`.

`test/parallel/test-util-styletext.js` calls `common.getTTYfd()` and needs a
host TTY file descriptor. It is classified as
`requires_pseudo_tty_host_harness` under `process-and-timing/tty-host`.

## Verification

Deno fork focused checks:

```bash
CARGO_ENCODED_RUSTFLAGS=$'--cfg\x1ftokio_unstable\x1f-C\x1flink-args=-weak_framework Metal -weak_framework MetalPerformanceShaders -weak_framework QuartzCore -weak_framework CoreGraphics' \
  cargo test -p deno_resolver --lib package_json_scope -- --nocapture
# 2 passed; 0 failed; 36 filtered out

CARGO_ENCODED_RUSTFLAGS=$'--cfg\x1ftokio_unstable\x1f-C\x1flink-args=-weak_framework Metal -weak_framework MetalPerformanceShaders -weak_framework QuartzCore -weak_framework CoreGraphics' \
  cargo test -p deno_resolver --features deno_ast --lib cjs_analysis -- --nocapture
# 3 passed; 0 failed; 38 filtered out
```

The explicit `CARGO_ENCODED_RUSTFLAGS` bypassed Deno's checked-in macOS
`-fuse-ld=lld` target flag for this host-only verification run. The first
un-overridden attempt failed before reaching Deno code with:

```text
clang: error: invalid linker name in argument '-fuse-ld=lld'
```

Nimbus focused harness unit proof:

```bash
cargo test -p nimbus-runtime --lib rewrite_bundle_path_preserves_symlink_source_path -- --nocapture
# 1 passed; 0 failed; 953 filtered out
```

Local broad proof against the canonical local Deno worktree:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-symlink-main-bundle1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_module_loader_required_surface_blocker_watchpoint -- --ignored --nocapture
# selected=48, passed=46, skipped=1, failed=1
```

The one failure was the native-addon fixture
`test/parallel/test-module-loading-error.js`; the one skip was the host-TTY
fixture `test/parallel/test-util-styletext.js`. Both were source-confirmed and
classified out of the isolate-required denominator.

Promoted batch proof against the canonical local Deno worktree:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-module-loader-promoted1 \
  cargo test -p nimbus-runtime --lib node26_current_lane_executes_esm_module_loader_promoted_batch_fixture -- --nocapture
# selected=146, passed=146, skipped=0, failed=0
```

First immutable-tag attempt, intentionally retained as a false-tag guard:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-module-loader-promoted-tag53 \
  cargo test --locked -p nimbus-runtime --lib node26_current_lane_executes_esm_module_loader_promoted_batch_fixture -- --nocapture
# selected=146, passed=145, skipped=0, failed=1
# failed: test/parallel/test-util-inspect-regexp.js
```

Corrected immutable-tag promoted proof:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-module-loader-promoted-tag54 \
  cargo test --locked -p nimbus-runtime --lib node26_current_lane_executes_esm_module_loader_promoted_batch_fixture -- --nocapture
# selected=146, passed=146, skipped=0, failed=0
```

Immutable-tag drained selector proof:

```bash
NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT=/private/tmp/nds-node26-module-loader-drained-tag54 \
  cargo test --locked -p nimbus-runtime --lib node26_current_lane_module_loader_required_surface_blocker_watchpoint -- --ignored --nocapture
# selected=0, passed=0, skipped=0, failed=0
```

Generator and integrity checks:

```bash
git diff --check
# no output

cargo fmt --all --check
# no output

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/default_support_posture.py --check
# node default support posture: pass

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/required_surface_blockers.py --check
# node required-surface blocker inventory: pass

/opt/homebrew/bin/python3.12 -B scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 150 entries
```

Current posture after regeneration:

- Node22 `v8_isolate_required`: `0` gaps, `100%`, `2363 / 2363`.
- Node24 `v8_isolate_required`: `0` gaps, `100%`, `2400 / 2400`.
- Node26 `v8_isolate_required`: `167` gaps, `92.06%`, `1936 / 2103`.
- Node26 module-loader required selector: `0`.

Verifier:

```bash
bash scripts/verify-node-default-runtime-support-hardening.sh
# Summary: 14 passed, 20 failed
```

Step 9 remains green for Node22/Node24. The overall verifier remains red
honestly because Node26 still has `167` required gaps and the final closeout
proof rows are not complete.

## Diagnostics

Useful diagnostic artifacts from this wave:

- `/private/tmp/nds-node26-symlink-main-bundle1`
- `/private/tmp/nds-node26-module-loader-promoted1`
- `/private/tmp/nds-node26-module-loader-promoted-tag53`
- `/private/tmp/nds-node26-module-loader-promoted-tag54`
- `/private/tmp/nds-node26-module-loader-drained-tag54`

Summary artifacts:

- `/private/tmp/nds-node26-module-loader-promoted-tag53/batch/node26__node26_current_lane_executes_esm_module_loader_promoted_batch__summary.json`
- `/private/tmp/nds-node26-module-loader-promoted-tag54/batch/node26__node26_current_lane_executes_esm_module_loader_promoted_batch__summary.json`
- `/private/tmp/nds-node26-module-loader-drained-tag54/batch/node26__node26_current_lane_module_loader_required_surface_blocker_watchpoint__summary.json`

## Remaining Node26 Required Gaps

After regeneration, Node26 has `167` required gaps. The current owner grouping
from the generated posture is:

- `67` `node-compat/unpromoted-surface`
- `34` `node-compat/current-lane`
- `23` `loader-context/vm`
- `18` `loader-context/domain`
- `7` `runtime/v8`
- `6` `process-and-timing/process-host`
- `5` `streams-local-io/fs-host-io`
- `4` `core-semantics/console`
- `2` `core-semantics/assert`
- `1` `core-semantics/url`

Recommended next waves are high-yield clusters from `unpromoted-surface`,
`node-compat/current-lane`, and `loader-context/domain` before lower-yield
singleton cleanup. `loader-context/vm` still includes known deep/native blockers
and should be handled as a coherent VM wave, not a casual singleton pass.

## Integrity

- No V8 or rusty_v8 changes were made.
- No official upstream Node fixture or checker was edited.
- No generated JSON was hand-edited to fake a green result.
- No local Deno path pin remains in `Cargo.toml` or `Cargo.lock`.
- Deno fork is clean at `v2.8.3-nimbus.54`.
- `measure_ah.sh` and other scratch files remain untracked.
