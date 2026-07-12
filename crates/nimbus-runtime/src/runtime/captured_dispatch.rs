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

use crate::backends::v8::embedder::{JsRealm, JsRuntime, v8};
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeGuestSemantics;
use crate::runtime::classify::runtime_js_error;

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

fn read_global_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    global: v8::Local<v8::Object>,
    name: &str,
) -> Option<v8::Local<'s, v8::Function>> {
    let key = v8::String::new(scope, name)?.into();
    let value = global.get(scope, key)?;
    v8::Local::<v8::Function>::try_from(value).ok()
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

fn capture_one(
    scope: &mut v8::PinScope,
    global: v8::Local<v8::Object>,
    global_name: &str,
    captured_name: &str,
) -> Result<()> {
    if let Some(func) = read_global_function(scope, global, global_name) {
        let key = private_key(scope, captured_name)?;
        if global.set_private(scope, key, func.into()).is_none() {
            return Err(contract(format!(
                "failed to capture trusted dispatch entrypoint {global_name} into private slot"
            )));
        }
    }
    Ok(())
}

/// Capture the trusted invocation entrypoints of `realm` (or the main realm when
/// `realm` is `None`) off the guest-reachable graph. Call this exactly once per
/// realm, immediately after the bundle evaluates and before any guest handler
/// body executes. Idempotent and defensive: an entrypoint that is not installed
/// on this lane is skipped, and re-capturing simply refreshes the private slot.
pub(crate) fn capture_invocation_targets(
    runtime: &mut JsRuntime,
    realm: Option<&JsRealm>,
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
    capture_one(
        scope,
        global,
        GLOBAL_BEGIN_GUEST_INVOCATION,
        CAPTURED_BEGIN_GUEST_INVOCATION,
    )?;
    Ok(())
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
    let module_namespace_promise = match cloudflare_module_specifier {
        Some(specifier) => {
            let specifier_json = serde_json::to_string(specifier)?;
            let source = format!("import({specifier_json})");
            Some(
                runtime
                    .execute_script("<nimbus-runtime:cloudflare-module-namespace>", source)
                    .map_err(runtime_js_error)?,
            )
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

    let request_string = v8::String::new(scope, request_json)
        .ok_or_else(|| contract("invocation request JSON exceeds V8 string limits"))?;
    let request_value = v8::json::parse(scope, request_string)
        .ok_or_else(|| contract("failed to parse invocation request JSON in the runtime realm"))?;

    // A TryCatch so a synchronous fault in the entrypoint — most importantly V8
    // terminating execution on a heap-limit or watchdog trip — is surfaced with
    // the same "execution terminated" signal the error classifier keys on
    // (`classify.rs`) as the prior `execute_script` dispatch path produced.
    v8::tc_scope!(let scope, scope);

    let completion = if let Some(promise) = module_namespace_promise {
        let fetch =
            read_captured_function(scope, global, CAPTURED_CLOUDFLARE_FETCH)?.ok_or_else(|| {
                contract("captured __nimbusInvokeCloudflareWorkerFetch entrypoint is missing")
            })?;
        let namespace = v8::Local::new(scope, &promise);
        fetch.call(scope, receiver, &[namespace, request_value])
    } else {
        // Mirror the ConvexDefault invoke prelude
        // `(globalThis.__nimbusBeginGuestInvocation?.(), globalThis.__nimbusInvoke(request))`:
        // reconfigure the guest-semantics surface for this invocation before
        // dispatching. The hook is optional (absent on Host-semantics lanes).
        if guest_semantics == RuntimeGuestSemantics::ConvexDefault
            && let Some(begin) =
                read_captured_function(scope, global, CAPTURED_BEGIN_GUEST_INVOCATION)?
        {
            begin.call(scope, receiver, &[]);
        }
        let invoke = read_captured_function(scope, global, CAPTURED_INVOKE)?
            .ok_or_else(|| contract("captured __nimbusInvoke entrypoint is missing"))?;
        invoke.call(scope, receiver, &[request_value])
    };

    match completion {
        // Async dispatchers return a promise here rather than resolving it; the
        // caller's existing resolve/event-loop plumbing awaits it, unchanged.
        Some(value) => Ok(v8::Global::new(scope, value)),
        None if scope.is_execution_terminating() => Err(NimbusRuntimeError::JavaScript(
            "execution terminated".to_string(),
        )),
        None => {
            let message = scope
                .exception()
                .map(|exception| exception.to_rust_string_lossy(scope))
                .unwrap_or_else(|| {
                    "trusted invocation dispatch faulted before returning a value".to_string()
                });
            Err(NimbusRuntimeError::JavaScript(message))
        }
    }
}
