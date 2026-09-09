# UIR18 Function error class

Date: 2026-09-08
Worktree: `~/src/github.com/nimbus/nimbus-worktrees/nimbus-ui-rebuild`, branch `codex/nimbus-ui-rebuild-phase4` (stacked on #333)

Commit `f22da425b`. A handler's own `throw` reached the console as
`service.internal` with the operator-investigation copy, and the message
and the stack were gone (finding F1, `ws-protocol-150`). Root cause: the
generated `__nimbusInvoke` wrapper rethrew the remapped error, the bridge
turned it into `NimbusRuntimeError::JavaScript`, and the server mapped
that to `Error::Internal`. The throw now has its own class on every hop:
the wrapper answers `function_thrown`, the core carries
`Error::FunctionThrown`, the public envelope is `function.thrown`, the
socket carries the same envelope, the run row keeps the stack, the SDK
decodes it, and the console renders it as the developer's error.

## What changed

| Item | Result |
| --- | --- |
| `packages/codegen/src/emit/runtime_remap.mjs` | `nimbusRemapHandlerError` coerces to an `Error`, then marks a handler throw with `nimbusFunctionThrown` and `nimbusOriginalStack` on every return path. A host error (`nimbusHostError`) keeps its identity and is never marked. The marker is a nested arrow because the function is embedded through `toString()`. |
| `packages/codegen/src/emit/runtime_bundle_dispatch_global_invoke.mjs` | The generated catch answers `{status: "error", error: {kind: "function_thrown", function_path, message, stack}}` for a marked error. Contract errors (missing function, visibility gate) still rethrow and stay `service.internal`. |
| `packages/codegen/src/planner/args_proxy.mjs`, `compile_time_interpreter.mjs` | Planner finding, fixed under this task: the compile-time interpreter folded an argument-derived value in a condition, a `??` default, a comparison, or a unary operand to a constant, so `if (text.length === 0) throw` compiled to a bare insert plan and the guard never ran. `isArgumentDerived` probes the args proxy through a symbol; `assertCompileTimeValue` throws a plain error, which is the existing "runtime-only resolver logic" fallback, so such handlers keep a `runtime_handler` and `plan: null`. |
| `packages/codegen/src/selftest/runtime_remap_fixtures.mjs` (7 cases), `runtime_fixtures.mjs` | Remap cases assert the marker, the original stack, host-error passthrough, and string coercion. `testThrownHandlerErrorEnvelopeFixture` compiles a guarded `messages:send`, asserts the runtime fallback (`runtime_handler_line === 9`), and invokes it in the worker realm: `{text: ""}` answers `function_thrown` with `"Message text must not be empty (at messages:11)"` and a stack; `{text: "hello"}` answers `ok`. |
| `crates/nimbus-core/src/error.rs` | `Error::FunctionThrown { function_path, message, stack }`: terminal, deterministic user error, constructor `function_thrown`. |
| `crates/nimbus-convex/src/host_bridge/responses.rs` | `ConvexRuntimeEncodedError::FunctionThrown` (`kind: "function_thrown"`) with a round-trip test. |
| `crates/nimbus-bridge/src/responses.rs` | `RuntimeHostPublicError` code `function.thrown`, detail `{functionPath, stack}`, remediation `fix_function`. |
| `crates/nimbus-server/src/error_envelope.rs` | `function.thrown`: 422, severity `error`, not retryable, `with_request_id`. `ErrorSeverity` derives `PartialEq, Eq`. |
| `crates/nimbus-server/src/protocol.rs`, `subscriptions/socket/named_subscriptions/{mod,direct,runtime_backed}.rs` | `ServerMessage::request_core_error` puts the public envelope in `op.error`, so a socket request error carries the same code, detail, remediation, and request id as HTTP. |
| `crates/nimbus-system/src/records/run.rs`, `nimbus-server` function routes | `RunError { message, stack }` and `from_core_error`: run rows store `error: {message, location?, stack?}`. `RunTrace::record` takes the `nimbus_core::Error`; the paginated query handler keeps the core error until after the record. |
| `crates/nimbus-server/src/tests/convex_functions/runtime_writes/thrown_errors.rs` (new) | `convex_named_mutation_reports_thrown_function_errors_with_message_and_stack`: a bundle that mirrors the generated wrapper; asserts the 422 envelope (code, message, `retryable false`, `severity "error"`, `detail.functionPath`, `detail.stack`, `remediation.action "fix_function"`, `requestId`), reads the run row through the engine (`status "error"`, `error.message`, `error.location "messages:12"`, `error.stack`), then a valid call answers 200. |
| `packages/nimbus/src/browser.ts`, `selftest.mjs` | `handleSocketMessage` decodes `op.error` and `error` through `decodeNimbusErrorEnvelope`, so a socket request error is a `NimbusError` with code, detail, remediation, and request id instead of a bare message. The selftest asserts a `function.thrown` envelope keeps message, `detail.stack`, `detail.functionPath`, `retryable false`, and `remediation.action`, and that a bare socket error keeps the fallback message. |
| `src/components/function-runner/function-runner.tsx` | `RunResult` error carries `functionPath` and `stack` from `detail`. The card for `function.thrown` (`data-error-class="function"`) shows a `threw` pill, "<path> threw. The message and the stack below are the function's own.", the message, a **Stack** disclosure, and **View runs** through the new optional `onOpenRuns` prop; the server remediation line is off. Every other code (`data-error-class="service"`) keeps the remediation copy and shows no stack. The runner stays router-free; the page passes `onOpenRuns` (navigate to `?tab=runs`). |
| `src/lib/run-error.ts`, `src/components/run-panels.tsx` | `parseRunError` returns `stack` (non-empty string only); `RunErrorPanel` folds it under a **Stack** disclosure (`<testid>-error-stack`) on the run page and the run sheet. |
| `docs/reference/native/errors.md`, `DESIGN.md` | `function.*` namespace and the `function.thrown` row; Function Runner section describes the two cards. |

## Spec notes

- `function-runner.spec.tsx` (+2): "renders a thrown-error card with the
  function path, message, stack, and a runs link" and "keeps the
  operator copy and no stack for a service fault" (`service.internal`
  keeps "contact the operator", no stack, no runs button).
