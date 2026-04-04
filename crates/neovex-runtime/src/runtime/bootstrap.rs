use std::cell::RefCell;
use std::rc::Rc;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deno_core::JsRuntimeForSnapshot;
use deno_core::{CancelFuture, CancelHandle, JsRuntime, OpState, RuntimeOptions, extension, op2};
use deno_error::JsErrorBox;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{InvocationAuth, NeovexRuntime, RuntimeBundle};
use crate::error::{NeovexRuntimeError, Result};
use crate::executor::SharedInvocationPermit;
use crate::host::{HostBridge, HostCallCancellation, HostCallOperation, HostCallRequest};
use crate::watchdog::{WatchdogRegistration, WatchdogTimer};

#[derive(Clone)]
struct RuntimeHostState {
    bridge: Arc<dyn HostBridge>,
}

#[derive(Clone)]
pub(crate) struct RuntimeCancellationState {
    pub(crate) cancel_handle: Rc<CancelHandle>,
    pub(crate) signal: HostCallCancellation,
}

#[derive(Clone)]
pub(crate) struct RuntimeInvocationTimeoutController {
    inner: Arc<Mutex<RuntimeInvocationTimeoutControllerState>>,
}

struct RuntimeInvocationTimeoutControllerState {
    timer: WatchdogTimer,
    remaining: Duration,
    armed_at: Option<Instant>,
    registration: Option<WatchdogRegistration>,
    callback: Arc<dyn Fn() + Send + Sync>,
    disarmed: bool,
}

impl RuntimeInvocationTimeoutController {
    pub(crate) fn new(
        timer: WatchdogTimer,
        timeout: Duration,
        callback: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let registration = if timeout.is_zero() {
            None
        } else {
            Some(Self::register(&timer, timeout, callback.clone())?)
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(RuntimeInvocationTimeoutControllerState {
                timer,
                remaining: timeout,
                armed_at: (!timeout.is_zero()).then_some(Instant::now()),
                registration,
                callback,
                disarmed: false,
            })),
        })
    }

    fn register(
        timer: &WatchdogTimer,
        timeout: Duration,
        callback: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<WatchdogRegistration> {
        timer.register_timeout(Instant::now() + timeout, move || {
            callback();
        })
    }

    pub(crate) async fn pause(&self) {
        let registration = {
            let mut state = self
                .inner
                .lock()
                .expect("runtime timeout controller lock should not be poisoned");
            if state.disarmed {
                return;
            }
            let Some(armed_at) = state.armed_at.take() else {
                return;
            };
            state.remaining = state.remaining.saturating_sub(armed_at.elapsed());
            state.registration.take()
        };
        if let Some(registration) = registration {
            registration.disarm().await;
        }
    }

    pub(crate) fn resume(&self) -> Result<()> {
        let mut state = self
            .inner
            .lock()
            .expect("runtime timeout controller lock should not be poisoned");
        if state.disarmed || state.remaining.is_zero() || state.registration.is_some() {
            return Ok(());
        }
        let registration = Self::register(&state.timer, state.remaining, state.callback.clone())?;
        state.armed_at = Some(Instant::now());
        state.registration = Some(registration);
        Ok(())
    }

    pub(crate) async fn disarm(&self) {
        let registration = {
            let mut state = self
                .inner
                .lock()
                .expect("runtime timeout controller lock should not be poisoned");
            state.disarmed = true;
            state.armed_at = None;
            state.registration.take()
        };
        if let Some(registration) = registration {
            registration.disarm().await;
        }
    }
}

pub(crate) struct RuntimeStartupSnapshot {
    bytes: &'static [u8],
}

impl RuntimeStartupSnapshot {
    fn new(bytes: Box<[u8]>) -> Self {
        // deno_core currently accepts startup snapshots as &'static [u8]. The
        // worker pool keeps a single bootstrap snapshot for its own lifetime,
        // so leaking one buffer per worker matches the pool's lifetime and
        // avoids unsound lifetime extension tricks.
        Self {
            bytes: Box::leak(bytes),
        }
    }

