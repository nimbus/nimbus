mod async_effects;
mod async_query;
mod async_runtime_extension;
mod async_services;
mod nested_runtime;
mod runtime_local;
mod shared;
mod sync_query_builder;

use self::async_effects::{
    op_neovex_ctx_action, op_neovex_ctx_mutation, op_neovex_ctx_scheduler_cancel,
    op_neovex_ctx_scheduler_run_after, op_neovex_ctx_scheduler_run_at, op_neovex_document_delete,
    op_neovex_document_insert, op_neovex_document_patch, op_neovex_http_route,
};
use self::async_query::{
    op_neovex_ctx_paginated_query, op_neovex_ctx_query, op_neovex_ctx_query_collect,
    op_neovex_ctx_query_first, op_neovex_ctx_query_paginate, op_neovex_ctx_query_take,
    op_neovex_ctx_query_unique, op_neovex_document_get,
};
use self::async_runtime_extension::op_neovex_runtime_extension_call;
use self::async_services::op_neovex_ctx_service_lookup;
use self::nested_runtime::{
    op_neovex_ctx_run_action, op_neovex_ctx_run_mutation, op_neovex_ctx_run_query,
    op_neovex_ctx_runtime_enter_nested_call,
};
use self::runtime_local::{
    op_bootstrap_color_depth, op_bootstrap_unstable_args, op_create_worker,
    op_current_thread_cpu_usage, op_host_get_worker_cpu_usage, op_host_post_message,
    op_host_post_message_raw, op_host_recv_ctrl, op_host_recv_message, op_host_recv_message_sync,
    op_host_terminate_worker, op_http_start, op_neovex_runtime_env_get,
    op_neovex_runtime_env_snapshot, op_neovex_runtime_exec_path, op_neovex_runtime_fs_read_file,
    op_neovex_runtime_fs_write_file, op_neovex_runtime_mkdir, op_neovex_runtime_mkdir_sync,
    op_neovex_runtime_read_dir, op_neovex_runtime_read_dir_sync,
    op_neovex_runtime_require_read_file, op_neovex_runtime_require_resolve, op_neovex_runtime_stat,
    op_neovex_runtime_stat_sync, op_neovex_runtime_target_triple, op_set_raw,
};
use self::shared::op_neovex_runtime_contract;
use self::sync_query_builder::{
    op_neovex_ctx_query_filter, op_neovex_ctx_query_order, op_neovex_ctx_query_start,
    op_neovex_ctx_query_with_index,
};
use crate::backends::v8::embedder::{Extension, extension};

extension!(
    neovex_runtime_ext,
    ops = [
        op_neovex_ctx_query_start,
        op_neovex_ctx_query_with_index,
        op_neovex_ctx_query_filter,
        op_neovex_ctx_query_order,
        op_neovex_ctx_query,
        op_neovex_ctx_paginated_query,
        op_neovex_ctx_mutation,
        op_neovex_ctx_action,
        op_neovex_http_route,
        op_neovex_document_get,
        op_neovex_document_insert,
        op_neovex_document_patch,
        op_neovex_document_delete,
        op_neovex_runtime_extension_call,
        op_neovex_ctx_query_collect,
        op_neovex_ctx_query_take,
        op_neovex_ctx_query_paginate,
        op_neovex_ctx_query_first,
        op_neovex_ctx_query_unique,
        op_neovex_ctx_scheduler_run_after,
        op_neovex_ctx_scheduler_run_at,
        op_neovex_ctx_scheduler_cancel,
        op_neovex_ctx_service_lookup,
        op_neovex_ctx_runtime_enter_nested_call,
        op_neovex_ctx_run_query,
        op_neovex_ctx_run_mutation,
        op_neovex_ctx_run_action,
        op_neovex_runtime_contract,
        op_neovex_runtime_fs_read_file,
        op_neovex_runtime_fs_write_file,
        op_neovex_runtime_stat,
        op_neovex_runtime_stat_sync,
        op_neovex_runtime_mkdir,
        op_neovex_runtime_mkdir_sync,
        op_neovex_runtime_read_dir,
        op_neovex_runtime_read_dir_sync,
        op_neovex_runtime_require_resolve,
        op_neovex_runtime_require_read_file,
        op_neovex_runtime_env_get,
        op_neovex_runtime_env_snapshot,
        op_bootstrap_color_depth,
        op_bootstrap_unstable_args,
        op_neovex_runtime_exec_path,
        op_neovex_runtime_target_triple,
        op_current_thread_cpu_usage,
        op_create_worker,
        op_host_terminate_worker,
        op_host_post_message,
        op_host_recv_ctrl,
        op_host_post_message_raw,
        op_host_recv_message,
        op_host_recv_message_sync,
        op_host_get_worker_cpu_usage,
        op_http_start,
        op_set_raw
    ],
);

pub(crate) fn runtime_extension() -> Extension {
    neovex_runtime_ext::init()
}
