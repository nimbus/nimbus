# Runtime Guest Trust-Global Hardening — Security Plan

Status: `active` (security; implementation started 2026-07-12, branch runtime-guest-trust-global-hardening)
Owner branch: `runtime-guest-trust-global-hardening` (own PR)
Origin: surfaced by the adversarial review of the Convex-runtimes-parity work in
`archive/examples-and-target-resolution-plan.md` (Band EX10R/EX10R2/EX10R3),
then verified + corrected by two independent Codex source-traces (2026-07-12,
`--effort high`): a first pass against Nimbus main, and a second pass that
cross-referenced the prescribed architecture against how deno_core/deno,
cloudflare/workerd, get-convex/convex-backend, denoland/rusty_v8, and
bytecodealliance/wasmtime actually solve the same guest/host trust boundary.

## Architectural framing (read first)

**The root cause is architectural, and every mature exemplar avoids the class by
construction — none of them harden trust globals property-by-property.** Nimbus's
trusted, codegen-emitted preamble and the guest handler bundle share ONE V8
realm / `globalThis`, so trusted dispatch functions sit on the same mutable
object graph guest code can reach. The exemplars keep the authoritative call
target off the guest-writable graph entirely:

- **cloudflare/workerd** (the multi-tenant gold standard): host methods are bound
  via V8 `FunctionTemplate` with the C++ impl as a compile-time template
  parameter (`src/workerd/jsg/jsg.h:200-204`, `resource.h:214-245`); the native
  callback is recovered from a V8 **internal field** (`wrappable.h:328-345`),
  never a property lookup — guest reassignment self-sabotages the guest only.
  Per-request authority is a **thread-local `IoContext::current()`**
  (`io-context.c++:1136,1341-1348`) re-established fresh per `run()`, so isolate
  reuse can't leak prior-request tampering. There is no `Deno.core.ops`-shaped
  ambient table; every capability is a typed resource handed out via `env`.
- **get-convex/convex-backend** (the upstream Nimbus targets): `ctx` is a **fresh
  object literal built per invocation and passed as a call ARGUMENT**
  (`crates/isolate/.../registration_impl.ts:55-72`), never installed on
  `globalThis`; the Rust→guest boundary reaches the handler via the **ES module
  namespace** (immutable bindings), not a global; guest syscalls go through a
  `Convex.{op,syscall}` FunctionTemplate (JS→Rust only). Convex explicitly
  **refuses isolate reuse across clients "for security isolation"**
  (`client.rs:1501-1517`) and drains + `check_isolate_clean()` between requests
  (`isolate.rs:261-289`) with the comment *"JS from this request may leak to a
  subsequent one on isolate reuse."* Nimbus's warm-pool amplifier is the exact
  concern the upstream already designed around — architecturally, not by
  per-property hardening.
- **denoland/deno_core + deno**: `Deno.core.ops` IS installed with plain
  `v8::Object::set` and never sealed (`bindings.rs:289,457`) — matching Nimbus's
  pinned fork. deno's protection is **capture-then-delete**: bootstrap captures
  each op into module scope (`bindings.rs:1146-1183`), imports them as lexical
  bindings (`99_main.js:8`), then `removeImportedOps()` **deletes** them off the
  live table (`99_main.js:633-727`) and deletes the `__bootstrap` alias.
  `ObjectFreeze(core)` (`01_core.js:854`) is shallow by design — fast-call
  upgrades keep `.set`-ing op slots afterward. deno tolerates leaving `Deno.core`
  reachable only because it is single-trust-level-per-realm; Nimbus's
  realm-sharing is strictly harder, so Nimbus must go FURTHER than deno, not just
  match it.
- **bytecodealliance/wasmtime** (contrast): no shared mutable global namespace at
  all; imports resolve once into an immutable `Arc<...>`, host state travels via
  Rust-owned `Store<T>`/`Caller<T>`. The class simply cannot exist.

