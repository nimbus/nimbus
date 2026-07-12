# Runtime Guest Trust-Global Hardening — Security Plan

Status: `proposed` (security; promote to `active` when scheduled)
Owner branch: TBD (own PR, not the examples PR)
Origin: surfaced by the adversarial review of the Convex-runtimes-parity work in
`archive/examples-and-target-resolution-plan.md` (Band EX10R/EX10R2/EX10R3),
then verified + corrected by an independent Codex source-trace (2026-07-12,
`--effort high`). That pass refuted one impact claim, flagged one unsafe
prescribed fix, and found additional instances; this document reflects those
corrections.

## The vulnerability class

Nimbus's runtime bootstrap installs functions and mutable state that the
**trusted, codegen-emitted** invocation path (and the Rust host) call on every
invocation — context construction, host-call transport, nested dispatch, the
top-level invoke entrypoint. The trust boundary has THREE reachable surfaces,
and the plan must sweep all three (the EX10R rounds only fought the first):

1. **Reassignable global properties.** Installed by plain
   `globalThis.__nimbusX = fn` → guest-reassignable. `Object.freeze(value)`
   freezes the function *object*, NOT the property slot; reassignment still
   succeeds. Only `Object.defineProperty(globalThis, name, {writable:false,
   configurable:false})` closes it.
2. **Mutable global lexical state.** Top-level `let`/`var` bindings and mutable
   objects captured by trusted code — e.g. `__nimbusInvocationGeneration`,
   the `__nimbusCoreOps`/`Deno.core.ops` table's own properties, wait-until
   queues. A frozen property SLOT does not protect a mutable VALUE behind it
   (`__nimbusHiddenDenoGlobals` is a hardened slot over a mutable object graph).
3. **Native op-table properties.** `Deno.core.ops` is an ordinary object with
   writable properties in the pinned deno_core fork (verified: `bindings.rs`
   uses plain `.set`, no freeze/seal). A guest overwrites an op the trusted
   transport reads.

**Amplifier — warm-pool reuse.** Isolates are reused across invocations. A
guest reassignment in invocation N persists into N+1's trusted path on the same
isolate (proven surviving current resets: `snapshot_lifecycle.rs:553`).

**Blast-radius correction (Codex, refuting the original draft):** warm-pool
reuse requires an exact key that **includes bundle identity, and bundle identity
includes tenant identity** (`warm_pool.rs:40,287`; test `warm_pool.rs:96`; the
cross-tenant test requires tenant B to cold-miss, `warm_pool.rs:301`). So this
is **cross-invocation / cross-user / cross-function exposure WITHIN one
tenant/authority partition — NOT cross-tenant.** Still serious (a later
invocation's `request`/`args`/`auth` for a different user or function on the
same tenant is exposed), but describe it precisely everywhere.

This class recurred through the EX10 review (plain global → bare `let` binding →
registrar → transport) because each pass patched one property while a sibling
sat untouched. This plan does the **systematic** sweep + a structural test, so
a future addition can't silently reintroduce it.

## Findings

Severity within the corrected (same-tenant) blast radius. All file:line from the
2026-07-12 Codex trace against main.

