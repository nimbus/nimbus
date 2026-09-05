use std::borrow::Cow;
use std::ffi::c_void;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::backends::RuntimeBackendInvocation;
use crate::error::{NimbusRuntimeError, Result};
use crate::host::{HostBridge, HostCallCancellation, HostCallRequest};
use crate::limits::{RuntimeExecutionAdapterArtifactDiagnostics, RuntimeExecutionAdapterState};

use super::adapter::{BunJscExecutionAdapter, BunJscExecutionAdapterFactory};
pub(crate) use super::contract::BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT;
use super::manifest;
use super::pool::BunJscPoolPolicy;

const BUN_JSC_LINKED_ADAPTER_OUTPUT_CAP: usize = 4 * 1024 * 1024;

type BunJscArtifactDiagnostics = RuntimeExecutionAdapterArtifactDiagnostics;
type SharedAdapterLibraryResult =
    std::result::Result<&'static BunJscSharedAdapterLibrary, manifest::BunJscAdapterDiscoveryError>;
type SharedAdapterLoadResult =
    std::result::Result<BunJscSharedAdapterLibrary, manifest::BunJscAdapterDiscoveryError>;

type BunJscProbeFn = unsafe extern "C" fn() -> i32;
type BunJscIsCancelledFn = unsafe extern "C" fn(context: *mut c_void) -> bool;
type BunJscInvokeProgramWrapperJsonFn = unsafe extern "C" fn(
    bundle_ptr: *const u8,
    bundle_len: usize,
    expected_sha256_ptr: *const u8,
    expected_sha256_len: usize,
    request_ptr: *const u8,
    request_len: usize,
    output_ptr: *mut u8,
    output_cap: usize,
    output_len: *mut usize,
    cancellation_context: *mut c_void,
    is_cancelled: Option<BunJscIsCancelledFn>,
) -> i32;
type BunJscHostCallJsonFn = unsafe extern "C" fn(
    context: *mut c_void,
    request_ptr: *const u8,
    request_len: usize,
    output_ptr: *mut u8,
    output_cap: usize,
    output_len: *mut usize,
) -> i32;
type BunJscInvokeProgramWrapperJsonWithHostBridgeFn = unsafe extern "C" fn(
    bundle_ptr: *const u8,
    bundle_len: usize,
    expected_sha256_ptr: *const u8,
    expected_sha256_len: usize,
    request_ptr: *const u8,
    request_len: usize,
    output_ptr: *mut u8,
    output_cap: usize,
    output_len: *mut usize,
    host_context: *mut c_void,
    host_call_json: Option<BunJscHostCallJsonFn>,
    cancellation_context: *mut c_void,
    is_cancelled: Option<BunJscIsCancelledFn>,
) -> i32;
type BunJscTakePendingResponseFn =
    unsafe extern "C" fn(output_ptr: *mut u8, output_cap: usize, output_len: *mut usize) -> i32;

#[derive(Debug)]
struct BunJscSharedAdapterLibrary {
    _library: libloading::Library,
    invoke_program_wrapper_json_with_host_bridge: BunJscInvokeProgramWrapperJsonWithHostBridgeFn,
    take_pending_response: BunJscTakePendingResponseFn,
}

static BUN_JSC_SHARED_ADAPTER_LIBRARY: OnceLock<BunJscSharedAdapterLibrary> = OnceLock::new();

#[derive(Debug, Default)]
pub(crate) struct BunJscLinkedExecutionAdapterFactory;

impl BunJscExecutionAdapterFactory for BunJscLinkedExecutionAdapterFactory {
    fn create(&self) -> Box<dyn BunJscExecutionAdapter> {
        Box::<BunJscLinkedExecutionAdapter>::default()
    }
}

#[derive(Debug, Default)]
struct BunJscLinkedExecutionAdapter;

impl BunJscExecutionAdapter for BunJscLinkedExecutionAdapter {
    fn state(&self) -> RuntimeExecutionAdapterState {
        if manifest::resolve_shared_adapter_library().is_ok() {
            RuntimeExecutionAdapterState::Linked
        } else {
            RuntimeExecutionAdapterState::NotLinked
        }
    }

    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
        pool_policy: BunJscPoolPolicy,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>> {
        Box::pin(async move { invoke_program_wrapper_json(invocation, pool_policy) })
    }
}