    pub(crate) fn as_startup_snapshot(&self) -> &'static [u8] {
        self.bytes
    }
}

#[cfg(test)]
static RUNTIME_BOOTSTRAP_SNAPSHOT_BUILDS: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct RuntimeWorkerIsolatePool {
    warmed: bool,
}

impl RuntimeWorkerIsolatePool {
    pub(crate) fn new() -> Self {
        Self { warmed: false }
    }

    pub(crate) fn take_runtime(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
    ) -> Result<JsRuntime> {
        let snapshot = runtime_owner.bootstrap_snapshot()?;
        if self.warmed {
            runtime_owner.policy.metrics().record_isolate_pool_hit();
            runtime_owner.create_runtime_from_snapshot(bundle, snapshot)
        } else {
            runtime_owner.policy.metrics().record_isolate_pool_miss();
            let runtime = runtime_owner.create_runtime_from_snapshot(bundle, snapshot)?;
            self.warmed = true;
            Ok(runtime)
        }
    }

    pub(crate) fn record_replacement(&self, runtime_owner: &NeovexRuntime) {
        runtime_owner
            .policy
            .metrics()
            .record_isolate_pool_replacement();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum RuntimeHostCallEnvelope {
    Ok { value: Value },
    Error { error: Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncQueryPayload {
    query: Value,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncPaginatedQueryPayload {
    query: Value,
    page_size: usize,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncDbGetPayload {
    table: String,
    id: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncMutationPayload {
    mutation: Value,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncActionPayload {
    action: Value,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncHttpRoutePayload {
    request: Value,
    route: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncDbInsertPayload {
    table: String,
    fields: Value,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncDbPatchPayload {
    table: String,
    id: String,
    patch: Value,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncDbDeletePayload {
    table: String,
    id: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeSyncQueryStartPayload {
    table: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeSyncQueryWithIndexPayload {
    builder_id: String,
    index_name: String,
    #[serde(default)]
    filters: Value,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeSyncQueryFilterPayload {
    builder_id: String,
    #[serde(default)]
    filters: Value,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeSyncQueryOrderPayload {
    builder_id: String,
    direction: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncSchedulerRunAfterPayload {
    delay_ms: u64,
    name: String,
    visibility: String,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncSchedulerRunAtPayload {
    timestamp_ms: u64,
    name: String,
    visibility: String,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncSchedulerCancelPayload {
    job_id: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncFunctionCallPayload {
    name: String,
    visibility: String,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auth: Option<InvocationAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeSyncNestedCallPayload {
    name: String,
    visibility: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncQueryTerminalPayload {
    builder_id: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncQueryTakePayload {
    builder_id: String,
    limit: usize,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuntimeAsyncQueryPaginatePayload {
    builder_id: String,
    page_size: usize,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

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
        op_neovex_ctx_runtime_enter_nested_call,
        op_neovex_ctx_run_query,
        op_neovex_ctx_run_mutation,
        op_neovex_ctx_run_action
    ],
);

const BOOTSTRAP_SOURCE: &str = r#"
const __neovexCoreOps = Deno.core.ops;
globalThis.__neovexSyncHostValue = function(opName, payload) {
  const operation = __neovexCoreOps[opName];
  if (typeof operation !== "function") {
    throw new Error(`Neovex runtime sync host op not found: ${opName}`);
  }
  const response = operation(payload);
  if (!response || response.status !== "ok") {
    const error = new Error(
      `Neovex runtime sync host call failed for ${opName}: ${__neovexFormatHostError(response?.error)}`,
    );
    error.neovexHostError = response?.error ?? null;
    throw error;
  }
  return response.value;
};

function __neovexFormatHostError(error) {
  if (error === null || error === undefined) {
    return "unknown host error";
  }
  if (typeof error === "string") {
    return error;
  }
  try {
    return JSON.stringify(error);
  } catch (_error) {
    return String(error);
  }
}

globalThis.__neovexAsyncHostValue = async function(opName, payload) {
  const operation = __neovexCoreOps[opName];
  if (typeof operation !== "function") {
    throw new Error(`Neovex runtime async host op not found: ${opName}`);
  }
  const response = await operation(payload);
  if (!response || response.status !== "ok") {
    const error = new Error(
      `Neovex runtime async host call failed for ${opName}: ${__neovexFormatHostError(response?.error)}`,
    );
    error.neovexHostError = response?.error ?? null;
    throw error;
  }
  return response.value;
};

function __neovexNormalizeFieldName(field) {
  if (typeof field === "string" && field.length > 0) {
    return field;
  }
  if (
    field !== null &&
    typeof field === "object" &&
    typeof field.__fieldName === "string" &&
    field.__fieldName.length > 0
  ) {
    return field.__fieldName;
  }
  throw new Error("ctx.db field constraints require a non-empty field name");
}

function __neovexCreateConstraintBuilder() {
  const filters = [];
  const builder = {
    field(name) {
      return { __fieldName: __neovexNormalizeFieldName(name) };
    },
    eq(field, value) {
      filters.push({ field: __neovexNormalizeFieldName(field), op: "eq", value });
      return builder;
    },
    neq(field, value) {
      filters.push({ field: __neovexNormalizeFieldName(field), op: "neq", value });
      return builder;
    },
    gt(field, value) {
      filters.push({ field: __neovexNormalizeFieldName(field), op: "gt", value });
      return builder;
    },
    gte(field, value) {
      filters.push({ field: __neovexNormalizeFieldName(field), op: "gte", value });
      return builder;
    },
    lt(field, value) {
      filters.push({ field: __neovexNormalizeFieldName(field), op: "lt", value });
      return builder;
    },
    lte(field, value) {
      filters.push({ field: __neovexNormalizeFieldName(field), op: "lte", value });
      return builder;
    },
  };
  return Object.assign(builder, { __filters: filters });
}

function __neovexCollectConstraintFilters(builderFn, label) {
  const builder = __neovexCreateConstraintBuilder();
  const result = builderFn ? builderFn(builder) : builder;
  if (result !== undefined && result !== builder && result?.__filters !== builder.__filters) {
    throw new Error(`ctx.db.${label}(...) must return the provided builder`);
  }
  return [...builder.__filters];
}

function __neovexCreateQueryBuilder(syncHostValue, builderId, sessionId) {
  return Object.freeze({
    __builderId: builderId,
    withIndex(indexName, builderFn) {
      syncHostValue("op_neovex_ctx_query_with_index", {
        builder_id: builderId,
        index_name: indexName,
        filters: __neovexCollectConstraintFilters(builderFn, "withIndex"),
      });
      return __neovexCreateQueryBuilder(syncHostValue, builderId, sessionId);
    },
    filter(builderFn) {
      syncHostValue("op_neovex_ctx_query_filter", {
        builder_id: builderId,
        filters: __neovexCollectConstraintFilters(builderFn, "filter"),
      });
      return __neovexCreateQueryBuilder(syncHostValue, builderId, sessionId);
    },
    order(direction) {
      syncHostValue("op_neovex_ctx_query_order", {
        builder_id: builderId,
        direction,
      });
      return __neovexCreateQueryBuilder(syncHostValue, builderId, sessionId);
    },
    collect() {
      return globalThis.__neovexAsyncHostValue("op_neovex_ctx_query_collect", {
        builder_id: builderId,
        session_id: sessionId,
      });
    },
    take(limit) {
      return globalThis.__neovexAsyncHostValue("op_neovex_ctx_query_take", {
        builder_id: builderId,
        limit,
        session_id: sessionId,
      });
    },
    async paginate(paginationOpts) {
      if (!paginationOpts || typeof paginationOpts !== "object") {
        throw new Error("ctx.db.query(...).paginate(...) requires pagination options");
      }
      if (typeof paginationOpts.numItems !== "number") {
        throw new Error("ctx.db.query(...).paginate(...) requires paginationOpts.numItems");
      }
      const cursor =
        typeof paginationOpts.cursor === "string" ? paginationOpts.cursor : null;
      const page = await globalThis.__neovexAsyncHostValue("op_neovex_ctx_query_paginate", {
        builder_id: builderId,
        page_size: paginationOpts.numItems,
        cursor,
        session_id: sessionId,
      });
      const pageItems = Array.isArray(page?.data) ? page.data : [];
      const hasContinuation =
        typeof page?.next_cursor === "string" &&
        pageItems.length === paginationOpts.numItems &&
        pageItems.length > 0;
      const continueCursor =
        page && typeof page.next_cursor === "string"
          ? page.next_cursor
          : cursor ?? "";
      return {
        page: pageItems,
        isDone: page?.has_more === true ? false : !hasContinuation,
        continueCursor,
        splitCursor: null,
        pageStatus: null,
      };
    },
    first() {
      return globalThis.__neovexAsyncHostValue("op_neovex_ctx_query_first", {
        builder_id: builderId,
        session_id: sessionId,
      });
    },
    unique() {
      return globalThis.__neovexAsyncHostValue("op_neovex_ctx_query_unique", {
        builder_id: builderId,
        session_id: sessionId,
      });
    },
  });
}

function __neovexNormalizeFunctionReference(functionRef, label) {
  if (!functionRef || typeof functionRef !== "object") {
    throw new Error(`ctx.${label}(...) requires a generated function reference`);
  }
  if (typeof functionRef.name !== "string" || functionRef.name.length === 0) {
    throw new Error(`ctx.${label}(...) requires a named generated function reference`);
  }
  return {
    name: functionRef.name,
    visibility: typeof functionRef.visibility === "string" ? functionRef.visibility : "public",
  };
}

async function __neovexRunNamedFunction(
  syncHostValue,
  asyncOpName,
  sessionId,
  authContext,
  kind,
  label,
  functionRef,
  args = {},
) {
  const normalized = __neovexNormalizeFunctionReference(functionRef, label);
  const localInvoker = globalThis.__neovexInvokeNamedLocal;
  const nestedAuthContext = authContext
    ? {
        ...authContext,
        throw_on_missing_identity: false,
      }
    : null;
  if (typeof localInvoker === "function") {
    syncHostValue("op_neovex_ctx_runtime_enter_nested_call", {
      name: normalized.name,
      visibility: normalized.visibility,
      session_id: sessionId,
    });
    return await localInvoker({
      kind,
      function_name: normalized.name,
      args,
      visibility: normalized.visibility,
      ...(nestedAuthContext ? { auth: nestedAuthContext } : {}),
    });
  }
  return globalThis.__neovexAsyncHostValue(asyncOpName, {
    ...normalized,
    args,
    session_id: sessionId,
    ...(nestedAuthContext ? { auth: nestedAuthContext } : {}),
  });
}

let __neovexNextSessionId = 1;

globalThis.__neovexCreateContext = function(options = {}) {
  const sessionId =
    typeof options.sessionId === "string" && options.sessionId.length > 0
      ? options.sessionId
      : `session-${__neovexNextSessionId++}`;
  const requestAuth =
    options.request !== null &&
    typeof options.request === "object" &&
    options.request.auth !== null &&
    typeof options.request.auth === "object"
      ? options.request.auth
      : null;
  const authIdentity =
    requestAuth &&
    requestAuth.identity !== null &&
    typeof requestAuth.identity === "object"
      ? requestAuth.identity
      : null;
  const verifiedAuthIdentity =
    requestAuth &&
    requestAuth.verified_identity !== null &&
    typeof requestAuth.verified_identity === "object"
      ? requestAuth.verified_identity
      : null;
  const throwOnMissingIdentity = requestAuth?.throw_on_missing_identity === true;

  const syncHostValue = (opName, payload) =>
    globalThis.__neovexSyncHostValue(opName, {
      session_id: sessionId,
      ...(payload ?? {}),
    });

  const asyncHostValue = (opName, payload) =>
    globalThis.__neovexAsyncHostValue(opName, {
      session_id: sessionId,
      ...(payload ?? {}),
    });

  const cloneAuthIdentityOrThrow = (identity) => {
    if (identity) {
      return JSON.parse(JSON.stringify(identity));
    }
    if (throwOnMissingIdentity) {
      throw new Error(
        "convex httpAction requires an authenticated identity",
      );
    }
    return null;
  };

  return {
    auth: Object.freeze({
      async getUserIdentity() {
        return cloneAuthIdentityOrThrow(authIdentity);
      },
      async getVerifiedIdentity() {
        return cloneAuthIdentityOrThrow(verifiedAuthIdentity);
      },
    }),
    db: {
      async get(tableOrId, maybeId) {
        if (maybeId === undefined) {
          if (
            tableOrId &&
            typeof tableOrId === "object" &&
            typeof tableOrId.table === "string" &&
            typeof tableOrId.id === "string"
          ) {
            return globalThis.__neovexAsyncHostValue("op_neovex_ctx_db_get", {
              table: tableOrId.table,
              id: tableOrId.id,
              session_id: sessionId,
            });
          }
          throw new Error(
            "Neovex runtime ctx.db.get currently requires table and id at runtime",
          );
        }
        return globalThis.__neovexAsyncHostValue("op_neovex_ctx_db_get", {
          table: tableOrId,
          id: maybeId,
          session_id: sessionId,
        });
      },
      query(table) {
        const builderId = syncHostValue("op_neovex_ctx_query_start", { table });
        return __neovexCreateQueryBuilder(syncHostValue, builderId, sessionId);
      },
      insert(table, fields) {
        return asyncHostValue("op_neovex_ctx_db_insert", {
          table,
          fields,
        });
      },
      patch(table, id, patch) {
        return asyncHostValue("op_neovex_ctx_db_patch", {
          table,
          id,
          patch,
        });
      },
      delete(table, id) {
        return asyncHostValue("op_neovex_ctx_db_delete", {
          table,
          id,
        });
      },
    },
    scheduler: {
      runAfter(delayMs, functionRef, args = {}) {
        const normalized = __neovexNormalizeFunctionReference(functionRef, "scheduler.runAfter");
        return asyncHostValue("op_neovex_ctx_scheduler_run_after", {
          delay_ms: delayMs,
          ...normalized,
          args,
        });
      },
      runAt(timestampMs, functionRef, args = {}) {
        const normalized = __neovexNormalizeFunctionReference(functionRef, "scheduler.runAt");
        return asyncHostValue("op_neovex_ctx_scheduler_run_at", {
          timestamp_ms: timestampMs,
          ...normalized,
          args,
        });
      },
      cancel(jobId) {
        return asyncHostValue("op_neovex_ctx_scheduler_cancel", {
          job_id: jobId,
        });
      },
    },
      runQuery(functionRef, args = {}) {
        return __neovexRunNamedFunction(
          syncHostValue,
          "op_neovex_ctx_run_query",
          sessionId,
          requestAuth,
          "query",
        "runQuery",
        functionRef,
        args,
      );
    },
      runMutation(functionRef, args = {}) {
        return __neovexRunNamedFunction(
          syncHostValue,
          "op_neovex_ctx_run_mutation",
          sessionId,
          requestAuth,
          "mutation",
        "runMutation",
        functionRef,
        args,
      );
    },
      runAction(functionRef, args = {}) {
        return __neovexRunNamedFunction(
          syncHostValue,
          "op_neovex_ctx_run_action",
          sessionId,
          requestAuth,
          "action",
        "runAction",
        functionRef,
        args,
      );
    },
  };
};

Object.freeze(globalThis.__neovexSyncHostValue);
Object.freeze(globalThis.__neovexAsyncHostValue);
Object.freeze(globalThis.__neovexCreateContext);
"#;

const POST_BOOTSTRAP_SOURCE: &str = "delete globalThis.Deno;";

#[op2]
#[serde]
fn op_neovex_ctx_query_start(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryStartPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxDbQueryStart, payload)
}

#[op2]
#[serde]
fn op_neovex_ctx_query_with_index(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryWithIndexPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxDbQueryWithIndex, payload)
}

#[op2]
#[serde]
fn op_neovex_ctx_query_filter(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryFilterPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxDbQueryFilter, payload)
}

#[op2]
#[serde]
fn op_neovex_ctx_query_order(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryOrderPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxDbQueryOrder, payload)
}

#[op2]
#[serde]
async fn op_neovex_ctx_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxQuery, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_paginated_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncPaginatedQueryPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxPaginatedQuery, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_mutation(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncMutationPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxMutation, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_action(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncActionPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxAction, payload).await
}

#[op2]
#[serde]
async fn op_neovex_http_route(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncHttpRoutePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::HttpRoute, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_get(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbGetPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbGet, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_insert(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbInsertPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbInsert, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_patch(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbPatchPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbPatch, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_delete(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbDeletePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbDelete, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_collect(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbQueryCollect, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_take(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTakePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbQueryTake, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_paginate(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryPaginatePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbQueryPaginate, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_first(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbQueryFirst, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_unique(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxDbQueryUnique, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_scheduler_run_after(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerRunAfterPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxSchedulerRunAfter, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_scheduler_run_at(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerRunAtPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxSchedulerRunAt, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_scheduler_cancel(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerCancelPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxSchedulerCancel, payload).await
}

#[op2]
#[serde]
fn op_neovex_ctx_runtime_enter_nested_call(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncNestedCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, HostCallOperation::CtxRuntimeEnterNestedCall, payload)
}

#[op2]
#[serde]
async fn op_neovex_ctx_run_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxRunQuery, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_run_mutation(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxRunMutation, payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_run_action(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, HostCallOperation::CtxRunAction, payload).await
}

async fn op_neovex_async_host_call<T>(
    state: Rc<RefCell<OpState>>,
    operation: HostCallOperation,
    payload: T,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox>
where
    T: Serialize + Send + 'static,
{
    struct HostCallPermitLease {
        permit: SharedInvocationPermit,
        completed: bool,
    }

    impl HostCallPermitLease {
        fn new(permit: SharedInvocationPermit) -> Self {
            permit.begin_async_host_call();
            Self {
                permit,
                completed: false,
            }
        }

        async fn complete(&mut self) -> std::result::Result<(), JsErrorBox> {
            self.completed = true;
            self.permit
                .complete_async_host_call()
                .await
                .map_err(|error| JsErrorBox::generic(error.to_string()))
        }
    }

    impl Drop for HostCallPermitLease {
        fn drop(&mut self) {
            if !self.completed {
                self.permit.drop_async_host_call();
            }
        }
    }

    let (host_state, cancel_handle, cancellation_signal, permit) = {
        let state = state.borrow();
        (
            state.borrow::<RuntimeHostState>().clone(),
            state
                .borrow::<RuntimeCancellationState>()
                .cancel_handle
                .clone(),
            state.borrow::<RuntimeCancellationState>().signal.clone(),
            state.borrow::<SharedInvocationPermit>().clone(),
        )
    };
    let mut permit_lease = HostCallPermitLease::new(permit);
    let payload_value =
        serde_json::to_value(payload).map_err(|error| JsErrorBox::generic(error.to_string()))?;
    let host_call = host_state
        .bridge
        .call_async(
            HostCallRequest {
                operation,
                payload: payload_value,
            },
            cancellation_signal.clone(),
        )
        .or_cancel(cancel_handle.clone());
    tokio::pin!(host_call);

    tokio::select! {
        result = &mut host_call => {
            let result = normalize_host_call_value(
                result
                .map_err(JsErrorBox::from)?
                .map_err(|error| JsErrorBox::generic(error.to_string()))?
            );
            permit_lease.complete().await?;
            result
        }
        _ = cancellation_signal.cancelled() => {
            cancel_handle.cancel();
            let result = normalize_host_call_value(
                host_call
                .await
                .map_err(JsErrorBox::from)?
                .map_err(|error| JsErrorBox::generic(error.to_string()))?
            );
            permit_lease.complete().await?;
            result
        }
    }
}

fn normalize_host_call_value(
    value: Value,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    match serde_json::from_value::<RuntimeHostCallEnvelope>(value.clone()) {
        Ok(envelope) => Ok(envelope),
        Err(_) => Ok(RuntimeHostCallEnvelope::Ok { value }),
    }
}

fn op_neovex_sync_host_call<T>(
    state: &mut OpState,
    operation: HostCallOperation,
    payload: T,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox>
where
    T: Serialize,
{
    let host_state = state.borrow::<RuntimeHostState>().clone();
    let payload_value =
        serde_json::to_value(payload).map_err(|error| JsErrorBox::generic(error.to_string()))?;
    let value = host_state
        .bridge
        .call(HostCallRequest {
            operation,
            payload: payload_value,
        })
        .map_err(|error| JsErrorBox::generic(error.to_string()))?;
    normalize_host_call_value(value)
}

pub(crate) fn initialize_runtime_state(runtime: &mut JsRuntime, runtime_owner: &NeovexRuntime) {
    let op_state = runtime.op_state();
    let mut state = op_state.borrow_mut();
    state.put(RuntimeHostState {
        bridge: runtime_owner.host.clone(),
    });
    let signal = HostCallCancellation::default();
    state.put(RuntimeCancellationState {
        cancel_handle: CancelHandle::new_rc(),
        signal,
    });
    state.put(SharedInvocationPermit::new(
        runtime_owner.policy.clone(),
        None,
        None,
        true,
        None,
    ));
}

pub(crate) fn install_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
    runtime
        .execute_script("<neovex-runtime:bootstrap>", BOOTSTRAP_SOURCE)
        .map_err(|error| NeovexRuntimeError::JavaScript(error.to_string()))?;
    Ok(())
}

pub(crate) fn finalize_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
    runtime
        .execute_script("<neovex-runtime:bootstrap:finalize>", POST_BOOTSTRAP_SOURCE)
        .map_err(|error| NeovexRuntimeError::JavaScript(error.to_string()))?;
    Ok(())
}

pub(crate) fn create_bootstrap_snapshot() -> Result<RuntimeStartupSnapshot> {
    #[cfg(test)]
    RUNTIME_BOOTSTRAP_SNAPSHOT_BUILDS.fetch_add(1, Ordering::Relaxed);

    let mut runtime = JsRuntimeForSnapshot::new(RuntimeOptions {
        extensions: vec![runtime_extension()],
        ..Default::default()
    });
    install_bootstrap(&mut runtime)?;
    Ok(RuntimeStartupSnapshot::new(runtime.snapshot()))
}

pub(crate) fn runtime_extension() -> deno_core::Extension {
    neovex_runtime_ext::init()
}

#[cfg(test)]
pub(crate) fn bootstrap_snapshot_build_count_for_test() -> usize {
    RUNTIME_BOOTSTRAP_SNAPSHOT_BUILDS.load(Ordering::Relaxed)
}