- `run-error.spec.ts` (+1): the stack survives; empty, non-string, and
  string errors give none.
- `runs.spec.tsx`: the run-sheet fixture carries a stack and the sheet
  shows it under `observability-run-sheet-error-stack`.
- Fail-before: `$S/uir18-fail-before.txt` records the Rust test failing
  while the throw mapped to `service.internal` (500, no message). The new
  codegen runtime fixture first answered `status: "ok"` for empty text,
  which exposed the planner finding above.
- Net: 121 files, 1048 tests (1045 at UIR17).

## Verification

| Check | Result |
| --- | --- |
| `cargo test -p nimbus-server convex_named_mutation_reports_thrown_function_errors` | 1 passed |
| `cargo test -p nimbus-server error_envelope`, `runtime_writes`, `named_subscriptions` | 9, 11, 1 passed |
| `cargo test -p nimbus-server protocol` | 9 passed, `workload_composition::tests::protocol_only_profile_owns_no_workload_authority` failed once while the e2e suite and a second cargo run shared the host, then passed alone (0.16 s); unrelated to this change |
| `cargo test -p nimbus-core error`, `-p nimbus-convex responses`, `-p nimbus-bridge` | 12, 9, 30 passed |
| `cargo test -p nimbus-system` | 83 passed, 3 failed: `projection::reconciliation_tests::*` panic in `nimbus-storage/src/provider_test_fixtures.rs:287` because the external-provider fixture env is not pinned on this host (`make test-external-provider PROVIDER=…`); pre-existing and environmental, the crate change is `records/run.rs` only |
| `npm test -w packages/codegen` | exit 0 (remap fixtures 7 cases, runtime fixtures incl. the new thrown-envelope fixture) |
| `npm run test -w packages/nimbus` | exit 0 |
| `cargo fmt --all --check`, `make check`, `make clippy` | clean |
| `npm run lint` (313 files) | clean, 1 pre-existing warning |
| `npm run typecheck` (nimbus-ui) and root `npm run typecheck` | clean |
| `npm run test` | 121 files, 1048 passed |
| `npm run build`, `npm run storybook:build` | ok |
| `make build` | ok, 1m 36s |
| `npm run test:e2e` | 24 passed, 1 skipped |
| `verify.sh` | 27 ok, 0 failing |
| Manual acceptance (scripted, `$S/uir18-acceptance.mjs`) | `nimbus dev --once` on `examples/nimbus/agent-chat` with the rebuilt binary in a scratch HOME; session set through the request API (no token typed into a page). 13 checks passed: the wire answers 422 `function.thrown` with "Message text must not be empty (at agent:41)", `detail.stack`, `detail.functionPath: "agent:send"`; the runner card has class `function`, the message, the path, the stack, no operator copy; **View runs** opens the Runs tab with the error row; the run page shows the message and the stack; `{text: "hello"}` answers 200. |

Screenshots at 1280×720 from the acceptance walk, tenant `demo`:
`UIR18-thrown-card.png` (the runner card with the stack open),
`UIR18-runs-tab.png` (the function's Runs tab after **View runs**),
`UIR18-run-page.png` (the run page with the stack open).

## Open items

- The stack frames name the host bundle path (`file:///…/bundle.mjs`)
  and the generated wrapper frames (`nimbusWrapRuntimeInvoke`,
  `executeMutationDefinition`). A source-mapped, wrapper-trimmed stack
  is a codegen task; the message already carries the `(at module:line)`
  location.
- The planner fix covers conditions, defaults, comparisons, and unary
  and binary operands. Argument-derived values reaching the request
  markers of http actions were left as they are.
- The `nimbus dev` acceptance run rewrites the example `package.json`
  and the root lock file to point at the embedded package; both were
  restored before the commit.
- The Overview tab's "Last status" reads `never run` right after a run
  until the run row lands; the card and the Runs tab are live.