pub(crate) fn execution_adapter_artifact_diagnostics() -> BunJscArtifactDiagnostics {
    match manifest::resolve_shared_adapter_library() {
        Ok(resolved) => resolved.diagnostics,
        Err(error) => error.diagnostics(),
    }
}

fn invoke_program_wrapper_json(
    invocation: RuntimeBackendInvocation,
    pool_policy: BunJscPoolPolicy,
) -> Result<Value> {
    let shared_library = shared_adapter_library().map_err(|error| {
        NimbusRuntimeError::Contract(format!(
            "Bun/JSC shared adapter is not linked: {}",
            error.message()
        ))
    })?;
    let RuntimeBackendInvocation {
        policy,
        bundle,
        request,
        cancellation,
        host,
        ..
    } = invocation;

    let cancellation = cancellation.ok_or_else(|| {
        NimbusRuntimeError::Contract(
            "Bun/JSC linked execution requires a cancellation token".to_string(),
        )
    })?;
    if cancellation.is_cancelled() {
        return Err(NimbusRuntimeError::Cancelled);
    }
    if !pool_policy.outer_quota_required {
        return Err(NimbusRuntimeError::Contract(
            "Bun/JSC linked execution requires the fresh/discard outer-quota pool policy"
                .to_string(),
        ));
    }

    bundle.verify_integrity()?;
    policy.validate_bundle_content_kind(bundle.content_kind())?;

    let bundle_source = std::fs::read(bundle.entrypoint()).map_err(NimbusRuntimeError::from)?;
    // The private adapter ABI always binds the exact bytes that cross it. A
    // computed digest preserves filesystem-trusted bundle semantics; only a
    // recorded expected digest supplies provenance.
    let expected_sha256 = bundle
        .identity()
        .expected_sha256()
        .map(Cow::Borrowed)
        .unwrap_or_else(|| Cow::Owned(format!("{:x}", Sha256::digest(&bundle_source))));
    let expected_sha256 = expected_sha256.as_bytes();
    let request_json = serde_json::to_vec(&request)?;
    let mut output = vec![0_u8; BUN_JSC_LINKED_ADAPTER_OUTPUT_CAP];
    let mut output_len = 0_usize;
    let host_context = BunJscHostBridgeCallContext {
        host: host.bridge(),
        cancellation,
        pending_response: Mutex::new(None),
    };
    let cancellation_context = &host_context as *const BunJscHostBridgeCallContext as *mut c_void;

    let status = unsafe {
        (shared_library.invoke_program_wrapper_json_with_host_bridge)(
            bundle_source.as_ptr(),
            bundle_source.len(),
            expected_sha256.as_ptr(),
            expected_sha256.len(),
            request_json.as_ptr(),
            request_json.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut output_len,
            &host_context as *const BunJscHostBridgeCallContext as *mut c_void,
            Some(bun_jsc_host_bridge_call_json),
            cancellation_context,
            Some(bun_jsc_is_cancelled),
        )
    };

    if status == 314 || host_context.cancellation.is_cancelled() {
        return Err(NimbusRuntimeError::Cancelled);
    }
    if status == 307 {
        if output_len <= output.len() {
            return Err(NimbusRuntimeError::Contract(format!(
                "Bun/JSC linked execution reported overflow for invalid response length {} and {} byte buffer",
                output_len,
                output.len()
            )));
        }
        output =
            take_completed_invocation_response(shared_library.take_pending_response, output_len)?;
        output_len = output.len();
    } else if status != 0 {
        return Err(NimbusRuntimeError::Contract(format!(
            "Bun/JSC linked execution failed with embedder status {status} ({})",
            embedder_status_name(status)
        )));
    }
    if output_len > output.len() {
        return Err(NimbusRuntimeError::Contract(format!(
            "Bun/JSC linked execution reported invalid response length {} for {} byte buffer",
            output_len,
            output.len()
        )));
    }

    serde_json::from_slice(&output[..output_len]).map_err(NimbusRuntimeError::from)
}

fn take_completed_invocation_response(
    take_pending_response: BunJscTakePendingResponseFn,
    required_len: usize,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    output.try_reserve_exact(required_len).map_err(|error| {
        NimbusRuntimeError::Contract(format!(
            "Bun/JSC linked execution could not reserve {required_len} bytes for its completed response: {error}"
        ))
    })?;
    output.resize(required_len, 0);
    let mut output_len = 0_usize;
    let status =
        unsafe { take_pending_response(output.as_mut_ptr(), output.len(), &mut output_len) };
    if status != 0 {
        return Err(NimbusRuntimeError::Contract(format!(
            "Bun/JSC linked execution could not retrieve its completed response: embedder status {status} ({})",
            embedder_status_name(status)
        )));
    }
    if output_len != required_len {
        return Err(NimbusRuntimeError::Contract(format!(
            "Bun/JSC linked execution changed its completed response length from {required_len} to {output_len} bytes"
        )));
    }
    Ok(output)
}

