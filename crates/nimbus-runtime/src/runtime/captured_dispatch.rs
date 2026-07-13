//! Host-held capture of the guest-trust invocation entrypoints (HG0/HG5/HG1
//! flagship fix).
//!
//! The codegen-emitted preamble and the guest handler bundle share ONE V8 realm
//! and `globalThis`, so the trusted dispatch entrypoints
//! (`globalThis.__nimbusInvoke`, `globalThis.__nimbusInvokeCloudflareWorkerFetch`)
//! sit on the same mutable object graph guest code can reach. Historically the
//! Rust host string-eval'd `globalThis.__nimbusInvoke(request)` **by name on
//! every invocation** (`invocation.rs`), so a guest handler that reassigned the
//! global in invocation N would be handed the request/args/auth of a later
//! same-tenant invocation N+1 on the same warm isolate.
//!
//! This module removes the guest-writable name lookup from the call path. Once,
//! immediately after a bundle finishes evaluating (and after bootstrap for the
//! Cloudflare entrypoint) — a point at which no guest handler body has run yet —
//! the host reads each entrypoint function and stashes it under a **well-known
//! `v8::Private`** on that realm's global object (`capture_invocation_targets`).
//! Private symbols are a C++/embedder-only channel: guest JavaScript has no API
//! to enumerate, read, or overwrite them (`Object.getOwnPropertySymbols` does
//! not return them). At invocation time the host reads the *captured* function
//! back out of the private slot and calls it directly
//! (`call_captured_invocation`) — never re-reading the guest-writable name. A
//! guest that reassigns or deletes `globalThis.__nimbusInvoke` therefore only
//! sabotages itself.
//!
//! This is the Rust-held / off-graph authority the plan mandates, realised with
//! the mechanism deno_core/rusty_v8 actually expose per realm. deno_core does
//! not surface a public Rust-side per-realm slot map (`OpState` is shared
//! per-isolate, which would collide across the main realm and recycled fresh
//! realms), so the authoritative reference is anchored to each realm's own
//! global via a private symbol — the direct analogue of workerd's internal-field
//! authority (`wrappable.h`), scoped and freed with the realm it belongs to.

use crate::backends::v8::embedder::{JsError, JsRealm, JsRuntime, v8};
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeGuestSemantics;
use crate::runtime::classify::runtime_js_error;

/// The exact string `classify.rs` keys `HeapLimitExceeded` classification on
/// (`is_execution_terminated_error`). Every path that surfaces a V8 execution
/// termination — not just the final entrypoint call, but any allocation or
/// hook call that can also be cut short by the same heap-limit/watchdog trip
/// — must produce this literal message so the classifier recognizes it.
const EXECUTION_TERMINATED: &str = "execution terminated";

fn terminated_error() -> NimbusRuntimeError {
    NimbusRuntimeError::JavaScript(EXECUTION_TERMINATED.to_string())
}

/// Guest-trust entrypoints captured off the guest-reachable graph. Named after
/// the globals they replace so the private-symbol registry stays legible; the
/// `nimbus.captured:` prefix keeps them distinct from any api-private symbol a
/// dependency might register.
const CAPTURED_INVOKE: &str = "nimbus.captured:__nimbusInvoke";
const CAPTURED_CLOUDFLARE_FETCH: &str = "nimbus.captured:__nimbusInvokeCloudflareWorkerFetch";
const CAPTURED_BEGIN_GUEST_INVOCATION: &str = "nimbus.captured:__nimbusBeginGuestInvocation";

/// Public global names read at capture time. Present-or-absent per lane:
/// `__nimbusInvoke` after any function/route bundle evaluates,
/// `__nimbusInvokeCloudflareWorkerFetch` only on the Cloudflare lane,
/// `__nimbusBeginGuestInvocation` only on ConvexDefault guest-semantics lanes.
const GLOBAL_INVOKE: &str = "__nimbusInvoke";
const GLOBAL_CLOUDFLARE_FETCH: &str = "__nimbusInvokeCloudflareWorkerFetch";
const GLOBAL_BEGIN_GUEST_INVOCATION: &str = "__nimbusBeginGuestInvocation";

fn contract(message: impl Into<String>) -> NimbusRuntimeError {
    NimbusRuntimeError::Contract(message.into())
}

fn private_key<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: &str,
) -> Result<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, name)
        .ok_or_else(|| contract("failed to allocate captured-dispatch private symbol name"))?;
    Ok(v8::Private::for_api(scope, Some(name)))
}

