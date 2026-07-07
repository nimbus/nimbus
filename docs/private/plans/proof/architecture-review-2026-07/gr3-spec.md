# GR3 Spec — durable-before-response decision logging, fail closed

Design authority: `docs/private/plans/architecture-review-2026-07-plan.md`
GR3 + the 2026-07-06 logger-plumbing inventory. Crate scope:
`nimbus-proxy` (+ the `nimbus-sandbox` OCI wiring site + a readiness
field consumers see). Pre-launch: breaking changes preferred.

## Facts this rests on

- `DecisionLogger = Arc<dyn Fn(EgressDecisionLog)>` (decision_log.rs:13)
  is return-less; `AppendOnlyDecisionLogSink::logger()`
  (decision_log.rs:250-257) swallows `append`'s `io::Result` with an
  `eprintln!`. Appends are synchronous+inline on the request task
  (fanout.rs:17-24 documents this).
- Deny/malformed/upstream-error terminals emit BEFORE
  `write_http_response_async`; but the ALLOW paths (plain-HTTP forward
  via `NimbusForwardApp::logging` pingora_app.rs:233-238, CONNECT splice
  worker.rs:687, intercept :747) emit AFTER the client already received
  bytes.
- `emit_terminal_log` (worker.rs:1043) records the `TerminalLog` phase
  BEFORE invoking the sink, so a failed write is indistinguishable from
  success to `AbortTerminalGuard` (worker.rs:1013) and the intercept
  re-emit checks.
- `WorkloadPepReadiness { ready, policy_generation }`
  (policy_state.rs:20) is policy-liveness only; the sole functional
  consumer is `OutboundEgress::authorize_outbound`
  (nimbus-services/src/outbound.rs:95) which fail-closes
  enforcement-required egress when `!ready`.
- The OCI wiring (nimbus-sandbox/src/backends/oci/egress.rs:131-181)
  builds the sink, wraps it in `fan_out_decision_loggers` with a
  best-effort counter sink, and installs via `with_decision_logger`.

## Target design (normative)

Split DURABLE AUDIT COMMIT from TERMINAL TELEMETRY:

1. **New fallible durable seam.** `WorkloadPepConfig` gains
   `with_durable_decision_sink(sink: DurableDecisionSink)` where
   `DurableDecisionSink = Arc<dyn Fn(&EgressDecisionLog) -> io::Result<()> + Send + Sync>`
   (or an equivalent small trait — your choice, keep it one method).
   `AppendOnlyDecisionLogSink` provides it directly (stop wrapping the
   append sink into the return-less fan-out; `logger()`'s swallowing
   closure is DELETED). The existing `DecisionLogger` fan-out remains for
   best-effort telemetry sinks only (counters), unchanged in type.
2. **Durable-before-response, all paths:**
   - DENY / MALFORMED / UPSTREAM-ERROR terminals: commit the durable
     write first; only on `Ok` write the client response. On `Err`:
     mark the audit unhealthy (below), emit nothing to the client
     (close the connection without a response), still record the
     TerminalLog phase and fire best-effort telemetry.
   - ALLOW plain-HTTP forward: commit the durable allow record (it is
     already fully built pre-forward, worker.rs:869) BEFORE handing off
     to Pingora / before `Forward`. On `Err`: treat as a deny-terminal
     with no client response (upstream must never be contacted). The
     post-response `logging` callback keeps firing the best-effort
     telemetry exactly as today.
   - ALLOW CONNECT splice (worker.rs:644) and intercept: same — durable
     commit before the splice/relay begins; on `Err`, abort before any
     upstream byte flows.
3. **Exactly-once + failure visibility:** introduce a
   `durable_recorded` flag beside `terminal_seen` on the phase
   recorder (or an adjacent atomic): set ONLY after a confirmed `Ok`
   append. `AbortTerminalGuard::drop` attempts the durable deny-write
   only when `!durable_recorded`, and on failure flips audit health (it
   cannot deny — there is no client — that is acceptable and documented).
   Do NOT change `REQUEST_PHASE_ORDER` or the meaning/ordering of the
   `TerminalLog` phase — the pinned phase-model tests must stay green
   unmodified; the durable commit is not a phase.
4. **Audit health in readiness:** `WorkloadPepReadiness` gains
   `pub audit_healthy: bool`, wired from a shared `AtomicBool` the
   durable sink path flips on first failure (sticky until process
   restart; document). `ready` itself becomes
   `policy_ready && audit_healthy` so the existing consumer
   (outbound.rs:95) fail-closes with zero changes; keep
   `policy_generation` semantics untouched.
5. **Wiring:** the OCI site passes the append sink as the durable sink
   and keeps only best-effort sinks in the fan-out. Open-time failure
   behavior (fail the launch) is already correct — unchanged.

## Hard constraints

- No client-visible response may be produced after a FAILED durable
  append, on any path. No upstream connection may be initiated after a
  failed pre-forward commit.
- The durable append stays synchronous/inline (no channel, no buffer) —
  that is what makes durable-before-response true. Do not "optimize"
  with async here.
- Pinned tests in `src/tests/decision_log_phase.rs` (phase orderings,
  exactly-one-terminal, redaction) must pass UNMODIFIED except where a
  test constructs a logger fixture whose type changes — signature-level
  updates only, never assertion weakening.
- Error strings must stay tenant-safe (path is fine; no request bodies).

## Required tests (all asserting observable behavior)

1. Failing durable sink, deny path: client connection closes with NO
   response bytes; audit_healthy flips false; readiness.ready false.
