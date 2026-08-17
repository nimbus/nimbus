# AVR6 Bare-Local Target Resolution

Date: 2026-08-17

## Result

AVR6 is complete in work commit
`390bcaf27b3b7b458c951ff867aa5252be995714`. Explicit and omitted local
targets now use the same application invocation path. The omitted-target path
reads only the local discovery record. It does not load or send the host admin
token.

Authenticated local administration still uses `LocalServerHttpClient`. That
client now sends the host credential only in `X-Nimbus-Admin-Token`. It never
places the credential in `Authorization`. Convex application authentication
continues to own bearer credentials. The change keeps the host-admin and
application trust planes separate.

## Fail-before evidence

| Case | Result before AVR6 |
| --- | --- |
| Live omitted target | The Convex Tasks smoke passed 5/5 through an explicit target. The same invocation without a target failed with `401 Unauthorized: no Convex auth providers are configured for silo demo`. |
| Isolated regression | `isolated_legacy_admin_bearer_reproduces_fail_before_unauthorized` creates case-local discovery and admin-token files, simulates the old bearer request, and proves that it is refused as application authentication. |
| Root cause | `LocalServerHttpClient` reused the host admin token as an application bearer. Convex correctly interpreted that credential in the application-authentication plane and rejected it. |

## Acceptance ledger

| Action | Result | Evidence |
| --- | --- | --- |
| AVR6.1 Reproduce in isolated operator state. | Pass. | The live run reproduced the 401. The isolated regression captures the exact legacy `Authorization: Bearer <host-token>` request and its permission-denied result. |
| AVR6.2 Repair the credential seam. | Pass. | Bare-local discovery reads only the discovery record. Explicit and omitted targets share `invoke_remote_run_function`. Admin requests use only the dedicated admin header. |
| AVR6.3 Remove the workaround. | Pass. | The runner invokes the bare-local form directly. Its comment no longer describes an explicit-target authentication workaround. |
| AVR6.4 Prove result, stdio, and trust behavior. | Pass. | Explicit and omitted forms return deep-equal JSON. Stdout contains JSON only, banners stay on stderr, a wrong silo fails with empty stdout, and an invalid application bearer returns HTTP 401. |

## Verification evidence

| Command or check | Result |
| --- | --- |
| `cargo test -p nimbus-cli run::tests` | Pass. 11 passed, 0 failed. |
| `cargo test -p nimbus-cli local_server_client::tests` | Pass. 2 passed, 0 failed. |
| `cargo test -p nimbus-cli --lib` | Pass. 1,019 passed, 0 failed, 4 ignored. |
| `cargo clippy -p nimbus-cli --all-targets -- -D warnings` | Pass. Only upstream vendored Brotli warnings were emitted. |
| `cargo fmt --all --check` | Pass. |
| `cargo build -p nimbus-bin --bin nimbus` | Pass. |
| `bash scripts/examples-verify-contract-test.sh --task AVR6` | Pass. AVRC18 is 1/1. |
| `bash scripts/verify-docs-app-verification.sh --task AVR6` | Pass. AVRC18 is 1/1. |
| `bash scripts/verify-docs-app-verification.sh --self-test` | Pass. All 24/24 mutations fail closed. |
| Bash syntax and ShellCheck | Pass with no diagnostics. |
| Live Convex Tasks under Node.js 22 | Pass. The application passed 5/5 smoke assertions. Both target forms returned the same JSON, kept stdout clean, and wrote banners only to stderr. Wrong-silo and invalid-bearer requests failed closed. The source-byte finalizer matched. |
| PR #238/#239 trust regressions | Pass. Four named tests passed 4/4: both Cloud Functions tenant-binding tests and both Convex silo-authentication tests. |
| `git diff --check` | Pass. |

## Residual boundary

AVR6 changes only local command discovery and credential routing. It does not
weaken trusted tenant binding, Convex silo selection, or public application
authentication. The application lane still shares some operator roots and
resource lifetimes. AVR7 owns case-local authentication, discovery, audit,
data, control, log, and cleanup state, plus listener and process ownership.
