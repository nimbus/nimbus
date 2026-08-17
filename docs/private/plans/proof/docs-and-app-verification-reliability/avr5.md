# AVR5 Explicit Compose Mode

Date: 2026-08-17

## Result

AVR5 is complete in work commit
`02788d24bababd1063b89241ff3274dc36b47a79`. `nimbus dev` now accepts
`--no-compose-discovery`. The flag skips both `COMPOSE_FILE` and walk-up
discovery. Clap rejects its use with `--compose-file`.

Default discovery and explicit file selection keep their prior behavior. The
two dev-mode application cases use the opt-out. The runner no longer renames,
restores, or heals a backup of tracked `compose.yaml`.

## Fail-before evidence

| Case | Result before AVR5 |
| --- | --- |
| Source verifier | AVRC16 and AVRC17 reported `0 passed, 2 failed`. |
| Behavior-first test | `compose_discovery_opt_out_performs_no_discovery` failed with `UnknownArgument` for `--no-compose-discovery`; Cargo exited 101. |
| Root project | The fixture contains both `docker-compose.yaml` and `docker-compose.yml`. Existing discovery rejects this ambiguity. |
| Runner | The runner moved `compose.yaml` to `compose.yaml.smoke-bak`, restored it after boot, and tried to heal a stranded backup at startup. |

## Acceptance ledger

| Action | Result | Evidence |
| --- | --- | --- |
| AVR5.1 Add explicit no-discovery mode. | Pass. | `DevCommand` owns the flag. Dev planning returns no Compose selection before it can read environment or source paths. |
| AVR5.2 Preserve default and explicit behavior. | Pass. | `compose_discovery_defaults_to_enabled` and `explicit_compose_file_still_loads` each pass. |
| AVR5.3 Route both dev cases. | Pass. | Firebase Tasks and Cloud Functions Tasks declare `--no-compose-discovery` in the case manifest. Both live cases pass. |
| AVR5.4 Delete sideline recovery. | Pass. | The runner contains no sideline, restore, backup, or Compose `mv` path. A signal cannot strand a backup that no longer exists. |
| AVR5.5 Update CLI documentation. | Pass. | The CLI reference states precedence and conflict behavior. The source map cites the flag's source. |

## Verification evidence

| Command or check | Result |
| --- | --- |
| `cargo test -p nimbus-cli compose_discovery_opt_out -- --nocapture` | Pass. 3 passed, 0 failed. |
| `cargo test -p nimbus-cli compose_discovery_defaults_to_enabled -- --nocapture` | Pass. 1 passed, 0 failed. |
| `cargo test -p nimbus-cli explicit_compose_file_still_loads -- --nocapture` | Pass. 1 passed, 0 failed. |
| `cargo test -p nimbus-cli --lib` | Pass. 1,015 passed, 0 failed, 4 ignored. |
| `cargo clippy -p nimbus-cli --all-targets -- -D warnings` | Pass. |
| `cargo fmt --all --check` | Pass. |
| `bash scripts/examples-verify-contract-test.sh --task AVR5` | Pass. AVRC16-AVRC17 are 2/2. |
| `bash scripts/verify-docs-app-verification.sh --task AVR5` | Pass. AVRC16-AVRC17 are 2/2. |
| `bash scripts/verify-docs-app-verification.sh --self-test` | Pass. All 24/24 mutations fail closed. |
| Bash syntax and ShellCheck | Pass with no diagnostics. |
| Live Firebase Tasks under Node.js 22 | Pass. 5/5 assertions and a matching source-byte finalizer. |
| Live Cloud Functions Tasks under Node.js 22 | Pass. 2/2 assertions and a matching source-byte finalizer. |
| `bash scripts/check-docs.sh` | Pass. 109 pages. |
| `bash scripts/verify-nimbus-docs-site.sh` | Pass. 17/17 conditions. |
| Added Markdown and Rust comment prose | Pass. Technical-writing delta lint reports 0 diagnostics. |
| Full changed Markdown files | `UNVERIFIED`: the two files have an 85-diagnostic pre-existing writing baseline. AVRF25 routes the full-file cleanup to AVR10. |
| `git diff --cached --check` | Pass. |

## Residual boundary

The new flag changes only `nimbus dev`. `nimbus start`, `nimbus compose`,
explicit `--compose-file`, and default dev discovery keep their current
contracts. The application lane still shares ports and operator state. AVR7
owns those lifetime and isolation seams.