| ID | Surface | Sev | Mechanism | Where |
| --- | --- | --- | --- | --- |
| HG0 | `__nimbusInvoke` (**most serious; was missed**) | HIGH | The top-level host entrypoint, plain-assigned; **Rust reads it fresh every invocation and passes the complete request**. Same warm-reuse consequence as HG1 but one level EARLIER in dispatch — a reassignment intercepts the entire request/args/auth of a later same-tenant invocation | consume `crates/nimbus-runtime/src/runtime/invocation.rs:52`; emit `packages/codegen/src/emit/runtime_bundle_dispatch_global_invoke.mjs:3`; Cloud Functions also emits a mutable `__nimbusInvoke` at `packages/codegen/src/cloud_functions/runtime_sources.mjs:35` |
| HG1 | `__nimbusCreateContext` | HIGH | Plain assign (`nimbus_context_contract.js:272`) + `Object.freeze(value)` only (`:599`); builds the whole `ctx` (db/scheduler/runQuery/mutation/action/auth). Preamble reads it fresh per invocation (`runtime_bundle_preamble.mjs:40`; entrypoints `runtime_bundle_execution_entrypoints.mjs:2`). Warm reuse → replacement receives N+1's request/args/auth | see cells |
| HG2 | `__nimbusInvokeNamedLocal` | MED-HIGH | Plain assign (`runtime_bundle_dispatch_global_invoke.mjs:32`), read fresh into `localInvoker` per nested ctx.run* (`nimbus_context_contract.js:194`, invoked `:238`). Reassign mid-invocation → same-lane nested call returns attacker-selected data | see cells |
| HG3 | `__nimbusCoreOps` / `Deno.core.ops` | HIGH | Definitively exploitable: transports read op props fresh (`deno_host_call_transport.js:130,165`); pinned deno_core does NOT freeze/seal the table (`bindings.rs:347,563`, plain `.set`). Overwrite `op_nimbus_ctx_resolve_callee_lane` → forge the value consumed at `nimbus_context_contract.js:233`. **Fix is NOT `Object.freeze` — see below** | see cells |
| HG5 | Cloudflare entrypoint `__nimbusInvokeCloudflareWorkerFetch` | MED | Plain-assigned host entrypoint on the Cloudflare runtime, same reassign-the-entrypoint class | `crates/nimbus-runtime/src/runtime/bootstrap/js/cloudflare_workers_runtime.js:278` |
| HG6 | wait-until lifecycle: `__nimbusWaitUntil` / `__nimbusDrainWaitUntil` / `__nimbusResetWaitUntil` + the `__nimbusWaitUntilQueue` lexical | MED | Host-called lifecycle hooks (reassignable) plus mutable queue state the host drains | `deno_host_call_transport.js:184` + queue at `:1` |
| HG7 | `__nimbusRefreshNodeProcessCwd` and other host-called Node bootstrap hooks | LOW-MED | Host/reset-called, reassignable; classify each | `node22_runtime_bootstrap.js` (Codex list) |
| HG8 | mutable lexical trust state: `__nimbusInvocationGeneration` | MED | Mutable binding the host reset script writes (`reset_bootstrap_invocation_state.js:2`), read as trust/generation state (`nimbus_context_contract.js:270`) — a guest can desync it | see cells |
| HG9 | mutable object graphs behind hardened slots: `__nimbusHiddenDenoGlobals`, `__nimbusHiddenNodeGlobals` | MED | Slots are hardened but the object VALUES are mutable; trusted transpiled scripts consume them (`transpile.rs:29`). A descriptor-only invariant misses value mutation | see cells |
| HG4 | lane-oracle metadata (`op_nimbus_ctx_resolve_callee_lane`) | LOW | `runtime_environment_for_function` checks existence + handler/plan shape, no visibility (`runtime_access.rs:90`; bridge `runtime_calls.rs:140`). **NOT a strict subset** of the error oracle: nested-dispatch errors leak unknown/visibility/kind (`selection.rs:14`) but the lane oracle additionally reveals `default`/`node`/`bun`. Tenant-local deployment metadata, not a demonstrated authz bypass — **threat-model decision required, not "consider visibility"** | see cells |
| HGx | stale implicit global `__nimbusNextHostCallSessionId` | cleanup | Created by undeclared assignment, otherwise unused; remove or justify | `reset_bootstrap_invocation_state.js:1` |

Intentionally guest-mutable / application-singleton (document, do NOT harden):
`__nimbusCloudFunctionsState`, `__nimbusAdminApps` (`runtime_sources.mjs:81,834`),
and the compat/test surfaces (`__nimbusFlushEmbeddedTests`,
`__nimbusNodeRuntimeMajor`, etc.). The classification ledger must state why.

## Already hardened (verified accurate on main — do not regress)

`__nimbusRuntimeEnvironmentLane` (`deno_runtime_globals.js:312`),
`__nimbusCallDetachedFromInvocationContext` (`deno_host_call_transport.js:37`,
captured privately by the contract), `__nimbusSyncHostValue`/
`__nimbusAsyncHostValue` (`deno_host_call_transport.js:126,161`),
`__nimbusInstall/Enter/BeginGuest*` (guest-semantics, block-scoped + hardened,
`nimbus_guest_semantics.js:26,235`), and the callee-lane lookup was REMOVED
(resolved host-side, `runtime_calls.rs:133`). Caveat: the transports are
hardened at the SLOT but still read the unhardened op table (HG3) — not
end-to-end until HG3 lands.

## Fix approach — canonical, root-cause (Codex-corrected)

The strongest ownership boundary is **NOT a frozen `globalThis` property** (that
stops reassignment but still exposes the capability to guests). Prefer, in order:

1. **Module scope / IIFE-private closure / Rust-held V8 handle** — the trusted
   value is unreachable to guest code entirely. Apply to HG2 (pass the
   module-private `invokeNamedDefinitionLocally` into `__nimbusCreateContext`,
   which closes over it — remove the global) and, where feasible, HG0/HG1.
2. **Private captured references** for HG3: capture the exact trusted op
   functions in a private closure AFTER op init, and have transports call the
   captured refs — OR build a private null-prototype capability table from
   selected op refs and freeze THAT (never the live shared `Deno.core.ops`).
   **Do NOT `Object.freeze(__nimbusCoreOps)`**: the pinned fork performs
   deferred fast-call upgrades by overwriting op slots (`bindings.rs:593,690`);
   freezing early disables them.
