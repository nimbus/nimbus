# AVR7 Per-Case Resource Lifetime

Date: 2026-08-17

## Result

AVR7 is complete in work commit
`bd2a8a36496275e65672917f4e7d17983feff7a5` and review-correction commit
`2215a57726a12abc59e475e05e60b949ee1afda2`. The examples runner now has one run
lifetime owner and one process-group owner. Nimbus owns every listener and port
lease.

Each run uses one network-state root. Each case uses separate application,
data, control, authentication, discovery, audit, log, result, and process
roots. Discovery accepts only the expected process identity and a loopback
non-zero endpoint.

The lifetime owner removes successful runs. It retains failed runs in an
owner-only artifact root. A cleanup failure keeps the original root and makes
the run fail. The process owner sends TERM to the full process group, waits,
and uses KILL only after the grace period.

## Fail-before evidence

| Case | Result before AVR7 |
| --- | --- |
| Main listener | The shell opened port zero, read the assigned port, closed the socket, and passed the released port to Nimbus. Another binder could win that gap. |
| Wire listeners | Dev-mode cases used the conventional MongoDB, DynamoDB, and S3 port preferences. The runner did not request provider-assigned sibling listeners. |
| Operator state | Every case inherited the same host paths for authentication, discovery, audit, and configuration. |
| Process lifetime | The shell tracked only the direct server PID. Cleanup used best-effort `kill` and `wait`, which did not own descendants. |
| Temporary root | The runner created `DATA_ROOT` with `mktemp` and did not remove it on success. |
| Failure evidence | The EXIT path did not retain one named artifact with process, listener, and cleanup state. |
| Fault coverage | The runner had no cuts after root creation, case creation, process spawn, readiness, smoke start, or before stop. |

## Acceptance ledger

| Action | Result | Evidence |
| --- | --- | --- |
| AVR7.1 Inventory listeners. | Pass. | The manifest names each surface. Start cases disable unused listeners. Dev cases use their declared surfaces. The main and wire paths have named product owners. |
| AVR7.2 Disable unused surfaces. | Pass. | Start adds each `--no-*` flag from the surface set. Dev planning preserves its adapter surface decisions. |
| AVR7.3 Consume provider-assigned leases. | Pass. | Every boot passes `--port 0`. Dev treats this as provider-assigned-only for sibling listeners. The runner contains no socket probe or port allocator. |
| AVR7.4 Define global and local roots. | Pass. | One run context creates the network root. Each case context creates distinct application and operator roots. Collision after identifier normalization fails closed. |
| AVR7.5 Propagate case context. | Pass. | Server, codegen, smoke, and CLI subprocesses clear ambient `NIMBUS_*` values and receive the exact case paths. Wire values come only from validated Nimbus-owned `.env.local` keys. |
| AVR7.6 Own process and cleanup state. | Pass. | Detached process groups have atomic records. Corrupt records fail before spawn. Record-write failure settles the unrecorded child. Graceful shutdown keeps the admin token out of command arguments. |

## Verification evidence