**Consequence for this plan:** the tiered fix below (module-scope/closure →
private captured op refs → `defineProperty` slot-hardening) is the correct
*retrofit* — it is literally deno's own capture-and-delete idiom — for
Nimbus's existing shared-realm architecture. But tier-3 (`defineProperty`) is
NOT the aspirational end state. The aspirational target for any future
host-call-boundary redesign (especially HG0/HG1, the Rust-eval'd entrypoints) is
workerd's template-bound / thread-local-authority model and Convex's
fresh-ctx-as-argument + module-namespace dispatch. The structural test in this
plan is a **regression gate for an interim architecture**, not the destination.

## The vulnerability class (three reachable surfaces)

1. **Reassignable global properties** — `globalThis.__nimbusX = fn`;
   `Object.freeze(value)` freezes the function object, not the slot; only
   `Object.defineProperty {writable:false, configurable:false}` closes it.
2. **Mutable global lexical state / object graphs** — top-level `let`/`var`
   bindings and mutable objects captured by trusted code
   (`__nimbusInvocationGeneration`, the wait-until queue, and object graphs
   behind hardened slots like `__nimbusHiddenDenoGlobals` — a frozen SLOT does
   not protect a mutable VALUE).
3. **Native op-table properties** — `Deno.core.ops` props are writable in the
   pinned fork; overwrite an op the trusted transport reads.

**Amplifier — warm-pool reuse.** A guest reassignment in invocation N persists
into N+1's trusted path on the same isolate (`snapshot_lifecycle.rs:553` proves
module state survives current resets).

**Blast radius = SAME-TENANT cross-invocation / cross-user / cross-function —
NOT cross-tenant** (warm-pool keys on tenant-scoped bundle identity;
`warm_pool.rs:41,60,287`; tenant B cold-misses, `warm_pool.rs:96,301`). Still
serious: a later same-tenant invocation's `request`/`args`/`auth` is exposed.

## Findings (file:line corrected per the 2026-07-12 traces)

**Authoritative classification ledger:**
`proof/runtime-guest-trust-globals/classification-ledger.md` — the independently
re-enumerated inventory of every `globalThis.__nimbus*` install and realm-level
mutable lexical trust binding, each classified TRUST / INTENTIONALLY-MUTABLE /
COMPAT-OR-TEST with file:line, plus the structural-test allowlist and three
findings beyond this table's starting inventory (web-lane bare-name `__nimbusCoreOps`
alias to the ops table, global-lexical writability of HG6/HG8 state, and the
second HG0 emit site in Cloud Functions codegen). Later bands and the structural
test consume that ledger.

