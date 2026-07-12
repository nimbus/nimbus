# Runtime Guest Trust-Global Hardening — Security Plan

Status: `proposed` (security; promote to `active` when scheduled)
Owner branch: TBD (own PR, not the examples PR)
Origin: surfaced by the adversarial review of the Convex-runtimes-parity work in
`examples-and-target-resolution-plan.md` (Band EX10R/EX10R2/EX10R3, 2026-07-12).

## The vulnerability class

Nimbus's runtime bootstrap installs a set of `globalThis.__nimbus*` functions
that the **trusted, codegen-emitted** invocation preamble calls on every
invocation (context construction, host-call transport, nested dispatch, lane
resolution). Several are installed by **plain assignment** and are therefore
guest-reassignable: a guest handler body (compiled via `new Function`, sloppy
mode, resolving free identifiers against the realm global env) can do
`globalThis.__nimbusX = impostor`, and the trusted preamble then calls the
impostor. `Object.freeze(globalThis.__nimbusX)` (used in a few places) freezes
the function *object*, NOT the property slot — reassignment still succeeds.

**Amplifier — warm-pool reuse.** Isolates are reused across invocations
(warm pool). A global reassigned by a guest in invocation N persists into
invocation N+1's trusted preamble on the same isolate, turning a
single-invocation tamper into **cross-invocation / cross-tenant** exposure.

This class recurred four times during the EX10 review (plain global → bare
`let` binding → registrar → transport), each patched one property at a time
while a sibling with the identical defect sat untouched. This plan does the
systematic sweep instead of another one-off.

## Already hardened (reference — do not regress)

- `__nimbusRuntimeEnvironmentLane` — frozen (`deno_runtime_globals.js`).
- `__nimbusCallDetachedFromInvocationContext` — `Object.defineProperty` frozen.
- `__nimbusSyncHostValue`, `__nimbusAsyncHostValue` — frozen (EX10R3.3, examples
  PR commit `0b8e4152c`; this PR made the sync transport load-bearing for the
  host-side lane oracle, so it was hardened there).
- The callee-lane lookup: the guest-reachable JS lookup/registrar was REMOVED
  entirely; the callee lane is resolved host-side (EX10R3.1, `fa73b6a3a`).

## Known open instances (from the 2026-07-12 sweep)

| ID | Global | Severity | Mechanism | Where |
| --- | --- | --- | --- | --- |
| HG1 | `__nimbusCreateContext` | HIGH | Plain assign + `Object.freeze(value)` only; builds the entire `ctx` (db/scheduler/runQuery/mutation/action/auth) every invocation across every adapter. Reassign in N → warm-pool N+1 preamble calls impostor → forge db/auth, exfiltrate a later invocation's request/args/auth (cross-invocation/tenant) | install `nimbus_context_contract.js:272`, freeze `:599`; consumer `packages/codegen/src/emit/runtime_bundle_preamble.mjs:41` |
| HG2 | `__nimbusInvokeNamedLocal` | MEDIUM-HIGH | Plain assign, never hardened; read fresh on every nested ctx.run* local dispatch. Reassign mid-invocation → "local dispatch" runs attacker code instead of the real callee (forges nested-call result); no warm-pool needed | `packages/codegen/src/emit/runtime_bundle_dispatch_global_invoke.mjs:32`; consumer in `__nimbusRunNamedFunction` |
| HG3 | `__nimbusCoreOps` / `Deno.core.ops` | UNKNOWN — needs native check | The `const __nimbusCoreOps = Deno.core.ops` binding can't be reassigned, but if the ops object's own PROPERTIES are writable (standard deno_core), `__nimbusCoreOps.op_nimbus_ctx_resolve_callee_lane = spoof` forges the oracle through the now-frozen transport. No `Object.freeze/seal` on it anywhere in JS. Confirm whether the native ops-table install seals it; if not, `Object.freeze(__nimbusCoreOps)` defensively (flat name→fn map, no legit post-bootstrap mutation) | `deno_host_call_transport.js:1` |
| HG4 | (info-leak, was EX10R3.4) `op_nimbus_ctx_resolve_callee_lane` visibility | LOW | `runtime_environment_for_function` does existence + handler/plan checks, no visibility check → guest can probe existence+lane of internal functions. Strict subset of the pre-existing `ctx.run*`-on-unknown-name error-text oracle | `crates/nimbus-convex/src/registry/resolution/functions/runtime_access.rs` |

## Approach

1. **Enumerate** every `globalThis.__nimbus*` property installed anywhere in the
   bootstrap JS tree (and codegen-emitted preamble/dispatch). Classify each:
   trust-relevant (the trusted preamble/host relies on it) → MUST be
   `Object.defineProperty` `{writable:false, configurable:false}`; or
   intentionally guest-mutable → leave writable with a comment stating why.
2. **Harden** HG1, HG2 the same way HG-transport was fixed. Resolve HG3 (native
   sealing check, then freeze if needed). Consider a visibility-scoped lane
   oracle for HG4 or accept parity with the existing leak.
3. **Structural test** that asserts every trust-relevant global is
   non-configurable+non-writable, so a future addition can't silently
   reintroduce the class (the review's key recommendation — stop relying on
   review rounds to catch each instance).
4. **Threat model** doc: enumerate the trusted-preamble → guest-code trust
   boundary, the warm-pool cross-invocation amplifier, and the invariant
   ("no guest-reachable global may drive a trust decision or be called by the
   trusted preamble unless frozen").

## Notes

- These are largely PRE-EXISTING on `main` (the reassignable globals predate the
  examples PR). The examples PR hardened only the surface it made newly
  load-bearing (the sync transport / lane oracle) and split the rest here by
  owner decision (2026-07-12) rather than growing that PR into a full security
  audit.
- Warm-pool reuse makes HG1 a genuine cross-tenant data-exposure risk — treat
  this plan as launch-relevant security, not cleanup.

## Suggested Goal Prompt (when promoted)

/goal Execute docs/private/plans/runtime-guest-trust-global-hardening-plan.md: enumerate every globalThis.__nimbus* install in the bootstrap JS + codegen preamble/dispatch, classify trust-relevant vs intentionally-mutable, defineProperty-harden HG1 (__nimbusCreateContext) and HG2 (__nimbusInvokeNamedLocal) with red-then-green adversarial tests (guest reassigns the global from a new Function body → the trusted preamble must not call the impostor; cover warm-pool cross-invocation for HG1), resolve HG3 by checking whether Deno.core.ops properties are natively sealed and freezing __nimbusCoreOps if not, add a structural test asserting every trust-relevant global is non-writable+non-configurable, and decide HG4. Run make test-rust-runtime + make test-rust-workspace green. The goal is met when every HG row is done/decision-recorded with red-then-green evidence, the structural test passes, and an adversarial re-review confirms no guest-reachable global drives a trust decision — or stop after 40 turns and record the blocker.