/// Read `global[name]`. `Ok(None)` means the property is absent (or
/// explicitly `undefined`/`null`), which is legitimate on lanes that never
/// install this entrypoint. `Err` means the property IS present but is not a
/// callable function — HG5 BEGIN-HOOK FAIL-OPEN (Band B-FIX): a bundle that
/// clobbers a trusted entrypoint name with a non-function value is a defect
/// (or an attack), not an "optional, not on this lane" absence, and must not
/// be treated the same as absence.
fn read_global_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    global: v8::Local<v8::Object>,
    name: &str,
) -> Result<Option<v8::Local<'s, v8::Function>>> {
    let key = v8::String::new(scope, name)
        .ok_or_else(|| contract("failed to allocate captured-dispatch global lookup key"))?
        .into();
    let Some(value) = global.get(scope, key) else {
        return Ok(None);
    };
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    Ok(Some(v8::Local::<v8::Function>::try_from(value).map_err(
        |_| {
            contract(format!(
                "globalThis.{name} is present but is not a function"
            ))
        },
    )?))
}

fn read_captured_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    global: v8::Local<v8::Object>,
    captured_name: &str,
) -> Result<Option<v8::Local<'s, v8::Function>>> {
    let key = private_key(scope, captured_name)?;
    let Some(value) = global.get_private(scope, key) else {
        return Ok(None);
    };
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }
    Ok(Some(v8::Local::<v8::Function>::try_from(value).map_err(
        |_| contract("captured dispatch entrypoint is not callable"),
    )?))
}

/// Capture `global_name` into the `captured_name` private slot. Returns
/// whether an entrypoint was actually captured, so callers can enforce
/// lane-specific requiredness (HG5 BEGIN-HOOK FAIL-OPEN, Band B-FIX).
fn capture_one(
    scope: &mut v8::PinScope,
    global: v8::Local<v8::Object>,
    global_name: &str,
    captured_name: &str,
) -> Result<bool> {
    let Some(func) = read_global_function(scope, global, global_name)? else {
        return Ok(false);
    };
    let key = private_key(scope, captured_name)?;
    // HG5 BEGIN-HOOK FAIL-OPEN (Band B-FIX): `set_private` returns
    // `Option<bool>` — `None` means an exception was thrown, but `Some(false)`
    // means the operation failed *without* throwing. Checking only `.is_none()`
    // let a `Some(false)` failure fall through as if capture had succeeded, so
    // dispatch would later read an empty private slot and treat a genuinely
    // installed entrypoint as absent. Anything other than `Some(true)` is a
    // capture failure.
    if global.set_private(scope, key, func.into()) != Some(true) {
        return Err(contract(format!(
            "failed to capture trusted dispatch entrypoint {global_name} into private slot"
        )));
    }
    Ok(true)
}

/// Capture the trusted invocation entrypoints of `realm` (or the main realm when
/// `realm` is `None`) off the guest-reachable graph. Call this exactly once per
/// realm, immediately after the bundle evaluates and before any guest handler
/// body executes. Idempotent and defensive: an entrypoint that is not installed
/// on this lane is skipped, and re-capturing simply refreshes the private slot.
///
/// A global that IS present but is not callable is always a hard error
/// (`read_global_function`/`capture_one`) — that is never legitimate
/// lane-absence. On top of that, `guest_semantics` enforces the one
/// lane-specific requiredness rule HG5 depends on: `ConvexDefault` guest
/// semantics reconfigure the determinism surface (clock/PRNG) for every
/// invocation via `__nimbusBeginGuestInvocation`
/// (`call_captured_invocation`), so a bundle that fails to install it must
/// fail closed at load time rather than silently running invocations without
/// a determinism reset (Band B-FIX HG5 BEGIN-HOOK FAIL-OPEN).
pub(crate) fn capture_invocation_targets(
    runtime: &mut JsRuntime,
    realm: Option<&JsRealm>,
    guest_semantics: RuntimeGuestSemantics,
) -> Result<()> {
    let context = match realm {
        Some(realm) => realm.context().clone(),
        None => runtime.main_context(),
    };
    let isolate = runtime.v8_isolate();
    v8::scope_with_context!(let scope, isolate, &context);
    let global = scope.get_current_context().global(scope);
    capture_one(scope, global, GLOBAL_INVOKE, CAPTURED_INVOKE)?;
    capture_one(
        scope,
        global,
        GLOBAL_CLOUDFLARE_FETCH,
        CAPTURED_CLOUDFLARE_FETCH,
    )?;
    let begin_captured = capture_one(
        scope,
        global,
        GLOBAL_BEGIN_GUEST_INVOCATION,
        CAPTURED_BEGIN_GUEST_INVOCATION,
    )?;
    if guest_semantics == RuntimeGuestSemantics::ConvexDefault && !begin_captured {
        return Err(contract(
            "ConvexDefault guest-semantics lane requires globalThis.__nimbusBeginGuestInvocation \
             to be installed by the bundle, but it was absent at capture time",
        ));
    }
    Ok(())
}