struct BunJscHostBridgeCallContext {
    host: Arc<dyn HostBridge>,
    cancellation: HostCallCancellation,
    pending_response: Mutex<Option<Vec<u8>>>,
}

unsafe extern "C" fn bun_jsc_is_cancelled(context: *mut c_void) -> bool {
    if context.is_null() {
        return true;
    }

    // SAFETY: Bun calls this callback synchronously while the Nimbus invocation
    // owns the context stack frame. The callback never stores the reference.
    let context = unsafe { &*(context as *const BunJscHostBridgeCallContext) };
    context.cancellation.is_cancelled()
}

unsafe extern "C" fn bun_jsc_host_bridge_call_json(
    context: *mut c_void,
    request_ptr: *const u8,
    request_len: usize,
    output_ptr: *mut u8,
    output_cap: usize,
    output_len: *mut usize,
) -> i32 {
    if context.is_null() || output_ptr.is_null() || output_len.is_null() {
        return 300;
    }

    // SAFETY: Bun calls this callback synchronously while the Nimbus invocation
    // owns the context stack frame. The callback never stores the reference.
    let context = unsafe { &*(context as *const BunJscHostBridgeCallContext) };
    if request_ptr.is_null() {
        if request_len != 0 {
            return 300;
        }
        let mut pending = context
            .pending_response
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(response) = pending.as_ref() else {
            return 320;
        };
        unsafe {
            *output_len = response.len();
        }
        if response.len() > output_cap {
            return 307;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(response.as_ptr(), output_ptr, response.len());
        }
        pending.take();
        return 0;
    }
    context
        .pending_response
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    // SAFETY: Bun passes an immutable request buffer for the duration of this
    // callback. The slice is deserialized before the function returns.
    let request = unsafe { std::slice::from_raw_parts(request_ptr, request_len) };
    let response = match serde_json::from_slice::<HostCallRequest>(request) {
        Ok(request) => match context
            .host
            .call_cancellable(request, &context.cancellation)
        {
            Ok(value) => json!({ "status": "ok", "value": value }),
            Err(NimbusRuntimeError::Cancelled) => json!({
                "status": "error",
                "error": {
                    "code": "cancelled",
                    "message": "Bun/JSC host call was cancelled",
                },
            }),
            Err(error) => json!({
                "status": "error",
                "error": {
                    "code": "host_bridge_denied",
                    "message": error.to_string(),
                },
            }),
        },
        Err(error) => json!({
            "status": "error",
            "error": {
                "code": "invalid_host_bridge_request",
                "message": error.to_string(),
            },
        }),
    };

    let response = match serde_json::to_vec(&response) {
        Ok(response) => response,
        Err(_) => return 312,
    };
    if response.len() > output_cap {
        // SAFETY: `output_len` was validated non-null above. On overflow the
        // ABI reports the required capacity and performs no buffer write.
        unsafe {
            *output_len = response.len();
        }
        context
            .pending_response
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .replace(response);
        return 307;
    }
    // SAFETY: `output_len` was validated non-null above and the capacity check
    // has established that this length fits the caller-provided output buffer.
    unsafe {
        *output_len = response.len();
    }
    // SAFETY: `output_ptr` was validated non-null and the capacity check bounds
    // the copy into the caller-provided ABI buffer.
    unsafe {
        std::ptr::copy_nonoverlapping(response.as_ptr(), output_ptr, response.len());
    }
    0
}

fn embedder_status_name(status: i32) -> &'static str {
    match status {
        1 => "vm_init_failed",
        300 => "invalid_abi_pointer",
        301 => "invalid_request_json",
        302 => "bundle_evaluation_failed",
        303 => "invocation_evaluation_failed",
        304 => "invocation_promise_rejected",
        305 => "result_json_stringify_failed",
        306 => "result_json_dead_string",
        307 => "output_buffer_too_small",
        308 => "missing_host_bridge_callback",
        309 => "host_bridge_transport_evaluation_failed",
        310 => "host_bridge_transport_initialization_failed",
        311 => "host_bridge_not_installed",
        312 => "host_bridge_response_json_failed",
        313 => "bundle_integrity_failed",
        314 => "invocation_cancelled",
        315 => "native_permission_profile_failed",
        316 => "cancellation_watcher_failed",
        320 => "missing_pending_response",
        _ => "unknown",
    }
}

