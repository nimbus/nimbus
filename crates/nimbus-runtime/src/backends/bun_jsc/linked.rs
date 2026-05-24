use std::ffi::c_void;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};

use crate::backends::RuntimeBackendInvocation;
use crate::error::{NimbusRuntimeError, Result};
use crate::host::{HostBridge, HostCallCancellation, HostCallRequest};
use crate::limits::RuntimeExecutionAdapterState;

use super::adapter::{BunJscExecutionAdapter, BunJscExecutionAdapterFactory};
use super::pool::BunJscPoolPolicy;

pub(crate) const BUN_JSC_SHARED_LIBRARY_ENV: &str = "NIMBUS_BUN_EMBED_SHARED_LIBRARY";
const BUN_JSC_LINKED_ADAPTER_OUTPUT_CAP: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(test)]
pub(crate) struct BunJscLinkedAdapterSourceContract {
    pub(crate) repository: &'static str,
    pub(crate) source_ref: &'static str,
    pub(crate) git_revision: &'static str,
    pub(crate) proof_target: &'static str,
    pub(crate) simdutf_namespace: &'static str,
    pub(crate) required_exports: &'static [&'static str],
}

#[cfg(test)]
pub(crate) const BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT: BunJscLinkedAdapterSourceContract =
    BunJscLinkedAdapterSourceContract {
        repository: "https://github.com/nimbus/bun",
        source_ref: "bun-v1.4.0-nimbus.4",
        git_revision: "7c6dd4312e437c67a6c4c8cbb252f0d7ae898db8",
        proof_target: "check-bun-embed-shared",
        simdutf_namespace: "nimbus_bun_simdutf",
        required_exports: &[
            "nimbus_bun_embed_probe_construct_and_destroy_vm",
            "nimbus_bun_embed_probe_sync_host_call",
            "nimbus_bun_embed_probe_async_host_call",
            "nimbus_bun_embed_probe_program_bundle_host_calls",
            "nimbus_bun_embed_probe_timeout_and_cancel",
            "nimbus_bun_embed_probe_permission_surface_inventory",
            "nimbus_bun_embed_probe_memory_behavior",
            "nimbus_bun_embed_probe_package_module_policy",
            "nimbus_bun_embed_probe_lifecycle_reuse_stress",
            "nimbus_bun_embed_invoke_program_wrapper_json",
            "nimbus_bun_embed_invoke_program_wrapper_json_with_host_bridge",
        ],
    };

type BunJscProbeFn = unsafe extern "C" fn() -> i32;
type BunJscInvokeProgramWrapperJsonFn = unsafe extern "C" fn(
    bundle_ptr: *const u8,
    bundle_len: usize,
    request_ptr: *const u8,
    request_len: usize,
    output_ptr: *mut u8,
    output_cap: usize,
    output_len: *mut usize,
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
    request_ptr: *const u8,
    request_len: usize,
    output_ptr: *mut u8,
    output_cap: usize,
    output_len: *mut usize,
    host_context: *mut c_void,
    host_call_json: Option<BunJscHostCallJsonFn>,
) -> i32;

#[derive(Debug)]
struct BunJscSharedAdapterLibrary {
    _library: libloading::Library,
    invoke_program_wrapper_json_with_host_bridge: BunJscInvokeProgramWrapperJsonWithHostBridgeFn,
}

static BUN_JSC_SHARED_ADAPTER_LIBRARY: OnceLock<
    std::result::Result<BunJscSharedAdapterLibrary, String>,
