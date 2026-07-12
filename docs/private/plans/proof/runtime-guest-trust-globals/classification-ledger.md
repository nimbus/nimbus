# Runtime Guest Trust-Global Classification Ledger

Authoritative reference for the runtime-guest-trust-global-hardening plan
(`docs/private/plans/runtime-guest-trust-global-hardening-plan.md`). Every later
band and the structural regression test consume this file. It enumerates every
`globalThis.__nimbus*` install AND every realm-level mutable lexical trust
binding reachable from guest code in
`crates/nimbus-runtime/src/runtime/bootstrap/**`,
`packages/codegen/src/emit/**`, and `packages/codegen/src/cloud_functions/**`,
classified as **TRUST**, **INTENTIONALLY-MUTABLE**, or **COMPAT-OR-TEST**.

Line numbers are against branch `runtime-guest-trust-global-hardening`
(base `eb2770c18`).

## Method + the reachability model (read first)

Three distinct guest-reachable surfaces exist in one shared V8 realm. The plan
names two; independent enumeration confirms a **third** that the plan flagged
only as an open "alias audit" question — it is real and is documented here.

1. **`globalThis.__nimbusX` own properties.** Reachable by property lookup.
   `Object.freeze(value)` freezes the function object, not the slot; only
   `Object.defineProperty {writable:false, configurable:false}` closes the slot.

2. **Global-lexical bindings from classic bootstrap scripts.** Bootstrap scripts
   run through `JsRealm::execute_script`, which compiles them with
   `v8::Script::compile` — a **classic Script, not a Module** (deno_core
   `libs/core/runtime/jsrealm.rs:461-490`). A classic script's top-level
   `let`/`const`/`class` are installed in the realm's **global lexical
   environment record**, NOT on `globalThis`. Per ECMA-262 a Module's
   environment `[[OuterEnv]]` is that same global environment record, so guest
   **module** code resolving a free identifier finds those bindings by bare
   name. Consequence:
   - `const __nimbusCoreOps` (`deno_host_call_transport.js:1`) is a **bare-name
     alias to the live `Deno.core.ops` table that survives `delete
     globalThis.Deno`** on the web/default lane. This is the extra path the plan
     asked to "rule out" for HG3 (plan line 112) — it is NOT ruled out; the web
     lane is reachable via this alias, not only via `Deno`. `const` means the
     guest can *read* the table (and overwrite op slots on it) but cannot rebind
     the alias itself.
   - `let __nimbusWaitUntilQueue` (`:182`) and `let __nimbusInvocationGeneration`
     (`nimbus_context_contract.js:270`) are bare-name **writable** from guest
     modules (`__nimbusInvocationGeneration = N`) — HG8/HG6 do not even require a
     `globalThis` property to exploit.

3. **`Deno.core.ops` native op-table properties.** Individually writable in the
   pinned fork (`bindings.rs:289,457`); overwrite an op slot the trusted
   transport reads (HG3).

**Warm-pool amplifier + blast radius** (verified, unchanged from plan): a guest
mutation in invocation N persists into N+1's trusted path on the same warm
isolate; blast radius is SAME-TENANT cross-invocation, not cross-tenant
(`warm_pool.rs` keys on tenant-scoped bundle identity).

## TRUST — read/called by the trusted preamble or Rust host for a trust decision (must harden at strongest boundary)

