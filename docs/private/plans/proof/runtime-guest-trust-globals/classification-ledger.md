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
| `globalThis.__nimbusCreateContext` | (1) global prop | `nimbus_context_contract.js:272,599`; read fresh per invocation by trusted preamble `runtime_bundle_preamble.mjs:41`, entrypoints `runtime_bundle_execution_entrypoints.mjs:3,15,42,53` | `Object.freeze(value)` only — slot writable | **HG1** (builds the whole ctx incl. auth/db/scheduler capabilities) | **Band B (this)** |
| `globalThis.__nimbusInvokeNamedLocal` | (1) global prop | emit `runtime_bundle_dispatch_global_invoke.mjs:32`; read fresh into `localInvoker` per nested `ctx.run*` `nimbus_context_contract.js:195`, invoked `:246` | plain assign, NOT hardened | **HG2** | HG2 band (later) |
| `Deno.core.ops` table + `const __nimbusCoreOps` alias | (2)+(3) | live table `deno_host_call_transport.js:1`; read fresh `:91,131,166,188`; also `reset_bootstrap_invocation_state.js:8`, `post_bootstrap.js:2` | op slots writable; alias `const` (readable by bare name, web-lane reachable post-`delete Deno`) | **HG3** (+ discovered web-lane bare-name alias; lane-conditional per plan) | HG3 band (later) |
| `globalThis.__nimbusWaitUntil` / `__nimbusDrainWaitUntil` / `__nimbusResetWaitUntil` + `let __nimbusWaitUntilQueue` | (1)+(2) | hooks `deno_host_call_transport.js:184,197,212`; queue `:182`; host drains via `driver/loading.rs:619-635` | plain assign; queue bare-name writable | **HG6** | HG6 band (later) |
| `globalThis.__nimbusRefreshNodeProcessCwd` | (1) global prop | `node22_runtime_bootstrap.js:4113` (`writable:true, configurable:true`); host/reset-called `reset_bootstrap_invocation_state.js:4-5` | NOT hardened (writable slot) | **HG7** | HG7 band (later) |
| `let __nimbusInvocationGeneration` | (2) global-lexical | `nimbus_context_contract.js:270`; read as stale-guard trust state `:273,276`; host reset writes `reset_bootstrap_invocation_state.js:2` | bare-name writable from guest modules | **HG8** (guest desyncs generation → defeats "ctx from previous invocation" guard) | HG8 band (later) |
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
  `deno_host_call_transport.js:126,161`. The fresh `globalThis.__nimbusAsyncHostValue`
  reads inside `nimbus_context_contract.js` (`:312,320,415-431`) and
  `cloudflare_workers_runtime.js` (`:190,201,211,220,224`) are therefore safe —
  the slot cannot be reassigned.
- `globalThis.__nimbusCallDetachedFromInvocationContext` — slot-hardened
  `:37-54`; captured privately as `const __nimbusDetachedNestedCall`
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
  `__nimbusInvokeNamedLocal` (HG2), `__nimbusWaitUntil`/`__nimbusDrainWaitUntil`/
  `__nimbusResetWaitUntil` (HG6), `__nimbusRefreshNodeProcessCwd` (HG7),
  `__nimbusHiddenDenoGlobals`/`__nimbusHiddenNodeGlobals` (HG9, value-freeze).
- **TRUST already-hardened (assert descriptor stays `writable:false,
  configurable:false`):** `__nimbusSyncHostValue`, `__nimbusAsyncHostValue`,
  `__nimbusCallDetachedFromInvocationContext`, `__nimbusRuntimeEnvironmentLane`,
  `__nimbusBeginGuestInvocation`, `__nimbusInstallGuestSemantics`,
  `__nimbusEnterGuestImportPhase`.
- **INTENTIONALLY-MUTABLE (assert present-and-mutable is acceptable):**
  `__nimbusCloudFunctionsState`, `__nimbusAdminApps` (Cloud Functions bundles only).
- **COMPAT-OR-TEST (assert deleted post-bootstrap OR present-non-authority):**
  deleted → `__nimbusRefreshNodeRuntimeOpState`, `__nimbusDenoFetchModule`,
  `__nimbusRetainDenoForNodeLazyScripts`; present → `__nimbusNodeRuntimeMajor`,
  `__nimbusDrainImmediates`, `__nimbusFlushEmbeddedTests`, the Node/worker
  plumbing hooks.
- **Global-lexical (separate from ownKeys — assert bare-name write is
  neutralized after the owning band):** `__nimbusInvocationGeneration` (HG8),
  `__nimbusWaitUntilQueue` (HG6), and the `__nimbusCoreOps` bare-name alias (HG3).

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
