use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::backends::RuntimeBackendInvocation;
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::RuntimeExecutionAdapterState;

use super::adapter::{BunJscExecutionAdapter, BunJscExecutionAdapterFactory};
use super::pool::BunJscPoolPolicy;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BunJscLinkedAdapterSourceContract {
    pub(crate) git_revision: &'static str,
    pub(crate) proof_target: &'static str,
    pub(crate) required_exports: &'static [&'static str],
}

pub(crate) const BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT: BunJscLinkedAdapterSourceContract =
    BunJscLinkedAdapterSourceContract {
        git_revision: "2f09ba33b184a541e2ade24bf6e46bebc971a262",
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
        drop(invocation);
        let _ = pool_policy;
        Box::pin(async {
            Err(NimbusRuntimeError::Contract(
                "BJA3 compiled the Bun/JSC linked adapter lane, but BJA4 has not wired Bun/JSC execution yet".to_string(),
            ))
        })
    }
}