/// Test-only: true iff the function captured into the `__nimbusInvoke`
/// private slot is the exact same V8 function object as `candidate` (V8
/// strict/reference equality, JavaScript `===`) — not merely behaviorally
/// equivalent. Band B-FIX IDENTITY TEST WEAK: a copyable numeric token
/// surviving a call only proves the impostor didn't run; it does not prove
/// dispatch still holds the ORIGINAL reference captured at load, since an
/// impostor could reproduce any observable return value.
#[cfg(test)]
pub(crate) fn captured_invoke_is(
    runtime: &mut JsRuntime,
    realm: Option<&JsRealm>,
    candidate: &v8::Global<v8::Function>,
) -> Result<bool> {
    let context = match realm {
        Some(realm) => realm.context().clone(),
        None => runtime.main_context(),
    };
    let isolate = runtime.v8_isolate();
    v8::scope_with_context!(let scope, isolate, &context);
    let global = scope.get_current_context().global(scope);
    let Some(captured) = read_captured_function(scope, global, CAPTURED_INVOKE)? else {
        return Ok(false);
    };
    let candidate = v8::Local::new(scope, candidate);
    Ok(captured.strict_equals(candidate.into()))
}

/// Call the captured invocation entrypoint of `realm` (or the main realm when
/// `realm` is `None`) with `request_json`, returning the raw completion value
/// (a promise for the async dispatchers) for the caller's existing
/// resolve/event-loop plumbing — the exact shape `execute_script` returned, so
/// downstream handling is unchanged.
///
/// This reads the *captured* reference, never `globalThis.__nimbusInvoke` by
/// name, so a guest reassignment/deletion of the global cannot redirect it.
pub(crate) fn call_captured_invocation(
    runtime: &mut JsRuntime,
    realm: Option<&JsRealm>,
    request_json: &str,
    guest_semantics: RuntimeGuestSemantics,
    cloudflare_module_specifier: Option<&str>,
) -> Result<v8::Global<v8::Value>> {
    // The Cloudflare entrypoint takes the guest module namespace as its first
    // argument. `import(specifier)` is a syntactic form, not a global-property
    // lookup, so a guest cannot hijack it; evaluating this trusted, host-built
    // specifier is safe and yields the namespace promise the entrypoint expects.
    //
    // HG5 (Band B-FIX, CLOUDFLARE REALM ISOLATION): this import must run in
    // the same realm as the captured entrypoint and the eventual dispatch.
    // `JsRuntime::execute_script` always evaluates in deno_core's main realm
    // regardless of which realm is logically "current" (it dispatches to
    // `self.inner.main_realm` unconditionally), so calling it here would
    // resolve and cache the worker module's singletons in the main realm
    // even when a fresh/recycled `realm` is present, defeating fresh-realm
    // isolation under `WarmContextRecycle`. Evaluate in `realm` explicitly
    // whenever one is given, and only fall back to the runtime's own
    // `execute_script` (main realm) when there is no realm at all.
    let module_namespace_promise = match cloudflare_module_specifier {
        Some(specifier) => {
            let specifier_json = serde_json::to_string(specifier)?;
            let source = format!("import({specifier_json})");
            let result = match realm {
                Some(realm) => realm.execute_script(
                    runtime.v8_isolate(),
                    "<nimbus-runtime:cloudflare-module-namespace>",
                    source,
                ),
                None => {
                    runtime.execute_script("<nimbus-runtime:cloudflare-module-namespace>", source)
                }
            };
            Some(result.map_err(runtime_js_error)?)
        }
        None => None,
    };

    let context = match realm {
        Some(realm) => realm.context().clone(),
        None => runtime.main_context(),
    };
    let isolate = runtime.v8_isolate();
    v8::scope_with_context!(let scope, isolate, &context);
    let global = scope.get_current_context().global(scope);
    let receiver: v8::Local<v8::Value> = v8::undefined(scope).into();

    // HG5 (Band B-FIX, ERROR CLASSIFICATION): the TryCatch is established
    // before the request-string allocation and JSON parse below, not just
    // around the entrypoint call. A heap-limit/watchdog termination can fire
    // during either of those V8 allocations too; if the TryCatch only wrapped
    // the entrypoint call, a termination there would surface as a bespoke
    // Contract error instead of the "execution terminated" string
    // `classify.rs` keys `HeapLimitExceeded` classification on, silently
    // misclassifying a heap-limit trip as a plain request-shape defect.
    v8::tc_scope!(let scope, scope);

    // Classify a `None` completion under the active TryCatch above: execution
    // termination (heap-limit/watchdog) always normalizes to the exact
    // `EXECUTION_TERMINATED` string `classify.rs` keys `HeapLimitExceeded` on;
    // an ordinary exception is formatted with deno_core's stack-aware
    // `JsError` (matching the diagnostics guest-facing stack traces get
    // elsewhere in the runtime) rather than a lossy plain string conversion
    // that drops frame/location information (Band B-FIX ERROR CLASSIFICATION).
    // A local macro rather than a closure/function: `scope`'s concrete
    // deno_core/rusty_v8 TryCatch type is unnameable here without spelling
    // out its full lifetime-parameterized generics, and a closure's elided
    // parameter type cannot be inferred purely from later call sites.
    macro_rules! classify {
        ($fallback:expr) => {{
            if scope.is_execution_terminating() {
                terminated_error()
            } else {
                match scope.exception() {
                    Some(exception) => NimbusRuntimeError::JavaScript(
                        JsError::from_v8_exception(scope, exception).to_string(),
                    ),
                    None => NimbusRuntimeError::JavaScript($fallback.to_string()),
                }
            }
        }};
    }

    let request_string = match v8::String::new(scope, request_json) {
        Some(value) => value,
        None if scope.is_execution_terminating() => return Err(terminated_error()),
        None => return Err(contract("invocation request JSON exceeds V8 string limits")),
    };
    let request_value = match v8::json::parse(scope, request_string) {
        Some(value) => value,
        None if scope.is_execution_terminating() => return Err(terminated_error()),
        None => {
            return Err(contract(
                "failed to parse invocation request JSON in the runtime realm",
            ));
        }
    };

    let completion = if let Some(promise) = module_namespace_promise {
        let fetch =
            read_captured_function(scope, global, CAPTURED_CLOUDFLARE_FETCH)?.ok_or_else(|| {
                contract("captured __nimbusInvokeCloudflareWorkerFetch entrypoint is missing")
            })?;
        let namespace = v8::Local::new(scope, &promise);
        fetch.call(scope, receiver, &[namespace, request_value])
    } else {
        // Mirror the ConvexDefault invoke prelude
        // `(globalThis.__nimbusBeginGuestInvocation(), globalThis.__nimbusInvoke(request))`:
        // reconfigure the guest-semantics surface (clock/PRNG determinism) for
        // this invocation before dispatching. `capture_invocation_targets`
        // already refuses to load a ConvexDefault bundle that never installed
        // this hook, so its absence here would be a capture/dispatch
        // invariant violation, not a legitimate optional-hook lane.
        if guest_semantics == RuntimeGuestSemantics::ConvexDefault {
            let begin = read_captured_function(scope, global, CAPTURED_BEGIN_GUEST_INVOCATION)?
                .ok_or_else(|| {
                    contract(
                        "captured __nimbusBeginGuestInvocation entrypoint is missing on a \
                         ConvexDefault guest-semantics lane",
                    )
                })?;
            // Band B-FIX DETERMINISM-HOOK SWALLOWED: the prior code discarded
            // this call's result. If the determinism hook throws or is
            // terminated, the invocation must abort here — proceeding to
            // `invoke.call` below would run guest code against a clock/PRNG
            // that was never reconfigured for this invocation.
            if begin.call(scope, receiver, &[]).is_none() {
                return Err(classify!(
                    "__nimbusBeginGuestInvocation faulted before returning a value"
                ));
            }
        }
        let invoke = read_captured_function(scope, global, CAPTURED_INVOKE)?
            .ok_or_else(|| contract("captured __nimbusInvoke entrypoint is missing"))?;
        invoke.call(scope, receiver, &[request_value])
    };

    match completion {
        // Async dispatchers return a promise here rather than resolving it; the
        // caller's existing resolve/event-loop plumbing awaits it, unchanged.
        Some(value) => Ok(v8::Global::new(scope, value)),
        None => Err(classify!(
            "trusted invocation dispatch faulted before returning a value"
        )),
    }
}