| ID | Surface | Sev | Mechanism | Where |
| --- | --- | --- | --- | --- |
| HG0 | `__nimbusInvoke` (**most serious**) | HIGH | Top-level host entrypoint, plain-assigned; **Rust string-evals `globalThis.__nimbusInvoke(...)` fresh every invocation** and passes the whole request — one level EARLIER than HG1. This is the surface furthest from ALL exemplars (none read a name off a shared object at call time) | consume `crates/nimbus-runtime/src/runtime/invocation.rs:78,80`; emit `packages/codegen/src/emit/runtime_bundle_dispatch_global_invoke.mjs:3`; Cloud Functions dup at `packages/codegen/src/cloud_functions/runtime_sources.mjs:35` |
| HG1 | `__nimbusCreateContext` | HIGH | Plain assign (`nimbus_context_contract.js:272`) + `Object.freeze(value)` only (`:599`); builds the whole ctx. Preamble reads it fresh per invocation (`runtime_bundle_preamble.mjs:41`; entrypoints `runtime_bundle_execution_entrypoints.mjs:2`) | see cells |
| HG2 | `__nimbusInvokeNamedLocal` | MED-HIGH | Plain assign (`runtime_bundle_dispatch_global_invoke.mjs:32`), read fresh into `localInvoker` per nested ctx.run* (`nimbus_context_contract.js:195`, invoked `:238`) | see cells |
| HG3 | `__nimbusCoreOps` / `Deno.core.ops` | HIGH (lane-conditional) | Transports read op props fresh (`deno_host_call_transport.js:130,165`); deno_core does NOT seal the table (`bindings.rs:289,457`). Overwrite `op_nimbus_ctx_resolve_callee_lane` → forge the value at `nimbus_context_contract.js:233`. **Reachability differs by lane** (see note). **Fix is NOT `Object.freeze` — see below** | see cells |
| HG5 | Cloudflare entrypoint `__nimbusInvokeCloudflareWorkerFetch` | MED | Plain-assigned host entrypoint, same reassign-the-entrypoint class as HG0 | `cloudflare_workers_runtime.js:278` |
| HG6 | wait-until lifecycle hooks + `__nimbusWaitUntilQueue` | MED | Host-called `__nimbusWaitUntil`/`Drain`/`Reset` (reassignable) + mutable queue the host drains | hooks `deno_host_call_transport.js:184`; queue `:182` |
| HG7 | `__nimbusRefreshNodeProcessCwd` + other host-called Node hooks | LOW-MED | Host/reset-called, reassignable; classify each | `node22_runtime_bootstrap.js` |
| HG8 | `__nimbusInvocationGeneration` (mutable lexical) | MED | Host reset writes it (`reset_bootstrap_invocation_state.js:2`), read as generation/trust state (`nimbus_context_contract.js:270`); guest can desync | see cells |
| HG9 | object graphs behind hardened slots (`__nimbusHiddenDenoGlobals`/`Node`) | MED | Slots hardened, VALUES mutable; trusted transpiled scripts consume them (`transpile.rs:33,37,45`) | see cells |
| HG4 | lane-oracle metadata | LOW | `runtime_environment_for_function` — no visibility check (`runtime_access.rs:90`; bridge `runtime_calls.rs:140`). NOT a strict subset of the error oracle (`selection.rs:14`) — adds `default`/`node`/`bun`. **Threat-model decision, not "add visibility"** (guests construct `{name,visibility}`; internal fns are app-callable). No exemplar informs this | see cells |
| HGx | stale `__nimbusNextHostCallSessionId` | cleanup | Undeclared assignment, unused; remove or justify | `reset_bootstrap_invocation_state.js:1` |

**HG3 lane split (Codex-2):** on the default/web lane `post_bootstrap.js:26`
does `delete globalThis.Deno` (guarded by `__nimbusRetainDenoForNodeLazyScripts
!== true`), so `Deno.core.ops` is NOT directly guest-reachable by name there —
the live ref survives only via the script-private `__nimbusCoreOps` const. On
**Node-compat lanes that retain `Deno`, it stays directly guest-reachable**. So
HG3 is *definitively* exploitable on Node lanes; the web lane needs an
alias audit to rule out other paths to the ops object. The red test must be
**lane-conditional** (web-Deno-deleted vs Node-Deno-retained).

Intentionally guest-mutable / app-singleton (document, do NOT harden):
`__nimbusCloudFunctionsState`, `__nimbusAdminApps` (`runtime_sources.mjs:81,834`),
compat/test surfaces (`__nimbusFlushEmbeddedTests`, `__nimbusNodeRuntimeMajor`,
…). The classification ledger must state why.

## Already hardened (verified accurate on main — do not regress)

`__nimbusRuntimeEnvironmentLane` (`deno_runtime_globals.js:312`),
`__nimbusCallDetachedFromInvocationContext` (`deno_host_call_transport.js:37`,
captured privately), `__nimbusSyncHostValue`/`__nimbusAsyncHostValue`
(`deno_host_call_transport.js:126,161`), guest-semantics hooks (block-scoped +
hardened, `nimbus_guest_semantics.js:26,235`), and the callee-lane lookup was
REMOVED (host-side, `runtime_calls.rs:133`). Caveat: transports are hardened at
the SLOT but still read the unhardened ops table (HG3) — not end-to-end.

