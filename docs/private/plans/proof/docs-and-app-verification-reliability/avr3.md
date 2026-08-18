# AVR3 Fresh-Checkout Prerequisites and Node Contract

Date: 2026-08-17

## Result

AVR3 is complete in work commit
`8042e32e42557698a756198ef91bdb06092bfde4`. The Make entry now checks the
host before it enters single-flight work. Its nested target builds the
generated UI and embedded-package inputs before the runner can invoke Cargo.

The direct script checks the same host contract. It accepts Node.js versions
from 22 through 24. Node.js 22 and 24 are the tested anchors. If generated
inputs are absent, the script stops before work and reports the exact supported
command: `make examples-verify`.

An executable `NIMBUS_EXAMPLES_VERIFY_BIN` skips generated inputs and Cargo.
An empty value uses the normal build path. A missing or non-executable supplied
path fails during host preflight.

## Fail-before evidence

| Case | Result before AVR3 |
| --- | --- |
| Source verifier | AVRC11 and AVRC12 reported `0 passed, 2 failed`. |
| Raw Cargo, first attempt | `cargo build -p nimbus-bin --bin nimbus --locked` stopped because `packages/nimbus-ui/dist/index.html` was absent. |
| Raw Cargo, second attempt | After only the UI artifact was present, Cargo stopped because `crates/nimbus-assets/embedded/packages/manifest.json` was absent. |
| Unsupported host | Node.js 20.20.2 reached later case-selection work instead of failing during host preflight. |

## Acceptance ledger

| Action | Result | Evidence |
| --- | --- | --- |
| AVR3.1 Define both entry contracts. | Pass. | `make examples-verify` owns generation and build. Direct invocation either uses complete inputs or reports the Make entry before work. |
| AVR3.2 Add explicit prerequisites. | Pass. | `examples-verify-run` depends on `UI_DIST_INDEX` and `EMBEDDED_PKG_MANIFEST` when no binary is supplied. |
| AVR3.3 Keep the supplied-binary path. | Pass. | A valid executable bypasses both generated inputs and Cargo. An invalid path fails before those effects. |
| AVR3.4 Add the Node preflight. | Pass. | Node.js 22, 23, and 24 pass. Versions 20, 21, 25, and 26 fail. Missing, malformed, and failing version probes also fail. |
| AVR3.5 Test each host mode. | Pass. | The focused suite covers 13 host and entry behaviors. The real Node.js 22 and 24 lanes each ran `nimbus/tasks`. |

## Live fresh-export proof

The test used this tracked-files-only export:

`/var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/nimbus-avr3-live.XXXXXX.tFSOmBjVAz`

Before the run, the export had no UI index, embedded-package manifest, or
`target/debug/nimbus` executable. The test copied the target cache without the
executable to bound dependency compilation. `npm ci` ran under Node.js 22
before application verification.

This command generated both inputs, built `nimbus-bin`, and passed the five
`nimbus/tasks` smoke assertions:

```bash
NIMBUS_EXAMPLES_VERIFY_ONLY=nimbus/tasks make examples-verify
```

The command ran under Node.js 22. The Rust fallback completed in 1 minute and
39 seconds. The app passed create, list, toggle, delete, and live-update checks.

The same export then passed the direct entry under Node.js 24:

```bash
NIMBUS_EXAMPLES_VERIFY_ONLY=nimbus/tasks bash scripts/examples-verify.sh
```

Both runs left the owner worktree unchanged outside the three AVR3-owned files.

## Verification evidence

| Command or check | Result |
| --- | --- |
| `bash -n scripts/examples-verify.sh scripts/examples-verify-contract-test.sh` | Pass. |
| `shellcheck scripts/examples-verify.sh scripts/examples-verify-contract-test.sh` | Pass with no diagnostics. |
| `bash scripts/examples-verify-contract-test.sh --task AVR3` | Pass. AVRC11-AVRC12 are 2/2, and 13 behavior cases pass. |
| `bash scripts/verify-docs-app-verification.sh --task AVR3` | Pass. AVRC11-AVRC12 are 2/2. |
| AVRC11 and AVRC12 mutation tests | Pass. Both mutations fail closed. |
| `bash scripts/verify-docs-app-verification.sh --self-test` | Pass. All 24/24 mutations fail closed. |
| Real Node.js preflight matrix | Node.js 20 fails, 22 passes, 24 passes, and 26 fails. |
| Fresh export through Make on Node.js 22 | Pass. Three missing outputs became present, and all 5/5 app assertions passed. |
| Direct script on Node.js 24 | Pass. All 5/5 app assertions passed. |
| `git diff --check` | Pass. |

## Routed cleanup

The fresh UI prerequisite emitted 18 TanStack route-file warnings for support
modules under `packages/nimbus-ui/src/routes`. AVRF23 routes this low-severity
verification-noise cleanup to AVR10. It does not change the AVR3 host or build
contract.
