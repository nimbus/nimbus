use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::backends::RuntimeBackendInvocation;
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeExecutionAdapterState;

use super::adapter::{BunJscExecutionAdapter, BunJscExecutionAdapterFactory};
use super::pool::BunJscPoolPolicy;

#[cfg(nimbus_bun_jsc_linked_ffi)]
const BUN_JSC_LINKED_ADAPTER_OUTPUT_CAP: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BunJscLinkedAdapterSourceContract {
    pub(crate) git_revision: &'static str,
    pub(crate) proof_target: &'static str,
    pub(crate) required_exports: &'static [&'static str],
}

pub(crate) const BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT: BunJscLinkedAdapterSourceContract =
    BunJscLinkedAdapterSourceContract {
        git_revision: "a409f596e8e1394d8860e2cd8b2bb558ff1afcac",
        proof_target: "check-bun-embed-probe",
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
        ],
    };

#[allow(dead_code)]
mod ffi {
    unsafe extern "C" {
        pub(crate) fn nimbus_bun_embed_probe_construct_and_destroy_vm() -> i32;
        pub(crate) fn nimbus_bun_embed_probe_sync_host_call() -> i32;
        pub(crate) fn nimbus_bun_embed_probe_async_host_call() -> i32;
        pub(crate) fn nimbus_bun_embed_probe_program_bundle_host_calls() -> i32;
        pub(crate) fn nimbus_bun_embed_probe_timeout_and_cancel() -> i32;
        pub(crate) fn nimbus_bun_embed_probe_permission_surface_inventory() -> i32;
        pub(crate) fn nimbus_bun_embed_probe_memory_behavior() -> i32;
        pub(crate) fn nimbus_bun_embed_probe_package_module_policy() -> i32;
        pub(crate) fn nimbus_bun_embed_probe_lifecycle_reuse_stress() -> i32;
        pub(crate) fn nimbus_bun_embed_invoke_program_wrapper_json(
            bundle_ptr: *const u8,
            bundle_len: usize,
            request_ptr: *const u8,
            request_len: usize,
            output_ptr: *mut u8,
            output_cap: usize,
            output_len: *mut usize,
        ) -> i32;
    }
}

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
        RuntimeExecutionAdapterState::Linked
    }

    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
        pool_policy: BunJscPoolPolicy,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>> {
        Box::pin(async move { invoke_program_wrapper_json(invocation, pool_policy) })
    }
}

#[cfg(nimbus_bun_jsc_linked_ffi)]
fn invoke_program_wrapper_json(
    invocation: RuntimeBackendInvocation,
    pool_policy: BunJscPoolPolicy,
) -> Result<Value> {
    let RuntimeBackendInvocation {
        policy,
        bundle,
        request,
        cancellation,
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

    let status = unsafe {
        ffi::nimbus_bun_embed_invoke_program_wrapper_json(
            bundle_source.as_ptr(),
            bundle_source.len(),
            request_json.as_ptr(),
            request_json.len(),
            output.as_mut_ptr(),
            output.len(),
            &mut output_len,
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

#[cfg(not(nimbus_bun_jsc_linked_ffi))]
fn invoke_program_wrapper_json(
    invocation: RuntimeBackendInvocation,
    pool_policy: BunJscPoolPolicy,
) -> Result<Value> {
    drop(invocation);
    let _ = pool_policy;
    Err(NimbusRuntimeError::Contract(
        "Bun/JSC linked adapter feature was compiled without NIMBUS_BUN_EMBED_LINK_ARGS; run the linked gate after building Bun's embedder link manifest".to_string(),
    ))
}

#[cfg(nimbus_bun_jsc_linked_ffi)]
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
        _ => "unknown",
    }
}