## Fix approach — strongest available boundary per finding

1. **Rust-held `v8::Global<Function>` in an isolate slot — REQUIRED for HG0/HG1/HG5**
   (not "where feasible"). Instead of the Rust host string-eval'ing
   `globalThis.__nimbusInvoke(...)` by name every invocation
   (`invocation.rs:78,80`), capture the function ONCE post-bootstrap /
   pre-guest-execution into an isolate slot and call it directly — removing the
   entrypoints from the guest-reachable surface entirely rather than hardening a
   slot. Confirmed available: rusty_v8 `Isolate::set_slot/get_slot`
   (`isolate.rs:1250-1278`), `v8::Global` (`handle.rs:286-294`); deno_core uses
   exactly this for `OpState` (`jsruntime.rs:1020-1021`); it is workerd's
   thread-local-authority model in Rust form. `defineProperty` slot-hardening is
   the FALLBACK only if the isolate-slot approach proves incompatible with the
   current bootstrap-script-eval invocation path.
2. **Private captured op references — HG3.** Capture the exact trusted op
   functions into a private closure and have transports call the captured refs
   (deno's capture-then-delete), or build a private null-prototype capability
   table from selected op refs and freeze THAT — **never `Object.freeze` the
   live `Deno.core.ops`** (breaks deferred fast-call upgrades, `bindings.rs:593,690`).
   **Precision requirement (Codex-2):** capture must survive the *lazy* deferred
   fast-call upgrade pass (`ensure_fast_ops_upgraded`, `bindings.rs:713+`) that
   runs from residual ext-module loaders AFTER bootstrap, not just initial op
   registration. Because V8 fast-call swaps the internal fast-API pointer in
   place (not the JS function identity), a captured *reference* stays valid — but
   this must be verified for the pinned fork with a test asserting a captured op
   ref still fast-calls correctly after the lazy pass.
3. **Module-scope / closure — HG2.** Pass the module-private
   `invokeNamedDefinitionLocally` into `__nimbusCreateContext` (which closes over
   it) and remove the global — Convex's fresh-ctx-as-argument pattern.
4. **`defineProperty` slot hardening** ({writable:false, configurable:false,
   enumerable:false}, atomic install) — the minimum for HG6/HG7 and the HG0/HG1/HG5
   fallback. Function-object freeze is optional defense-in-depth.
5. **Mutable-value protection — HG8/HG9.** Descriptor invariant is insufficient;
   move generation/reset state to host-owned or closure-private storage; review
   the hidden-globals object graphs directly.
6. **HG4:** decide in the threat model — document lane metadata as non-secret, or
   design uniform failure behavior against an explicit confidentiality
   requirement.

## Structural test — full spec

A single regression-gate test (extend the EXISTING idioms in
`nested_dispatch.rs:124-129,368,432` and `guest_semantics.rs:531-597`, which
already do reassignment + `getOwnPropertyDescriptor` + strict-mode-throws
assertions — do not invent a new one; the ownKeys+allowlist inventory is
net-new):
- After bootstrap AND after bundle load: `Reflect.ownKeys(globalThis)` inventory
  of every `__nimbus*` property.
- Explicit **allowlist classification** for every `__nimbus*` (trust / mutable /
  compat) — a NEW unlisted property fails the test.
- Per trust property: own data descriptor, `writable:false`, `configurable:false`,
  **stable identity after attempted assignment/delete/redefine**.
- **Identity-stability assertion (Codex-2):** not only "impostor not called" but
  "the trusted path's captured reference is the SAME function object after a
  reassignment attempt" (capture the internally-used ref via a test hook,
  reassign the global, invoke, assert same object executed) — modeling workerd's
  compile-time identity guarantee, closing the gap where an impostor happens to
  behave identically.
- Separate checks for global **lexical** bindings and **mutable object values**
  (HG8/HG9), not just descriptors.