3. **`Object.defineProperty` slot hardening** ({writable:false,
   configurable:false, enumerable:false}, install the real fn atomically) as the
   minimum where a global must remain — HG0, HG1, HG5, HG6, HG7. Freezing the
   function object is optional defense-in-depth.
4. **Mutable-value protection** for HG8/HG9: the descriptor invariant is
   insufficient; the reset/generation state needs host-owned or closure-private
   storage, and the hidden-globals object graphs need their own review.

HG4: decide, don't hedge. Adding `visibility` to the payload is not a secrecy
control (guests construct `{name, visibility:"internal"}`; internal functions
are intentionally app-callable). Either document lane metadata as non-secret,
or design a uniform failure behavior against an explicit confidentiality
requirement — a threat-model output.

## Structural test — full spec (Codex)

A single test that FAILS if a future trust global ships unhardened:
- After bootstrap AND after bundle load, take a `Reflect.ownKeys(globalThis)`
  inventory of every `__nimbus*` property.
- An explicit **allowlist classification** for every `__nimbus*` property
  (trust-relevant vs intentionally-mutable vs compat/test), so a NEW unlisted
  property fails the test.
- For every trust property: own data descriptor, `writable:false`,
  `configurable:false`, and **stable identity after attempted
  assignment/delete/redefine**.
- Separate static/semantic checks for global **lexical** bindings and for
  trusted **mutable object values** (HG8/HG9) — not just property descriptors.
- Coverage across runtimes: web/default, Node, Cloudflare; and lifecycles:
  main-realm warm reuse AND fresh-realm recycling; both normal and Cloud
  Functions codegen.
- **Red/green exploit tests** for HG0, HG1, HG2, HG3, HG5, and the
  waitUntil/Cloudflare lifecycle hooks (guest reassigns → trusted path must not
  call the impostor / must fail safe).

## Threat model deliverable (concrete)

Identify: principals (guest handler code, trusted preamble, Rust host); protected
data (a later invocation's request/args/auth on the same tenant); the
same-tenant-vs-cross-tenant boundary (warm-pool keys on tenant-scoped bundle
identity — cross-tenant is out of reach, document why); import-time execution and
`new Function` reachability into the realm global env; warm-pool partitioning as
the persistence vector; host-session-id enforcement; and which guest-visible
capabilities are INTENTIONALLY callable (so hardening doesn't break them).

## Verification lanes

Runtime changes: `make test-rust-runtime` + `make test-rust-workspace`. Codegen
changes: the codegen selftests + npm gates (`npm run typecheck/test/build`),
and restage embedded packages before any live check. Closeout on the repo's
`make ci`. JS/codegen bootstrap edits need BOTH the Rust runtime lanes (they
compile the bootstrap JS in) and the codegen gates.

## Revisions already applied (from the 2026-07-12 verification)

Incorporated here: corrected same-tenant blast radius (was cross-tenant); added
HG0 `__nimbusInvoke`, HG5 Cloudflare, HG6 waitUntil, HG7 Node hooks, HG8 lexical
state, HG9 object graphs; the sweep now covers all three surfaces; the
`__nimbusCoreOps` fix replaced with a lifecycle-safe private-op-reference design
(NOT freeze); HG4 reframed as a threat-model decision; and the full
structural-test + threat-model specs. Corrected outside this file: the
plans-README entry and agent memory (both had the "cross-tenant" wording).

## Suggested Goal Prompt (when promoted)

/goal Execute docs/private/plans/runtime-guest-trust-global-hardening-plan.md. First produce the classification ledger (Reflect.ownKeys inventory of every globalThis.__nimbus* + realm lexical trust state, each labelled trust-relevant / intentionally-mutable / compat, with file:line). Then fix each trust surface at the strongest available boundary (module-scope/closure/Rust-held for HG0/HG1/HG2 where feasible; private captured op references for HG3 — never Object.freeze on the live Deno.core.ops, it breaks deferred fast-call upgrades; defineProperty slot-hardening otherwise; host-owned/closure-private storage for HG8/HG9), each with a red-then-green exploit test (guest reassigns the global / mutates the value → the trusted path must not use it). Land the structural test to the full spec (post-bootstrap + post-bundle-load ownKeys inventory, allowlist, descriptor + stable-identity assertions, lexical + object-graph checks, web/Node/Cloudflare + warm/fresh + normal/cloud-functions coverage). Decide HG4 in the threat-model doc. Run make test-rust-runtime + make test-rust-workspace + the codegen gates green; closeout make ci. The goal is met when every HG row is done/decision-recorded with red-then-green evidence, the structural test passes across all runtimes/lifecycles, the threat model is written, and an adversarial re-review confirms no guest-reachable global or mutable trust value drives a trust decision or is called by the trusted path — or stop after 60 turns and record the blocker.
