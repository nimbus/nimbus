mod async_effects;
mod async_query;
mod nested_runtime;
mod shared;
mod sync_query_builder;
mod sync_services;

use self::async_effects::{
    op_neovex_ctx_action, op_neovex_ctx_db_delete, op_neovex_ctx_db_insert, op_neovex_ctx_db_patch,
    op_neovex_ctx_mutation, op_neovex_ctx_scheduler_cancel, op_neovex_ctx_scheduler_run_after,
    op_neovex_ctx_scheduler_run_at, op_neovex_http_route,
};
use self::async_query::{
    op_neovex_ctx_db_get, op_neovex_ctx_paginated_query, op_neovex_ctx_query,
    op_neovex_ctx_query_collect, op_neovex_ctx_query_first, op_neovex_ctx_query_paginate,
    op_neovex_ctx_query_take, op_neovex_ctx_query_unique,
};
use self::nested_runtime::{
    op_neovex_ctx_run_action, op_neovex_ctx_run_mutation, op_neovex_ctx_run_query,
    op_neovex_ctx_runtime_enter_nested_call,
};
use self::sync_query_builder::{
    op_neovex_ctx_query_filter, op_neovex_ctx_query_order, op_neovex_ctx_query_start,
    op_neovex_ctx_query_with_index,
};
use self::sync_services::op_neovex_ctx_service_lookup;
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
        op_neovex_ctx_db_get,
        op_neovex_ctx_db_insert,
        op_neovex_ctx_db_patch,
        op_neovex_ctx_db_delete,
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
        op_neovex_ctx_run_action
    ],
);

pub(crate) fn runtime_extension() -> Extension {
    neovex_runtime_ext::init()
}