- Coverage: web/default, Node, Cloudflare × main-realm warm reuse AND fresh-realm
  recycling × normal AND Cloud Functions codegen. **HG3 red test is
  lane-conditional** (web-Deno-deleted vs Node-Deno-retained).
- **Red/green exploit tests** for HG0, HG1, HG2, HG3, HG5, HG6.
- State in-test why this exists: a regression gate for an interim shared-realm
  architecture (exemplars need no such test because they don't put trust state
  on the global) — not the exemplar-validated end state.

## Threat model deliverable (concrete)

Principals (guest handler code, trusted preamble, Rust host); protected data (a
later same-tenant invocation's request/args/auth); the same-tenant-vs-cross-tenant
boundary (warm-pool keys on tenant-scoped bundle identity — cross-tenant is out
of reach; cite **Convex's refusal to reuse isolates across clients**,
`client.rs:1501-1517`, and its `check_isolate_clean()` drain, `isolate.rs:261-289`,
as industry precedent that same-client reuse is the boundary requiring explicit
reasoning); import-time execution + `new Function` realm-global reachability;
warm-pool partitioning as the persistence vector; host-session-id enforcement;
and which guest-visible capabilities are INTENTIONALLY callable.

## Verification lanes

Runtime: `make test-rust-runtime` + `make test-rust-workspace`. Codegen: codegen
selftests + `npm run typecheck/test/build`, restage embedded packages before
live checks. Closeout `make ci`. Bootstrap-JS edits need BOTH the Rust runtime
lanes (they compile the JS in) and the codegen gates.

## Revision history

- 2026-07-12 Codex pass 1 (Nimbus source): corrected cross-tenant→same-tenant;
  added HG0/HG5/HG6/HG8/HG9; corrected HG3 fix (not freeze); reframed HG4.
- 2026-07-12 Codex pass 2 (exemplar cross-reference, this revision): added the
  architectural framing (workerd/Convex/deno/wasmtime); upgraded HG0/HG1/HG5 to
  REQUIRED Rust-held isolate-slot capture; added the HG3 lazy-fast-call-upgrade
  precision + lane split; added the identity-stability structural assertion; cited
  Convex isolate-reuse refusal in the threat model; fixed citation drift
  (`invocation.rs:52`→`:78,80`, `context_contract.js:194`→`:195`,
  `preamble.mjs:40`→`:41`, `transpile.rs:29`→`:33,37,45`, HG6 queue→`:182`,
  `warm_pool.rs:40`→`:41`).

## Suggested Goal Prompt (when promoted)

/goal Execute docs/private/plans/runtime-guest-trust-global-hardening-plan.md. Produce the classification ledger (Reflect.ownKeys inventory of every globalThis.__nimbus* + realm lexical trust state, labelled trust/mutable/compat with file:line). Fix each surface at the strongest available boundary: HG0/HG1/HG5 via a Rust-held v8::Global<Function> captured once into an isolate slot post-bootstrap (defineProperty fallback only if incompatible with the string-eval invocation path); HG3 via private captured op references verified to survive the lazy deferred fast-call upgrade pass — NEVER Object.freeze the live Deno.core.ops; HG2 by passing the module-private invoker into __nimbusCreateContext and removing the global; HG8/HG9 via host-owned/closure-private storage. Each with a red-then-green exploit test following the existing nested_dispatch.rs idiom, HG3 lane-conditional (web-Deno-deleted vs Node-retained). Land the structural test to the full spec including the identity-stability assertion (captured ref unchanged after reassignment). Decide HG4 in the threat-model doc; cite Convex's cross-client isolate-reuse refusal. Run make test-rust-runtime + make test-rust-workspace + codegen gates green; closeout make ci. Met when every HG row is done/decision-recorded with red-then-green evidence, the structural test passes across all lanes/lifecycles, the threat model is written, and an adversarial re-review confirms no guest-reachable global or mutable trust value drives a trust decision or is called by the trusted path — or stop after 60 turns and record the blocker.