| Property / binding | Surface | File:line | Current hardening | Finding | Fix owner band |
| --- | --- | --- | --- | --- | --- |
| `globalThis.__nimbusInvoke` (default/Convex bundle) | (1) global prop | emit `runtime_bundle_dispatch_global_invoke.mjs:3`; **Rust host string-evals by name** `invocation.rs:78,80`, `driver/loading.rs:928,1057`, `cooperative.rs:518` | plain assign, NOT hardened | **HG0** (most serious — Rust reads the guest-writable name at call time) | **Band B (this)** |
| `globalThis.__nimbusInvoke` (Cloud Functions bundle) | (1) global prop | emit `cloud_functions/runtime_sources.mjs:35`; same Rust eval sites | plain assign, NOT hardened | **HG0** (Cloud Functions codegen variant — same class, second emit site) | **Band B (this)** |
| `globalThis.__nimbusInvokeCloudflareWorkerFetch` | (1) global prop | `cloudflare_workers_runtime.js:278,295`; **Rust host string-evals by name** `invocation.rs:68` | `Object.freeze(value)` only — slot writable | **HG5** (bootstrap-stable entrypoint, same class as HG0) | **Band B (this)** |
| `globalThis.__nimbusCreateContext` | (1) global prop | `nimbus_context_contract.js:298,648`; read fresh per invocation by trusted preamble `runtime_bundle_preamble.mjs:41`, entrypoints `runtime_bundle_execution_entrypoints.mjs:3,15,42,53` | `Object.freeze(value)` only — slot writable | **HG1** (builds the whole ctx incl. auth/db/scheduler capabilities) | **Band B (this)** |
| ~~`globalThis.__nimbusInvokeNamedLocal`~~ (REMOVED — see "Already-hardened") | (1) global prop | baseline: emit `runtime_bundle_dispatch_global_invoke.mjs:32`; read fresh into `localInvoker` per nested `ctx.run*` `nimbus_context_contract.js:195`, invoked `:246` | baseline: plain assign, NOT hardened | **HG2** | **Band C — DONE** |
| ~~`Deno.core.ops` table + `const __nimbusCoreOps` alias~~ (HARDENED — see "Already-hardened") | (2)+(3) | baseline: live table `deno_host_call_transport.js:1`; read fresh `:91,131,166,188`; also `reset_bootstrap_invocation_state.js:8`, `post_bootstrap.js:2` | baseline: op slots writable; alias `const` (readable by bare name, web-lane reachable post-`delete Deno`) | **HG3** (+ discovered web-lane bare-name alias, closed by the same fix; lane-conditional red test per plan) | **Band D — DONE** |
| ~~`globalThis.__nimbusWaitUntil` / `__nimbusDrainWaitUntil` / `__nimbusResetWaitUntil` + `let __nimbusWaitUntilQueue`~~ (HARDENED — see "Already-hardened") | (1)+(2) | baseline: hooks `deno_host_call_transport.js:184,197,212`; queue `:182`; host drains via `driver/loading.rs:619-635` | baseline: plain assign; queue bare-name writable | **HG6** | **Band E — DONE** |
| `globalThis.__nimbusRefreshNodeProcessCwd` | (1) global prop | `node22_runtime_bootstrap.js:4113` (`writable:true, configurable:true`); host/reset-called `reset_bootstrap_invocation_state.js:4-5` | NOT hardened (writable slot) | **HG7** | HG7 band (later) |
| ~~`let __nimbusInvocationGeneration`~~ (HARDENED — see "Already-hardened") | (2) global-lexical | baseline: `nimbus_context_contract.js:270`; read as stale-guard trust state `:273,276`; host reset writes `reset_bootstrap_invocation_state.js:2` | baseline: bare-name writable from guest modules | **HG8** (guest desyncs generation → defeats "ctx from previous invocation" guard) | **Band E — DONE** |
| `globalThis.__nimbusHiddenDenoGlobals` / `__nimbusHiddenNodeGlobals` | (1) global prop, mutable VALUE | `node22_runtime_bootstrap.js:3430,3436` (slot `writable:false` but value object mutable); consumed by trusted transpiled scripts `transpile.rs:33,37,45` | slot hardened, VALUE graph mutable | **HG9** (frozen slot does not protect a mutable value) | HG9 band (later) |

## INTENTIONALLY-MUTABLE — app-singleton / guest-owned state (documented safe; do NOT harden)