fn shared_adapter_library() -> SharedAdapterLibraryResult {
    if let Some(library) = BUN_JSC_SHARED_ADAPTER_LIBRARY.get() {
        return Ok(library);
    }

    let library = load_shared_adapter_library()?;
    let _ = BUN_JSC_SHARED_ADAPTER_LIBRARY.set(library);
    Ok(BUN_JSC_SHARED_ADAPTER_LIBRARY
        .get()
        .expect("Bun/JSC shared adapter library should be cached after successful load"))
}

fn load_shared_adapter_library() -> SharedAdapterLoadResult {
    let resolved = manifest::resolve_shared_adapter_library()?;
    let library = open_shared_adapter_library(&resolved.path).map_err(|error| {
        shared_adapter_load_error(&resolved, "shared_library_load_failed", error)
    })?;

    for symbol in &BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT.required_exports[..9] {
        let _: BunJscProbeFn =
            unsafe { load_required_symbol(&library, symbol) }.map_err(|error| {
                shared_adapter_load_error(&resolved, "missing_required_export", error)
            })?;
    }
    let _: BunJscInvokeProgramWrapperJsonFn = unsafe {
        load_required_symbol(&library, "nimbus_bun_embed_invoke_program_wrapper_json")
    }
    .map_err(|error| shared_adapter_load_error(&resolved, "missing_required_export", error))?;
    let invoke_program_wrapper_json_with_host_bridge = unsafe {
        load_required_symbol(
            &library,
            "nimbus_bun_embed_invoke_program_wrapper_json_with_host_bridge",
        )
    }
    .map_err(|error| shared_adapter_load_error(&resolved, "missing_required_export", error))?;
    let take_pending_response =
        unsafe { load_required_symbol(&library, "nimbus_bun_embed_take_pending_response") }
            .map_err(|error| {
                shared_adapter_load_error(&resolved, "missing_required_export", error)
            })?;

    Ok(BunJscSharedAdapterLibrary {
        _library: library,
        invoke_program_wrapper_json_with_host_bridge,
        take_pending_response,
    })
}

fn shared_adapter_load_error(
    resolved: &manifest::ResolvedBunJscAdapterLibrary,
    reason_code: &'static str,
    error: String,
) -> manifest::BunJscAdapterDiscoveryError {
    manifest::load_error_diagnostics(resolved.diagnostics.clone(), reason_code, error)
}

#[cfg(unix)]
fn open_shared_adapter_library(path: &Path) -> std::result::Result<libloading::Library, String> {
    use libloading::os::unix::{Library, RTLD_LOCAL, RTLD_NOW};

    let library = unsafe { Library::open(Some(path), RTLD_NOW | RTLD_LOCAL) }
        .map_err(|error| format!("failed to load {}: {error}", path.display()))?;
    Ok(library.into())
}

#[cfg(not(unix))]
fn open_shared_adapter_library(path: &Path) -> std::result::Result<libloading::Library, String> {
    let _ = path;
    Err("Bun/JSC shared adapter loading is currently implemented only for Unix targets".to_string())
}