| Command or check | Result |
| --- | --- |
| `node scripts/examples-verify-workspace-test.mjs` | Pass. 9 top-level cases and all 9 preparation fixtures passed. The external-package case replaced a same-version source link with case-local bytes. |
| `node scripts/examples-verify-lifetime-test.mjs` | Pass. 11/11 groups covered cross-case sentinels, concurrent binds, external binders, process trees, six cuts, cleanup retry, cross-filesystem retention retry, secret-free supervisor argv, artifact retention, post-spawn record failure, and corrupt records. |
| `node scripts/examples-verify-product-lifetime-test.mjs` | Pass. 1/1 real-product concurrency case. Two servers shared one network authority, used distinct endpoints and tokens, shut down cleanly, and released both ports. |
| `node scripts/examples-verify-runner-fault-test.mjs` | Pass. 6/6 real-runner cuts. Every cut was red, retained evidence, removed discovery and process records, released the observed socket, and left only terminal lease phases. |
| `bash scripts/examples-verify.sh` | Pass. All 9 applications and 37 smoke assertions passed. The Convex Tasks target-form contract also passed. Source bytes matched and no run artifact remained. |
| `cargo test -p nimbus-network port_lease` | Pass. 120 passed, 0 failed, 128 filtered. |
| `cargo test -p nimbus-server listener_lease` | Pass. 24 passed, 0 failed, 674 filtered. |
| `cargo test -p nimbus-cli` | Pass. 1,021 passed, 0 failed, 4 ignored. |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-system` | Pass. 85 passed, 0 failed. |
| `make clippy` | Pass for the complete workspace. Only allowed vendored Brotli warnings appeared. |
| `cargo fmt --all --check` | Pass. |
| `bash scripts/examples-verify-contract-test.sh --task AVR7` | Pass. AVRC19-AVRC20 are 2/2 and the lifetime groups are 11/11. |
| `bash scripts/verify-docs-app-verification.sh --through-phase 2` | Pass. AVRC01-AVRC20 are 20/20. |
| `bash scripts/verify-docs-app-verification.sh --self-test` | Pass. All 24/24 mutations fail closed. |
| Bash syntax, ShellCheck, and Node syntax | Pass with no diagnostics. |
| `git diff --check` | Pass. |
| Plan technical-writing lint | Pass with 0 diagnostics. |
| Public docs | Pass. Link gate 109 pages; site verifier 17/17; website build 110 pages. `docs/reference/cli.md` stayed at its 5-diagnostic baseline. |
| TruffleHog review preflight | Pass. No verified or unknown credential finding remained in the AVR3-AVR7 branch history. |

## Structured review

The complete AVR3-AVR7 candidate received one GPT-5.6 Sol review with xhigh
reasoning and the fast service tier. The review scored `0.98` and reported three
accepted findings.

| Finding | Disposition |
| --- | --- |
| P2: smoke credentials remained in the long-lived supervisor argv. | Fixed in `2215a5772`. The runner writes an owner-only environment file. The supervisor reads it after it validates its type, size, mode, and unique keys. An 11th lifetime test proves the child receives the value and the supervisor argv does not. |
| P3: an EXDEV copy followed by source-removal failure made artifact retry impossible. | Fixed in `2215a5772`. An owner record identifies a completed destination. Retry validates the record, removes the source, and converges. The injected EXDEV and removal-failure test proves both the ambiguous state and retry. |
| P3: the CLI reference omitted `--control-data-dir`. | Fixed in `2215a5772`. The `start` and `dev` tables now define its default and separate control-state purpose. Public docs gates and the site build pass. |

## Rejected or interrupted runs

| Run | Disposition |
| --- | --- |
| First full nine-application run | The execution session vanished. The EXIT owner retained `nimbus-examples-verify.Ggn7Om` and left no process. This was interruption evidence, not a product failure. |
| Second full run | `convex/runtimes` exposed a real linked-package capability-root defect. The case now copies same-version external packages into its own root. The focused case and two later full runs passed. |
| Unscoped `cargo test -p nimbus-system` | Three external-provider tests refused absent pinned fixture URLs. The required local-mode environment produced 85/85. |
| Custom `cargo clippy --all-features` | The V8 pointer-compression guard rejected a mixed shared artifact. The canonical `make clippy` gate passed. No alternate target or clean build was used. |
| First structured-review preflight | The scanner found a credential-shaped synthetic MongoDB URI in an unpublished test commit. The fixture now constructs the URL from separate non-secret values, the local candidate history was rebuilt without the literal, and the scanner passed before Sol ran. |
| Ambient Node.js 26 correction run | The required host preflight refused before application work. The installed Node.js 24 toolchain then passed all nine applications and 37 assertions. |

After diagnosis, the task owner moved the two retained application artifacts
to the macOS Trash. The Trash keeps the move recoverable. No artifact remained
under `target/examples-verify-artifacts` at closeout.

## Residual boundary

The runner remains serial and emits console evidence. AVR8 owns versioned JSON
and JUnit evidence. AVR9 owns bounded scheduling and measured speed. AVR7 does
not add forwarding, policy, network-provider effects, or another port
authority.
