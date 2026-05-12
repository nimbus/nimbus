mod async_effects;
mod async_query;
mod async_runtime_extension;
mod async_services;
mod nested_runtime;
mod runtime_local;
mod shared;
mod sync_query_builder;
#[cfg(test)]
mod test_runtime;
mod worker_threads;

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
    op_bootstrap_color_depth, op_bootstrap_unstable_args, op_http_start, op_neovex_runtime_chmod,
    op_neovex_runtime_chmod_sync, op_neovex_runtime_copy_file, op_neovex_runtime_copy_file_sync,
    op_neovex_runtime_env_get, op_neovex_runtime_env_snapshot, op_neovex_runtime_exec_path,
    op_neovex_runtime_fs_read_file, op_neovex_runtime_fs_write_file, op_neovex_runtime_link,
    op_neovex_runtime_link_sync, op_neovex_runtime_mkdir, op_neovex_runtime_mkdir_sync,
    op_neovex_runtime_read_dir, op_neovex_runtime_read_dir_sync, op_neovex_runtime_read_link,
    op_neovex_runtime_read_link_sync, op_neovex_runtime_remove, op_neovex_runtime_remove_sync,
    op_neovex_runtime_rename, op_neovex_runtime_rename_sync, op_neovex_runtime_require_read_file,
    op_neovex_runtime_require_resolve, op_neovex_runtime_shared_env_delete,
    op_neovex_runtime_shared_env_get, op_neovex_runtime_shared_env_seed,
    op_neovex_runtime_shared_env_set, op_neovex_runtime_shared_env_snapshot,
    op_neovex_runtime_stat, op_neovex_runtime_stat_sync, op_neovex_runtime_symlink,
    op_neovex_runtime_symlink_sync, op_neovex_runtime_target_triple, op_neovex_runtime_utime,
    op_neovex_runtime_utime_sync, op_neovex_runtime_validate_open_path, op_set_raw,
};
use self::shared::op_neovex_runtime_contract;
use self::sync_query_builder::{
    op_neovex_ctx_query_filter, op_neovex_ctx_query_order, op_neovex_ctx_query_start,
    op_neovex_ctx_query_with_index,
};
#[cfg(test)]
use self::test_runtime::{
    op_neovex_runtime_test_force_gc, op_neovex_runtime_test_spawn,
    op_neovex_runtime_test_spawn_sync,
};
use self::worker_threads::{
    op_create_worker, op_current_thread_cpu_usage, op_host_get_worker_cpu_usage,
    op_host_post_message, op_host_post_message_raw, op_host_recv_ctrl, op_host_recv_ctrl_sync,
    op_host_recv_message, op_host_recv_message_sync, op_host_terminate_worker,
    op_neovex_worker_bootstrap_state, op_neovex_worker_parent_post_message,
    op_neovex_worker_parent_post_message_raw, op_neovex_worker_parent_recv_message,
    op_neovex_worker_parent_recv_message_sync,
};
use crate::backends::v8::embedder::{Extension, extension};

pub(crate) use self::worker_threads::worker_threads_state_extension;

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
        op_neovex_runtime_validate_open_path,
        op_neovex_runtime_copy_file,
        op_neovex_runtime_copy_file_sync,
        op_neovex_runtime_link,
        op_neovex_runtime_link_sync,
        op_neovex_runtime_stat,
        op_neovex_runtime_stat_sync,
        op_neovex_runtime_mkdir,
        op_neovex_runtime_mkdir_sync,
        op_neovex_runtime_chmod,
        op_neovex_runtime_chmod_sync,
        op_neovex_runtime_utime,
        op_neovex_runtime_utime_sync,
        op_neovex_runtime_read_dir,
        op_neovex_runtime_read_dir_sync,
        op_neovex_runtime_remove,
        op_neovex_runtime_remove_sync,
        op_neovex_runtime_symlink,
        op_neovex_runtime_symlink_sync,
        op_neovex_runtime_read_link,
        op_neovex_runtime_read_link_sync,
        op_neovex_runtime_rename,
        op_neovex_runtime_rename_sync,
        op_neovex_runtime_require_resolve,
        op_neovex_runtime_require_read_file,
        op_neovex_runtime_env_get,
        op_neovex_runtime_env_snapshot,
        op_neovex_runtime_shared_env_get,
        op_neovex_runtime_shared_env_seed,
        op_neovex_runtime_shared_env_snapshot,
        op_neovex_runtime_shared_env_set,
        op_neovex_runtime_shared_env_delete,
        op_bootstrap_color_depth,
        op_bootstrap_unstable_args,
        op_neovex_runtime_exec_path,
        op_neovex_runtime_target_triple,
        op_current_thread_cpu_usage,
        op_create_worker,
        op_host_terminate_worker,
        op_host_post_message,
        op_host_recv_ctrl,
        op_host_recv_ctrl_sync,
        op_host_post_message_raw,
        op_host_recv_message,
        op_host_recv_message_sync,
        op_host_get_worker_cpu_usage,
        op_neovex_worker_bootstrap_state,
        op_neovex_worker_parent_post_message,
        op_neovex_worker_parent_post_message_raw,
        op_neovex_worker_parent_recv_message,
        op_neovex_worker_parent_recv_message_sync,
        op_http_start,
        op_set_raw
    ],
);

pub(crate) fn runtime_extension() -> Extension {
    neovex_runtime_ext::init()
}

#[cfg(test)]
extension!(
    neovex_runtime_test_ext,
    ops = [
        op_neovex_runtime_test_spawn,
        op_neovex_runtime_test_spawn_sync,
        op_neovex_runtime_test_force_gc
    ],
);

#[cfg(test)]
pub(crate) fn runtime_test_extension() -> Extension {
    neovex_runtime_test_ext::init()
}