unsafe fn load_required_symbol<T: Copy>(
    library: &libloading::Library,
    symbol: &'static str,
) -> std::result::Result<T, String> {
    let loaded = unsafe { library.get::<T>(symbol.as_bytes()) }
        .map_err(|error| format!("missing Bun/JSC shared adapter symbol {symbol}: {error}"))?;
    Ok(*loaded)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::host::HostCallOperation;

    #[derive(Debug)]
    struct FixedResponseHost(Value);

    impl HostBridge for FixedResponseHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Ok(self.0.clone())
        }
    }

    fn host_bridge_context(value: Value) -> BunJscHostBridgeCallContext {
        BunJscHostBridgeCallContext {
            host: Arc::new(FixedResponseHost(value)),
            cancellation: HostCallCancellation::default(),
            pending_response: Mutex::new(None),
        }
    }

    fn serialized_host_call_request() -> Vec<u8> {
        serde_json::to_vec(&HostCallRequest::new(
            HostCallOperation::RuntimeExtensionCall,
            json!({}),
        ))
        .expect("host call request should serialize")
    }

    unsafe extern "C" fn take_fixed_completed_response(
        output_ptr: *mut u8,
        output_cap: usize,
        output_len: *mut usize,
    ) -> i32 {
        const RESPONSE: &[u8] = br#"{"status":"ok","value":{"completed":true}}"#;
        if output_ptr.is_null() || output_len.is_null() {
            return 300;
        }
        unsafe {
            *output_len = RESPONSE.len();
        }
        if output_cap < RESPONSE.len() {
            return 307;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(RESPONSE.as_ptr(), output_ptr, RESPONSE.len());
        }
        0
    }

    #[test]
    fn completed_response_is_retrieved_without_invocation_replay() {
        const RESPONSE_LEN: usize = br#"{"status":"ok","value":{"completed":true}}"#.len();

        let response =
            take_completed_invocation_response(take_fixed_completed_response, RESPONSE_LEN)
                .expect("completed response should be retrieved");

        assert_eq!(
            serde_json::from_slice::<Value>(&response).expect("response should remain valid JSON"),
            json!({ "status": "ok", "value": { "completed": true } })
        );
    }

    #[test]
    fn host_bridge_callback_reports_required_length_without_copy_on_overflow() {
        let context = host_bridge_context(json!("response larger than the output buffer"));
        let request = serialized_host_call_request();
        let expected_response = serde_json::to_vec(&json!({
            "status": "ok",
            "value": "response larger than the output buffer",
        }))
        .expect("expected response should serialize");
        let mut output = [0xA5_u8; 8];
        let mut output_len = usize::MAX;

        let status = unsafe {
            bun_jsc_host_bridge_call_json(
                &context as *const BunJscHostBridgeCallContext as *mut c_void,
                request.as_ptr(),
                request.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut output_len,
            )
        };

        assert_eq!(status, 307);
        assert_eq!(output_len, expected_response.len());
        assert_eq!(output, [0xA5_u8; 8]);

        let mut recovered = vec![0_u8; output_len];
        let mut recovered_len = 0;
        let status = unsafe {
            bun_jsc_host_bridge_call_json(
                &context as *const BunJscHostBridgeCallContext as *mut c_void,
                std::ptr::null(),
                0,
                recovered.as_mut_ptr(),
                recovered.len(),
                &mut recovered_len,
            )
        };
        assert_eq!(status, 0);
        assert_eq!(recovered_len, expected_response.len());
        assert_eq!(&recovered[..recovered_len], expected_response.as_slice());

        let status = unsafe {
            bun_jsc_host_bridge_call_json(
                &context as *const BunJscHostBridgeCallContext as *mut c_void,
                std::ptr::null(),
                0,
                recovered.as_mut_ptr(),
                recovered.len(),
                &mut recovered_len,
            )
        };
        assert_eq!(status, 320);
    }

    #[test]
    fn host_bridge_callback_sets_output_length_after_successful_capacity_check() {
        let context = host_bridge_context(json!("ok"));
        let request = serialized_host_call_request();
        let expected_response = serde_json::to_vec(&json!({
            "status": "ok",
            "value": "ok",
        }))
        .expect("expected response should serialize");
        let mut output = [0_u8; 128];
        let mut output_len = usize::MAX;

        let status = unsafe {
            bun_jsc_host_bridge_call_json(
                &context as *const BunJscHostBridgeCallContext as *mut c_void,
                request.as_ptr(),
                request.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut output_len,
            )
        };

        assert_eq!(status, 0);
        assert_eq!(output_len, expected_response.len());
        assert_eq!(&output[..output_len], expected_response.as_slice());
    }

    #[test]
    fn cancellation_callback_reads_the_invocation_signal() {
        let context = host_bridge_context(json!(null));
        let context_ptr = &context as *const BunJscHostBridgeCallContext as *mut c_void;

        assert!(!unsafe { bun_jsc_is_cancelled(context_ptr) });
        context.cancellation.cancel();
        assert!(unsafe { bun_jsc_is_cancelled(context_ptr) });
        assert!(unsafe { bun_jsc_is_cancelled(std::ptr::null_mut()) });
    }

    #[test]
    fn embedder_status_names_cover_fail_closed_invocation_setup() {
        assert_eq!(
            embedder_status_name(315),
            "native_permission_profile_failed"
        );
        assert_eq!(embedder_status_name(316), "cancellation_watcher_failed");
        assert_eq!(embedder_status_name(320), "missing_pending_response");
    }
}