| Property / binding | File:line | Why safe to leave mutable |
| --- | --- | --- |
| `globalThis.__nimbusCloudFunctionsState` | `cloud_functions/runtime_sources.mjs:81` (`??=`) | App-singleton registry of guest-declared Cloud Functions targets/global-defaults. It IS guest state by design (the guest's own `onDocumentCreated`/framework registrations accumulate here). No trusted path reads it for a trust/authority decision — the dispatcher captured it at build time into `__nimbusCollectedTargets`. Tampering only corrupts the guest's own registration view. |
| `globalThis.__nimbusAdminApps` | `cloud_functions/runtime_sources.mjs:834` (`??=`) | App-singleton list of initialized admin app handles (firebase-admin `initializeApp` parity). Guest-owned; not a trust oracle. |
| `globalThis.__nimbusTargets` (module export, not a global) | `cloud_functions/runtime_sources.mjs:34` | An ES module `export const`, not installed on `globalThis`; immutable module binding. Listed for completeness — not an attack surface. |

## COMPAT-OR-TEST — bootstrap-transient or test/diagnostic helpers (not a persistent guest-reachable trust surface)

| Property / binding | File:line | Classification rationale |
| --- | --- | --- |
| `globalThis.__nimbusRefreshNodeRuntimeOpState` | install `node22_runtime_bootstrap.js:4066` (writable), **`delete`d** `post_bootstrap.js:23` | Bootstrap-transient: deleted before any guest code runs. Not reachable at invocation time. |
| `globalThis.__nimbusDenoFetchModule` | `98_global_scope_shared.js:28`, **`delete`d** `post_bootstrap.js:24` | Bootstrap-transient (WASM-streaming wiring), deleted post-bootstrap. |
| `globalThis.__nimbusRetainDenoForNodeLazyScripts` | `node22_runtime_bootstrap.js:3964`, **`delete`d** `post_bootstrap.js:28` | Bootstrap-transient lane flag, deleted post-bootstrap. |
| `globalThis.__nimbusDrainImmediates` | `deno_host_call_transport.js:12` (`writable:true, configurable:true`) | Test/harness drain hook for async_hooks destroy queue (spawn-emulation postlude, `render.rs`); diagnostic, carries no trust decision. |
| `globalThis.__nimbusFlushEmbeddedTests` | `node22_runtime_bootstrap.js:3970` | Embedded-test harness hook. Test-only. |
| `globalThis.__nimbusNodeRuntimeMajor` (+ `process.__nimbusNodeRuntimeMajor`) | `post_bootstrap.js:44,51` (`writable:false`) | Node major-version metadata for compat shims; non-secret, non-authority; already slot-hardened. |
| `globalThis.__nimbusProcessTicksAndRejections` / `__nimbusEventLoopHasMoreWork` / `__nimbusPerfHooksBuiltin` / `__nimbusStartWorkerMessagePump` / `__nimbusCloseWorker` / `__nimbusInstallSharedWorkerEnvProxy` / `__nimbusWorkerThreadEnv` | `node22_runtime_bootstrap.js:3976,3982,3988,3120,3598,3648,4182` | Node/worker runtime plumbing hooks. Called host-side or by trusted Node bootstrap; enumerate per-hook in the HG7 band, but none carries a same-tenant confidentiality trust decision the way HG0/HG1 do. Recorded here so the HG7 band classifies each explicitly rather than trusting this row. |
| `globalThis.__nimbusStartWorkerMessagePump` etc. | (as above) | (see above) |
| `__nimbusNextHostCallSessionId` (undeclared global assignment) | `reset_bootstrap_invocation_state.js:1` | **HGx cleanup**: assigned (`= 1`) but never declared and never read anywhere in the tree (`grep` confirms the reset write is the only reference). Dead. Remove or justify in cleanup. |

## Already-hardened trust surfaces (verified accurate — do NOT regress)

- `globalThis.__nimbusSyncHostValue` / `__nimbusAsyncHostValue` — slot-hardened
  `{writable:false, configurable:false, enumerable:false}` at
  `deno_host_call_transport.js:165,200`. The fresh `globalThis.__nimbusAsyncHostValue`
  reads inside `nimbus_context_contract.js` (`:312,320,415-431`) and
  `cloudflare_workers_runtime.js` (`:190,201,211,220,224`) are therefore safe —
  the slot cannot be reassigned.
- `globalThis.__nimbusCallDetachedFromInvocationContext` — slot-hardened
  `:76-93`; captured privately as `const __nimbusDetachedNestedCall`
  (`nimbus_context_contract.js:6`).
- `globalThis.__nimbusRuntimeEnvironmentLane` — slot-hardened
  `deno_runtime_globals.js:312`; read as the trusted local-vs-host lane signal
  `nimbus_context_contract.js:217`.
- `globalThis.__nimbusBeginGuestInvocation` — slot-hardened + `Object.freeze`
  `nimbus_guest_semantics.js:266`. **Note for Band B:** the ConvexDefault invoke
  prelude reads this by name in the Rust-eval'd expression
  (`invocation.rs:78`). Because the slot is hardened it is safe to keep reading
  by name; Band B captures it alongside `__nimbusInvoke` when it moves the invoke
  call off string-eval so the whole invoke expression leaves the guest-name path.
- `globalThis.__nimbusInstallGuestSemantics` / `__nimbusEnterGuestImportPhase` —
  slot-hardened `nimbus_guest_semantics.js:235,245`; block-scoped guest-semantics
  hooks hardened at `nimbus_guest_semantics.js` (per plan "already hardened").
- `globalThis.__nimbusInvokeNamedLocal` (HG2, Band C) — **removed entirely**,
  not slot-hardened: `invokeNamedDefinitionLocally` now passes as an explicit
  `invokeNamedLocal` call argument into `globalThis.__nimbusCreateContext({...})`
  (Convex's fresh-ctx-as-argument pattern), threaded through
  `__nimbusCreateContextImpl` → `__nimbusRunNamedFunction(..., localInvoker)`
  for `runQuery`/`runMutation`/`runAction`
  (`nimbus_context_contract.js:193,281-282,569,586,603`; preamble wiring
  `runtime_bundle_preamble.mjs:54`). The trusted dispatch call site never reads
  a `globalThis` property by name, so there is no slot to harden — unlike
  HG0/HG1/HG5, the global itself no longer exists after this band. The
  structural test's HG2 assertion is **absence**: `Reflect.ownKeys(globalThis)`
  must NOT contain `__nimbusInvokeNamedLocal` after bootstrap or bundle load.
- `const __nimbusCoreOps` (HG3, Band D) — **no longer an alias to the live
  `Deno.core.ops` table.** It is rebound to
  `Object.freeze(Object.assign(Object.create(null), Deno.core.ops))`
  (`deno_host_call_transport.js:36-38`), a private null-prototype clone taken
  as the very first bootstrap script runs, strictly before any guest code —
  and before deno_core's lazy `ensure_fast_ops_upgraded` pass could ever fire
  (that pass only triggers from residual ext-module/Node-polyfill lazy
  loading, never during static bootstrap; verified against the pinned
  deno_core fork's `bindings.rs`). The transports
  (`__nimbusCurrentHostCallSessionId:130`, `__nimbusSyncHostValue:170`,
  `__nimbusAsyncHostValue:205`, `__nimbusWaitUntil:227`) all read this frozen
  clone, never the live table, on both lanes. The bare-name binding itself is
  still guest-readable (that cannot be closed — see method note 2 above), but
  writes through it or through `Deno.core.ops` directly no longer reach
  anything the trusted path consults, because the clone is a distinct frozen
  object with no shared identity to the live table. Documented tradeoff:
  `op_nimbus_runtime_wait_until_pending` is the one op reached through this
  table that is `#[op2(fast)]`-eligible; if a lazy fast-call upgrade ever does
  fire later in this isolate's life it will not be reflected in the frozen
  clone, so that op stays on its slow-path snapshot function for the rest of
  the isolate's lifetime — accepted, since wait-until tracking is not a hot
  per-dispatch path and every other op reached through this table is a plain
  (non-fast) op that can never go stale. Red/green exploit tests, lane-
  conditional per the plan: `guest_core_ops_table_tampering_via_bare_binding_
  cannot_force_cross_lane_local_dispatch` (web/default lane, bare-name reach)
  and `guest_core_ops_table_tampering_via_retained_deno_cannot_force_cross_
  lane_local_dispatch` (Node-compat lane, direct `Deno.core.ops` reach), plus
  `guest_core_ops_table_tampering_leaves_same_lane_local_dispatch_intact`
  (over-correction guard) — all three in
  `crates/nimbus-runtime/src/runtime/tests/basic_invocation/nested_dispatch.rs`.
- `let __nimbusWaitUntilQueue` (HG6, Band E) — **no longer a bare top-level
  `let`.** The queue now lives inside a block scope
  (`deno_host_call_transport.js:230-276`, `{ let queue = []; ... }`) that is
  never installed under any name, bare or property — a strictly stronger
  reachability closure than a `const` would give, since there is no binding
  at all outside the block for guest module code to resolve. The three hooks
  (`__nimbusWaitUntil`/`__nimbusDrainWaitUntil`/`__nimbusResetWaitUntil`) are
  closures over `queue`, installed on `globalThis` via `Object.defineProperty`
  (`writable:false, configurable:false`), replacing the baseline's plain
  `globalThis.X = function(){}` assignments. Red/green exploit test:
  `pir4_wait_until_hook_tampering_cannot_hide_unreferenced_pending_background_work_from_system_timeout`
  in `crates/nimbus-runtime/src/runtime/tests/timeout_cancellation.rs` — a
  sloppy-mode reassignment of the bare `__nimbusWaitUntil` name to a no-op
  impostor is now a harmless decoy-property write; the real hook still tracks
  the promise and the system timeout still bounds it.
- `let __nimbusInvocationGeneration` (HG8, Band E) — **no longer a bare
  top-level `let`.** The counter now lives inside an IIFE closure
  (`nimbus_context_contract.js`,
  `const __nimbusReadInvocationGeneration = (() => { let generation = 0;
  ... })()`) that installs only a frozen `__nimbusAdvanceInvocationGeneration`
  increment slot on `globalThis`; the getter function itself is private to
  the closure. `guardStale()` and the ctx factory's `myGeneration` capture
  both call the returned getter instead of reading the bare binding, and the
  trusted host-issued reset script (`reset_bootstrap_invocation_state.js:5`)
  advances the counter through the same hardened slot instead of bare-name
  arithmetic on a shared binding. Red/green exploit test:
  `guest_generation_forgery_cannot_defeat_stale_ctx_reuse_guard` in
  `crates/nimbus-runtime/src/runtime/tests/basic_invocation/nested_dispatch.rs`
  — a sloppy-mode attempt to forge the bare binding back to a captured
  earlier value (to keep a stale `ctx` object usable) is now a harmless decoy-
  property write; the real guard still fires
  (`"This ctx object is from a previous invocation and cannot be reused"`).
  **Follow-up (not a regression, tracked for HG7):**
  `__nimbusAdvanceInvocationGeneration` is itself guest-callable (frozen
  slot, but any caller may invoke it) — a guest could self-advance the
  counter to prematurely invalidate its OWN ctx objects. This is a
  self-inflicted-DoS-class concern only (no cross-invocation or cross-tenant
  authority impact — a guest can already make its own invocation fail in
  arbitrarily many other ways), not part of HG8's guest-desync-the-guard
  scope, and left for HG7/HGx triage.

## Structural-test allowlist (consumed by the regression gate)

The structural test's `Reflect.ownKeys(globalThis)` inventory must classify every
`__nimbus*` own property present after bootstrap AND after bundle load into
exactly one bucket below; a new unlisted property fails the test.

- **TRUST (must be non-reachable, slot-hardened, or host-authority-off-the-name
  after Band completion):**
  `__nimbusInvoke` — Band B moved the host's authority OFF the name: the host now
  calls the reference captured into a per-realm `v8::Private` at load
  (`captured_dispatch.rs`), so the global remains present as the guest's own
  (inert) dispatch handle but no longer drives the trusted path. The structural
  test's HG0 assertion is IDENTITY (the captured reference is invoked), not
  absence of the global. (Capture-then-delete was deliberately not taken: the
  global is the guest's own emitted code, so removing it from the guest's view
  is not itself a security boundary, and deleting on the central warm-reuse
  invoke path carries regression risk for no additional authority guarantee.)
  `__nimbusInvokeCloudflareWorkerFetch` (same treatment as HG0, Band B),
  `__nimbusCreateContext` (slot-hardened non-writable/non-configurable, Band B),
  `__nimbusRefreshNodeProcessCwd` (HG7),
  `__nimbusHiddenDenoGlobals`/`__nimbusHiddenNodeGlobals` (HG9, value-freeze).
- **TRUST already-hardened (assert descriptor stays `writable:false,
  configurable:false`):** `__nimbusSyncHostValue`, `__nimbusAsyncHostValue`,
  `__nimbusCallDetachedFromInvocationContext`, `__nimbusRuntimeEnvironmentLane`,
  `__nimbusBeginGuestInvocation`, `__nimbusInstallGuestSemantics`,
  `__nimbusEnterGuestImportPhase`, `__nimbusWaitUntil`/
  `__nimbusDrainWaitUntil`/`__nimbusResetWaitUntil` (HG6, Band E — queue
  itself is closure-private, never installed under any name),
  `__nimbusAdvanceInvocationGeneration` (HG8, Band E — counter itself is
  closure-private, never installed under any name; see the guest-callable
  follow-up noted above for HG7/HGx triage).
- **TRUST removed entirely (assert ABSENCE from `Reflect.ownKeys(globalThis)`
  after bootstrap and after bundle load, Band C):** `__nimbusInvokeNamedLocal`
  (HG2) — the module-private `invokeNamedDefinitionLocally` now passes as a
  call argument into `__nimbusCreateContext`, so no `globalThis` slot exists to
  harden or check a descriptor on.
- **INTENTIONALLY-MUTABLE (assert present-and-mutable is acceptable):**
  `__nimbusCloudFunctionsState`, `__nimbusAdminApps` (Cloud Functions bundles only).
- **COMPAT-OR-TEST (assert deleted post-bootstrap OR present-non-authority):**
  deleted → `__nimbusRefreshNodeRuntimeOpState`, `__nimbusDenoFetchModule`,
  `__nimbusRetainDenoForNodeLazyScripts`; present → `__nimbusNodeRuntimeMajor`,
  `__nimbusDrainImmediates`, `__nimbusFlushEmbeddedTests`, the Node/worker
  plumbing hooks.
- **Global-lexical, already neutralized (Band D, HG3 — DONE):**
  `__nimbusCoreOps`. The binding is still bare-name readable (unavoidable, see
  method note 2), but it is now a frozen private clone with no shared identity
  to the live `Deno.core.ops` table, so a write through either reach path
  (bare name on any lane, or `Deno.core.ops` directly on Node-compat lanes)
  cannot affect the trusted transports.
- **Global-lexical, already neutralized (Band E, HG6/HG8 — DONE):**
  `__nimbusInvocationGeneration`, `__nimbusWaitUntilQueue`. Neither binding
  exists under either name anymore, bare or property — both moved to
  closure-private storage (a block scope for the queue, an IIFE for the
  counter) with no name at all left for a bare-identifier read or write to
  resolve to. A sloppy-mode assignment attempt against either name now
  auto-vivifies an unrelated, harmless decoy `globalThis` property instead of
  touching any real state.

## Findings beyond the plan's starting inventory

1. **Web-lane bare-name alias to the ops table (extends HG3).** `const
   __nimbusCoreOps` in the classic-script global-lexical environment keeps
   `Deno.core.ops` reachable by bare name after `delete globalThis.Deno`. The
   HG3 band's "web lane alias audit" resolves to: the alias exists; web-lane HG3
   reachability is confirmed, not merely theoretical.
2. **Global-lexical writability of HG6/HG8 state.** `__nimbusWaitUntilQueue` and
   `__nimbusInvocationGeneration` are guest-writable by bare name without any
   `globalThis` property, because classic-script `let` lands in the shared global
   lexical environment. The HG6/HG8 fixes must move this state to closure-private
   or host-owned storage (a `defineProperty` on `globalThis` would not even
   address the reachable surface).
3. **Second HG0 emit site.** The Cloud Functions codegen path installs its own
   `globalThis.__nimbusInvoke` (`cloud_functions/runtime_sources.mjs:35`) via
   `createInvocationDispatcher`. Band B's capture-then-delete must cover BOTH the
   default `runtime_bundle_dispatch_global_invoke.mjs` emit and this Cloud
   Functions emit (the structural test's "normal AND Cloud Functions codegen"
   coverage axis).
4. **`node22_runtime_bootstrap.js` has its own, separate, un-hardened
   `core.ops` surface — out of scope for Band D/HG3, follow-up for HG7.**
   Unlike every other bootstrap file, this one is an ES **module**
   (`import { core, ... } from "ext:core/mod.js"` at line 1), not a classic
   script. Its `core` import binding is therefore a **module-environment
   binding**, not a global-lexical one — it does NOT share scope with the
   `__nimbusCoreOps` const from `deno_host_call_transport.js`, so Band D's fix
   does not reach it. Dozens of call sites read `core.ops.op_nimbus_*` fresh
   (e.g. `op_nimbus_runtime_cwd`, the `op_nimbus_worker_*` family,
   `op_nimbus_runtime_shared_env_{set,delete,get,snapshot,seed}`) alongside
   deno_core's own internal ops (`op_worker_close`, `op_uid`, …), spanning
   roughly lines 177–3670+. A guest overwrite of any of these op slots on the
   live table (reachable the same way HG3's Node-lane vector is, since this
   file only runs on Node-compat lanes) is unaudited. This needs its own
   band-scoped fix — most naturally HG7, since it is a much larger surface
   than a single-line capture swap (a module-scope const capturing `core.ops`
   analogous to Band D's fix is the likely shape, but every call site's op
   name would need enumerating or the same full-table-clone approach reused).
   Do not fold this into Band D — it is a distinct file, a distinct binding
   kind, and a distinct blast radius.
5. **`const __nimbusContextHostCallOps` (Phase-1 review addendum — safe, not a
   gap requiring a band).** A classic-script top-level `const ... = new
   Set([...])` (`deno_host_call_transport.js:96-127`) enumerating the
   context host-call op names (`op_nimbus_ctx_query_start`,
   `op_nimbus_ctx_run_query`, `op_nimbus_ctx_scheduler_run_after`, …). Like
   `__nimbusCoreOps`, `const` blocks *rebinding* but not bare-name reads or
   mutation of the Set's contents (`.add()`/`.delete()`), and the Set is not
   frozen. `__nimbusBindHostCallPayload` (`:137-158`) consults it via
   `.has(opName)` to decide whether to stamp/validate
   `host_call_session_id` on a payload before dispatch; a guest that
   `.delete()`s an op name from this Set (bare-name reachable, warm-pool-
   persistent like every other classic-script global-lexical binding) makes
   `__nimbusBindHostCallPayload` skip that stamping entirely for the op.
   Traced to ground before concluding safe: the *authoritative* session
   check is independent and host-side —
   `enforce_live_host_call_session` (`runtime/bootstrap/ops/shared.rs:308-338`)
   runs on every `op_nimbus_sync_host_call`/`op_nimbus_async_host_call`
   dispatch and compares the payload's `host_call_session_id` against the
   Rust-owned `RuntimeInvocationHostCallBinding::session_id()` — not
   anything the JS layer asserts. `operation_requires_host_call_session`
   (`:340-345`) defaults to `true` for every `HostCallOperation` except
   `HttpRoute`/`RuntimeExtensionCall`, which covers every op in this Set.
   So Set tampering only breaks the JS-side convenience stamping; the
   affected op call then arrives at the host with no (or a guest-forged)
   `host_call_session_id`, which the Rust-side check rejects with "runtime
   host-call session is stale or forged" — a hard failure, never a silent
   bypass. Not present anywhere in the TRUST/INTENTIONALLY-MUTABLE/
   COMPAT-OR-TEST tables above; recorded here rather than added to a table
   because it does not need a fix, only a documented reason it was
   considered and cleared.