2. Failing durable sink, allow plain-HTTP path: upstream test server
   records ZERO requests; client gets no response; health flipped.
3. Failing durable sink, CONNECT path: no splice bytes reach upstream.
4. Healthy sink: exactly one durable JSONL line per request on allow AND
   deny (extend the existing append-sink test), phase-order suite green.
5. Cancellation with failing sink: abort-guard attempt flips health
   (no client assertion — none exists).
6. Readiness unit test: audit_healthy=false ⇒ ready=false even with a
   live policy generation.

## Verification gates (worktree root, in order)

```
cargo fmt --all --check
cargo clippy -p nimbus-proxy -p nimbus-sandbox --all-targets -- -D warnings
cargo test -p nimbus-proxy
cargo test -p nimbus-sandbox
cargo test -p nimbus-services      # outbound readiness consumer
cargo check -p nimbus-server
```

Report real per-suite counts. If the sandbox KVM-gated suites skip
locally, say so explicitly (CI owns them).

## As built (PR #134, squash-merged `19ac348b9`, 2026-07-07)

The landed design implements this directive in full and then extends it
substantially: fifteen adversarial review passes (Codex) each produced a
verified audit-contract counterexample, and the fixes grew the change
from "thread an `io::Result`" into a complete durable-audit
architecture. 19 files, +3,884/−236.

### Directive items, as landed

- `DurableDecisionSink = Arc<dyn Fn(&EgressDecisionLog) -> io::Result<()>>`
  on `WorkloadPepConfig`; `AppendOnlyDecisionLogSink` provides it and
  its result-swallowing `logger()` closure was deleted. The return-less
  `DecisionLogger` fan-out remains for best-effort counter sinks only.
- Durable-before-response on every terminal path;
  deny/malformed/upstream-error commit before any client bytes; the
  allow paths commit BEFORE Forward / upstream dial / splice /
  intercept, so an executed egress is always preceded by a durable row.
  Appends stay synchronous and inline.
- Sticky `audit_healthy: AtomicBool` folded into
  `WorkloadPepReadiness.ready` (= `policy_ready && audit_healthy`), so
  the outbound consumer fail-closes with zero changes — AND enforced
  in-path: once the sink fails, new requests are refused before
  upstream contact.
- `REQUEST_PHASE_ORDER` and `TerminalLog` phase semantics unchanged;
  durable state is tracked as phase-recorder flags
  (`mark_durable_any_recorded` / `mark_durable_terminal_recorded`), and
  the pinned phase-model tests passed unmodified.

### Extensions the review passes forced

- **Record model.** One durable row per request was insufficient to
  audit "allowed and then executed/failed". As built:
  `DecisionRecordKind { Intent, Terminal, TerminalAfterResponse }` —
  two rows per plain request (forward-intent + terminal), THREE for
  intercepted HTTPS (outer CONNECT intent; inner request intent
  carrying the redacted URL + credential identity, appended before
  upstream contact and fail-closed on append failure; terminal). All
  rows for a request share a `request_id` (`RequestIdGenerator`:
  RandomState-derived 16-hex prefix + AtomicU64).
- **Pre- vs post-response classification.** A terminal after the client
  received bytes must not masquerade as a pre-response deny.
  `ResponseStartedSignal` is armed only at a CONFIRMED downstream write:
  `FinalHeadWriteScanner` inside `PrereadStream` observes actually
  written bytes and fires on a final (>= 200) or `101` status line;
  `disarm()` on pre-response terminals; `AbortTerminalGuard` synthesizes
  a deny only when no durable terminal exists; post-response failures
  record `TerminalAfterResponse` (`relay_failed_after_response`).
- **Fail-closed upstream-head classification (intercepted HTTPS).**
  Successive passes showed 101 handling and then a status-line
  parser-differential class (non-UTF8 obs-text in headers, then in the
  reason phrase, then HTAB separators, then exotic whitespace, then
  non-HTTP/1 version tokens). Closed ARCHITECTURALLY rather than by
  byte enumeration: `classify_upstream_status` returns
  Final(2xx–5xx) / Informational(100, 102–199) / Upgrade(101) /
  Unrecognized; the relay forwards ONLY Final/Informational and denies
  Upgrade and Unrecognized — a head the proxy cannot positively
  classify is never shown to the client. `response_status` is strict:
  version token must be `HTTP/1.0`/`HTTP/1.1`, status code exactly
  three ASCII digits, SP/HTAB token split, only bytes before the first
  CRLF read, reason phrase never decoded. Informational responses are
  bounded (`MAX_INFORMATIONAL_RESPONSES = 8`).
- **Decomposition.** `worker.rs` split; the durable/terminal/abort
  lifecycle moved to `terminal.rs` (both under the 1,500-line
  threshold).

### Evidence

- All six required test families exist, plus regressions for every
  review-pass finding (after-response ordering, request correlation,
  three-row intercept pairing by `request_id`, 1xx cap, 101 fail-closed
  on both paths, parser fail-closed classes incl. VT/FF/CR and `ICY`).
- Gates: `cargo fmt --all --check`; clippy `-D warnings`
  (proxy + sandbox); 130 nimbus-proxy + 334 sandbox/services tests;
  `cargo check -p nimbus-server`.
- autoreview (Codex): 15 passes, the last clean
  ("patch is correct (0.88)").
- CI: one flaky `nimbus-engine` materialized-serving concurrency test
  on shard 2/3, attributed (5/5 green locally, crate untouched by this
  change), rerun green; merged on confirmed-green verdict.