> = OnceLock::new();

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
        if shared_adapter_library().is_ok() {
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

fn invoke_program_wrapper_json(
    invocation: RuntimeBackendInvocation,
    pool_policy: BunJscPoolPolicy,
) -> Result<Value> {
    let shared_library = shared_adapter_library().map_err(|error| {
        NimbusRuntimeError::Contract(format!("Bun/JSC shared adapter is not linked: {error}"))
    })?;
    let RuntimeBackendInvocation {
        policy,
        bundle,
        request,
        cancellation,
        host,
        ..
    } = invocation;

    if cancellation
        .as_ref()
        .is_some_and(crate::host::HostCallCancellation::is_cancelled)
    {
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
    let request_json = serde_json::to_vec(&request)?;
    let mut output = vec![0_u8; BUN_JSC_LINKED_ADAPTER_OUTPUT_CAP];
    let mut output_len = 0_usize;
    let host_context = BunJscHostBridgeCallContext {
        host: host.bridge(),
        cancellation: cancellation.unwrap_or_default(),
    };

    let status = unsafe {
        (shared_library.invoke_program_wrapper_json_with_host_bridge)(
            bundle_source.as_ptr(),
            bundle_source.len(),
            request_json.as_ptr(),
            request_json.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut output_len,
            &host_context as *const BunJscHostBridgeCallContext as *mut c_void,
            Some(bun_jsc_host_bridge_call_json),
        )
    };

    if status == 307 {
        return Err(NimbusRuntimeError::Contract(format!(
            "Bun/JSC linked execution response exceeded {} bytes; embedder reported {} bytes",
            output.len(),
            output_len
        )));
    }
    if status != 0 {
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

struct BunJscHostBridgeCallContext {
    host: Arc<dyn HostBridge>,
    cancellation: HostCallCancellation,
}

unsafe extern "C" fn bun_jsc_host_bridge_call_json(
    context: *mut c_void,
    request_ptr: *const u8,
    request_len: usize,
    output_ptr: *mut u8,
    output_cap: usize,
    output_len: *mut usize,
) -> i32 {
    if context.is_null() || request_ptr.is_null() || output_ptr.is_null() || output_len.is_null() {
        return 300;
    }

    // SAFETY: Bun calls this callback synchronously while the Nimbus invocation
    // owns the context stack frame. The callback never stores the reference.
    let context = unsafe { &*(context as *const BunJscHostBridgeCallContext) };
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
    unsafe {
        *output_len = response.len();
    }
    if response.len() > output_cap {
        return 307;
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
        301 => "request_json_not_utf8",
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
        _ => "unknown",
    }
}

fn shared_adapter_library() -> std::result::Result<&'static BunJscSharedAdapterLibrary, &'static str>
{
    match BUN_JSC_SHARED_ADAPTER_LIBRARY.get_or_init(load_shared_adapter_library) {
        Ok(library) => Ok(library),
        Err(error) => Err(error.as_str()),
    }
}

fn load_shared_adapter_library() -> std::result::Result<BunJscSharedAdapterLibrary, String> {
    let library_path = shared_adapter_library_path()?;
    let library = open_shared_adapter_library(&library_path)?;

    for symbol in [
        "nimbus_bun_embed_probe_construct_and_destroy_vm",
        "nimbus_bun_embed_probe_sync_host_call",
        "nimbus_bun_embed_probe_async_host_call",
        "nimbus_bun_embed_probe_program_bundle_host_calls",
        "nimbus_bun_embed_probe_timeout_and_cancel",
        "nimbus_bun_embed_probe_permission_surface_inventory",
        "nimbus_bun_embed_probe_memory_behavior",
        "nimbus_bun_embed_probe_package_module_policy",
        "nimbus_bun_embed_probe_lifecycle_reuse_stress",
    ] {
        let _: BunJscProbeFn = unsafe { load_required_symbol(&library, symbol)? };
    }
    let _: BunJscInvokeProgramWrapperJsonFn =
        unsafe { load_required_symbol(&library, "nimbus_bun_embed_invoke_program_wrapper_json")? };
    let invoke_program_wrapper_json_with_host_bridge = unsafe {
        load_required_symbol(
            &library,
            "nimbus_bun_embed_invoke_program_wrapper_json_with_host_bridge",
        )?
    };

    Ok(BunJscSharedAdapterLibrary {
        _library: library,
        invoke_program_wrapper_json_with_host_bridge,
    })
}

fn shared_adapter_library_path() -> std::result::Result<PathBuf, String> {
    let Some(path) = std::env::var_os(BUN_JSC_SHARED_LIBRARY_ENV) else {
        return Err(format!(
            "set {BUN_JSC_SHARED_LIBRARY_ENV} to libnimbus_bun_jsc_embedder.so/dylib"
        ));
    };
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty() {
        return Err(format!("{BUN_JSC_SHARED_LIBRARY_ENV} is empty"));
    }
    if !path.is_file() {
        return Err(format!(
            "{BUN_JSC_SHARED_LIBRARY_ENV} points to {}, which is not a file",
            path.display()
        ));
    }
    Ok(path)
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
