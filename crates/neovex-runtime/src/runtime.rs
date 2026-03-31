use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use deno_core::op2;
use deno_core::{
    CancelFuture, CancelHandle, JsRuntime, ModuleSpecifier, OpState, PollEventLoopOptions,
    RuntimeOptions, extension, scope, serde_v8, v8,
};
use deno_error::JsErrorBox;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::RuntimeInvocationContext;
use crate::error::{NeovexRuntimeError, Result};
use crate::executor::RuntimeExecutor;
use crate::host::{HostBridge, HostCallCancellation, HostCallRequest};
use crate::limits::{RuntimeLimits, RuntimePolicy};
use crate::module_loader::SandboxedModuleLoader;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationKind {
    Query,
    PaginatedQuery,
    Mutation,
    Action,
}

impl InvocationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::PaginatedQuery => "paginated_query",
            Self::Mutation => "mutation",
            Self::Action => "action",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvocationRequest {
    pub kind: InvocationKind,
    pub function_name: String,
    #[serde(default)]
    pub args: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<InvocationAuth>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUserIdentity {
    pub token_identifier: String,
    pub subject: String,
    pub issuer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birthday: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_number_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub custom_claims: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedUserIdentityKind {
    Oidc,
    CustomJwt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedUserIdentity {
    pub kind: VerifiedUserIdentityKind,
    pub token_identifier: String,
    pub subject: String,
    pub issuer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birthday: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_number_verified: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub custom_claims: Map<String, Value>,
}

impl VerifiedUserIdentity {
    pub fn token_identifier(&self) -> &str {
        &self.token_identifier
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InvocationAuth {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<RuntimeUserIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_identity: Option<VerifiedUserIdentity>,
    #[serde(default)]
    pub throw_on_missing_identity: bool,
}

impl InvocationAuth {
    pub fn with_identities(
        identity: RuntimeUserIdentity,
        verified_identity: VerifiedUserIdentity,
        throw_on_missing_identity: bool,
    ) -> Self {
        Self {
            identity: Some(identity),
            verified_identity: Some(verified_identity),
            throw_on_missing_identity,
        }
    }

    pub fn token_identifier(&self) -> Option<&str> {
        self.verified_identity
            .as_ref()
            .map(VerifiedUserIdentity::token_identifier)
            .or_else(|| {
                self.identity
                    .as_ref()
                    .map(|identity| identity.token_identifier.as_str())
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBundle {
    entrypoint: PathBuf,
    expected_sha256: Option<String>,
}

impl RuntimeBundle {
    pub fn new(entrypoint: impl AsRef<Path>) -> Self {
        Self {
            entrypoint: entrypoint.as_ref().to_path_buf(),
            expected_sha256: None,
        }
    }

    pub fn with_expected_sha256(
        entrypoint: impl AsRef<Path>,
        expected_sha256: impl AsRef<str>,
    ) -> Result<Self> {
        Ok(Self {
            entrypoint: entrypoint.as_ref().to_path_buf(),
            expected_sha256: Some(normalize_sha256(expected_sha256.as_ref())?),
        })
    }

    pub fn entrypoint(&self) -> &Path {
        &self.entrypoint
    }

    pub fn compute_sha256_for_path(path: impl AsRef<Path>) -> Result<String> {
        let bytes = std::fs::read(path)?;
        Ok(compute_sha256_hex(&bytes))
    }

    fn module_specifier(&self) -> Result<ModuleSpecifier> {
        ModuleSpecifier::from_file_path(&self.entrypoint).map_err(|_| {
            NeovexRuntimeError::Contract(format!(
                "bundle entrypoint is not a valid file URL: {}",
                self.entrypoint.display()
            ))
        })
    }

    fn module_root(&self) -> Result<PathBuf> {
        self.entrypoint
            .parent()
            .ok_or_else(|| {
                NeovexRuntimeError::Contract(format!(
                    "bundle entrypoint does not have a parent directory: {}",
                    self.entrypoint.display()
                ))
            })?
            .canonicalize()
            .map_err(NeovexRuntimeError::from)
    }

    fn verify_integrity(&self) -> Result<()> {
        let Some(expected_sha256) = &self.expected_sha256 else {
            return Ok(());
        };
        let actual_sha256 = Self::compute_sha256_for_path(&self.entrypoint)?;
        if &actual_sha256 == expected_sha256 {
            return Ok(());
        }
        Err(NeovexRuntimeError::BundleIntegrityMismatch(format!(
            "{} (expected {}, got {})",
            self.entrypoint.display(),
            expected_sha256,
            actual_sha256
        )))
    }
}

#[derive(Clone)]
struct RuntimeHostState {
    bridge: Arc<dyn HostBridge>,
}

#[derive(Clone)]
struct RuntimeCancellationState {
    cancel_handle: Rc<CancelHandle>,
    signal: HostCallCancellation,
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
        op_neovex_host_call,
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
globalThis.__neovexRawHostCall = function(operation, payload) {
  return JSON.parse(__neovexCoreOps.op_neovex_host_call({
    operation,
    payload,
  }));
};

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

globalThis.__neovexHostValue = globalThis.__neovexSyncHostValue;

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

Object.freeze(globalThis.__neovexRawHostCall);
Object.freeze(globalThis.__neovexSyncHostValue);
Object.freeze(globalThis.__neovexHostValue);
Object.freeze(globalThis.__neovexAsyncHostValue);
Object.freeze(globalThis.__neovexCreateContext);
delete globalThis.Deno;
"#;

#[op2]
#[string]
fn op_neovex_host_call(
    state: &mut OpState,
    #[serde] request: HostCallRequest,
) -> std::result::Result<String, JsErrorBox> {
    let host_state = state.borrow::<RuntimeHostState>().clone();
    host_state
        .bridge
        .call(request)
        .and_then(|value| serde_json::to_string(&value).map_err(NeovexRuntimeError::from))
        .map_err(|error| JsErrorBox::generic(error.to_string()))
}

#[op2]
#[serde]
fn op_neovex_ctx_query_start(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryStartPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, "convex.ctx.db.query.start", payload)
}

#[op2]
#[serde]
fn op_neovex_ctx_query_with_index(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryWithIndexPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, "convex.ctx.db.query.with_index", payload)
}

#[op2]
#[serde]
fn op_neovex_ctx_query_filter(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryFilterPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, "convex.ctx.db.query.filter", payload)
}

#[op2]
#[serde]
fn op_neovex_ctx_query_order(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncQueryOrderPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, "convex.ctx.db.query.order", payload)
}

#[op2]
#[serde]
async fn op_neovex_ctx_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.query", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_paginated_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncPaginatedQueryPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.paginated_query", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_mutation(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncMutationPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.mutation", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_action(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncActionPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.action", payload).await
}

#[op2]
#[serde]
async fn op_neovex_http_route(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncHttpRoutePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.http_route", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_get(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbGetPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.db.get", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_insert(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbInsertPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.db.insert", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_patch(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbPatchPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.db.patch", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_db_delete(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncDbDeletePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.db.delete", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_collect(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.db.query.collect", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_take(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTakePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.db.query.take", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_paginate(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryPaginatePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.db.query.paginate", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_first(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.db.query.first", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_query_unique(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncQueryTerminalPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.db.query.unique", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_scheduler_run_after(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerRunAfterPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.scheduler.run_after", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_scheduler_run_at(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerRunAtPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.scheduler.run_at", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_scheduler_cancel(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncSchedulerCancelPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.scheduler.cancel", payload).await
}

#[op2]
#[serde]
fn op_neovex_ctx_runtime_enter_nested_call(
    state: &mut OpState,
    #[serde] payload: RuntimeSyncNestedCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_sync_host_call(state, "convex.ctx.runtime.enter_nested_call", payload)
}

#[op2]
#[serde]
async fn op_neovex_ctx_run_query(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.run_query", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_run_mutation(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.run_mutation", payload).await
}

#[op2]
#[serde]
async fn op_neovex_ctx_run_action(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeAsyncFunctionCallPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    op_neovex_async_host_call(state, "convex.ctx.run_action", payload).await
}

async fn op_neovex_async_host_call<T>(
    state: Rc<RefCell<OpState>>,
    operation: &'static str,
    payload: T,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox>
where
    T: Serialize + Send + 'static,
{
    let (host_state, cancel_handle, cancellation_signal) = {
        let state = state.borrow();
        (
            state.borrow::<RuntimeHostState>().clone(),
            state
                .borrow::<RuntimeCancellationState>()
                .cancel_handle
                .clone(),
            state.borrow::<RuntimeCancellationState>().signal.clone(),
        )
    };
    let payload_value =
        serde_json::to_value(payload).map_err(|error| JsErrorBox::generic(error.to_string()))?;
    let operation = operation.to_string();
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
            normalize_host_call_value(
                result
                .map_err(JsErrorBox::from)?
                .map_err(|error| JsErrorBox::generic(error.to_string()))?
            )
        }
        _ = cancellation_signal.cancelled() => {
            cancel_handle.cancel();
            normalize_host_call_value(
                host_call
                .await
                .map_err(JsErrorBox::from)?
                .map_err(|error| JsErrorBox::generic(error.to_string()))?
            )
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
    operation: &'static str,
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
            operation: operation.to_string(),
            payload: payload_value,
        })
        .map_err(|error| JsErrorBox::generic(error.to_string()))?;
    normalize_host_call_value(value)
}

#[derive(Clone)]
pub struct NeovexRuntime {
    host: Arc<dyn HostBridge>,
    policy: Arc<RuntimePolicy>,
    bypass_concurrency_limit: bool,
}

/// Legacy alias for Convex-shaped integrations.
pub type ConvexRuntime = NeovexRuntime;

impl NeovexRuntime {
    pub fn new(host: Arc<dyn HostBridge>) -> Self {
        Self::with_policy(host, Arc::new(RuntimePolicy::default()))
    }

    pub fn with_limits(host: Arc<dyn HostBridge>, limits: RuntimeLimits) -> Self {
        Self::with_policy(host, Arc::new(RuntimePolicy::new(limits)))
    }

    pub fn with_policy(host: Arc<dyn HostBridge>, policy: Arc<RuntimePolicy>) -> Self {
        Self {
            host,
            policy,
            bypass_concurrency_limit: false,
        }
    }

    pub fn with_policy_bypassing_limit(
        host: Arc<dyn HostBridge>,
        policy: Arc<RuntimePolicy>,
    ) -> Self {
        Self {
            host,
            policy,
            bypass_concurrency_limit: true,
        }
    }

    pub async fn invoke_bundle(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
    ) -> Result<Value> {
        self.invoke_bundle_with_cancellation(bundle, request, None)
            .await
    }

    pub async fn invoke_bundle_with_cancellation(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        RuntimeExecutor::new(self.policy.clone())
            .invoke_with_cancellation(
                self.clone(),
                bundle.clone(),
                request.clone(),
                RuntimeInvocationContext::top_level(request),
                cancellation,
            )
            .await
    }

    pub fn invoke_bundle_blocking(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
    ) -> Result<Value> {
        self.invoke_bundle_blocking_with_cancellation(bundle, request, None)
    }

    pub fn invoke_bundle_blocking_with_cancellation(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        RuntimeExecutor::new(self.policy.clone()).invoke_blocking_with_cancellation(
            self.clone(),
            bundle.clone(),
            request.clone(),
            RuntimeInvocationContext::top_level(request),
            cancellation,
        )
    }

    pub(crate) fn bypasses_concurrency_limit(&self) -> bool {
        self.bypass_concurrency_limit
    }

    pub(crate) async fn invoke_bundle_unmanaged(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        _context: &RuntimeInvocationContext,
        external_cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        bundle.verify_integrity()?;
        let mut runtime = self.create_runtime(bundle)?;
        let timeout = self.policy.limits().execution_timeout;
        let timeout_triggered = Arc::new(AtomicBool::new(false));
        let heap_limit_triggered = Arc::new(AtomicBool::new(false));
        let external_cancellation_triggered = Arc::new(AtomicBool::new(false));
        let cancellation_signal = {
            let op_state = runtime.op_state();
            op_state
                .borrow()
                .borrow::<RuntimeCancellationState>()
                .signal
                .clone()
        };
        let external_cancellation_watchdog = external_cancellation.clone().map(|external| {
            let (stop_tx, stop_rx) = mpsc::channel();
            let isolate_handle = runtime.v8_isolate().thread_safe_handle();
            let cancellation_signal = cancellation_signal.clone();
            let external_cancellation_triggered = external_cancellation_triggered.clone();
            let thread = std::thread::spawn(move || {
                loop {
                    if external.is_cancelled() {
                        external_cancellation_triggered.store(true, Ordering::SeqCst);
                        cancellation_signal.cancel();
                        let _ = isolate_handle.terminate_execution();
                        break;
                    }
                    match stop_rx.recv_timeout(std::time::Duration::from_millis(10)) {
                        Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
            });
            (stop_tx, thread)
        });

        {
            let heap_limit_triggered = heap_limit_triggered.clone();
            let cancellation_signal = cancellation_signal.clone();
            let isolate_handle = runtime.v8_isolate().thread_safe_handle();
            runtime.add_near_heap_limit_callback(move |current_limit, _initial_limit| {
                heap_limit_triggered.store(true, Ordering::SeqCst);
                cancellation_signal.cancel();
                let _ = isolate_handle.terminate_execution();
                current_limit.saturating_mul(2)
            });
        }

        let watchdog = if timeout.is_zero() {
            None
        } else {
            let (stop_tx, stop_rx) = mpsc::channel();
            let isolate_handle = runtime.v8_isolate().thread_safe_handle();
            let timeout_triggered = timeout_triggered.clone();
            let cancellation_signal = cancellation_signal.clone();
            let thread = std::thread::spawn(move || {
                if stop_rx.recv_timeout(timeout).is_err() {
                    timeout_triggered.store(true, Ordering::SeqCst);
                    cancellation_signal.cancel();
                    let _ = isolate_handle.terminate_execution();
                }
            });
            Some((stop_tx, thread))
        };

        let result = {
            let isolate_handle = runtime.v8_isolate().thread_safe_handle();
            let cancellation_signal = cancellation_signal.clone();
            let external_cancellation_triggered = external_cancellation_triggered.clone();
            let invoke = async {
                self.load_bundle(&mut runtime, bundle).await?;
                self.invoke_loaded_bundle(&mut runtime, request).await
            };
            tokio::pin!(invoke);
            match external_cancellation {
                Some(external_cancellation) => {
                    tokio::select! {
                        result = &mut invoke => result,
                        _ = external_cancellation.cancelled() => {
                            external_cancellation_triggered.store(true, Ordering::SeqCst);
                            cancellation_signal.cancel();
                            let _ = isolate_handle.terminate_execution();
                            invoke.await
                        }
                    }
                }
                None => invoke.await,
            }
        };

        if let Some((stop_tx, thread)) = watchdog {
            let _ = stop_tx.send(());
            let _ = thread.join();
        }
        if let Some((stop_tx, thread)) = external_cancellation_watchdog {
            let _ = stop_tx.send(());
            let _ = thread.join();
        }

        result.map_err(|error| {
            classify_runtime_error(
                error,
                &timeout_triggered,
                &heap_limit_triggered,
                &external_cancellation_triggered,
                self.policy.limits(),
            )
        })
    }

    async fn load_bundle(&self, runtime: &mut JsRuntime, bundle: &RuntimeBundle) -> Result<()> {
        let module_specifier = bundle.module_specifier()?;
        let module_id = runtime
            .load_main_es_module(&module_specifier)
            .await
            .map_err(runtime_js_error)?;
        let evaluation = runtime.mod_evaluate(module_id);
        runtime
            .run_event_loop(Default::default())
            .await
            .map_err(runtime_js_error)?;
        evaluation.await.map_err(runtime_js_error)?;
        Ok(())
    }

    async fn invoke_loaded_bundle(
        &self,
        runtime: &mut JsRuntime,
        request: &InvocationRequest,
    ) -> Result<Value> {
        let request_json = serde_json::to_string(request)?;
        let expression = format!("globalThis.__neovexInvoke({request_json})");
        let value = runtime
            .execute_script("<neovex-runtime:invoke>", expression)
            .map_err(runtime_js_error)?;
        let resolve = runtime.resolve(value);
        let value = runtime
            .with_event_loop_promise(resolve, PollEventLoopOptions::default())
            .await
            .map_err(runtime_js_error)?;
        deserialize_json_value(runtime, value)
    }

    fn create_runtime(&self, bundle: &RuntimeBundle) -> Result<JsRuntime> {
        let heap_megabyte = 1usize << 20;
        let create_params = v8::Isolate::create_params().heap_limits(
            self.policy.limits().initial_heap_mb * heap_megabyte,
            self.policy.limits().max_heap_mb * heap_megabyte,
        );
        let mut runtime = JsRuntime::new(RuntimeOptions {
            create_params: Some(create_params),
            module_loader: Some(Rc::new(SandboxedModuleLoader::new(bundle.module_root()?))),
            extensions: vec![neovex_runtime_ext::init()],
            ..Default::default()
        });
        {
            let op_state = runtime.op_state();
            let mut state = op_state.borrow_mut();
            state.put(RuntimeHostState {
                bridge: self.host.clone(),
            });
            let signal = HostCallCancellation::default();
            state.put(RuntimeCancellationState {
                cancel_handle: CancelHandle::new_rc(),
                signal,
            });
        }
        runtime
            .execute_script("<neovex-runtime:bootstrap>", BOOTSTRAP_SOURCE)
            .map_err(runtime_js_error)?;
        Ok(runtime)
    }
}

fn deserialize_json_value(runtime: &mut JsRuntime, value: v8::Global<v8::Value>) -> Result<Value> {
    scope!(scope, runtime);
    let local = v8::Local::new(scope, value);
    serde_v8::from_v8(scope, local)
        .map_err(|error| NeovexRuntimeError::JavaScript(error.to_string()))
}

fn runtime_js_error(error: impl std::fmt::Display) -> NeovexRuntimeError {
    NeovexRuntimeError::JavaScript(error.to_string())
}

fn classify_runtime_error(
    error: NeovexRuntimeError,
    timeout_triggered: &AtomicBool,
    heap_limit_triggered: &AtomicBool,
    external_cancellation_triggered: &AtomicBool,
    limits: &RuntimeLimits,
) -> NeovexRuntimeError {
    match error {
        NeovexRuntimeError::JavaScript(message)
            if heap_limit_triggered.load(Ordering::SeqCst)
                && is_execution_terminated_error(&message) =>
        {
            NeovexRuntimeError::HeapLimitExceeded(limits.max_heap_mb)
        }
        NeovexRuntimeError::JavaScript(message) if is_host_call_canceled_error(&message) => {
            NeovexRuntimeError::Cancelled
        }
        NeovexRuntimeError::JavaScript(_message)
            if external_cancellation_triggered.load(Ordering::SeqCst) =>
        {
            NeovexRuntimeError::Cancelled
        }
        NeovexRuntimeError::JavaScript(_message) if timeout_triggered.load(Ordering::SeqCst) => {
            NeovexRuntimeError::ExecutionTimeout(limits.execution_timeout)
        }
        other => other,
    }
}

fn is_execution_terminated_error(message: &str) -> bool {
    message.contains("execution terminated")
}

fn is_host_call_canceled_error(message: &str) -> bool {
    message.contains("runtime host call canceled")
}

fn normalize_sha256(value: &str) -> Result<String> {
    let normalized = value
        .split_ascii_whitespace()
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let valid = normalized.len() == 64 && normalized.bytes().all(|byte| byte.is_ascii_hexdigit());
    if valid {
        Ok(normalized)
    } else {
        Err(NeovexRuntimeError::Contract(format!(
            "runtime bundle sha256 must be a 64-character hex string, got {value:?}"
        )))
    }
}

fn compute_sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tempfile::tempdir;

    use super::*;
    use crate::host::{HostBridgeFuture, HostCallCancellation, HostCallRequest};

    #[derive(Default)]
    struct RecordingHost {
        calls: Mutex<Vec<HostCallRequest>>,
    }

    impl HostBridge for RecordingHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            self.calls
                .lock()
                .expect("recording host lock should not be poisoned")
                .push(request.clone());
            Ok(serde_json::json!({
                "operation": request.operation,
                "payload": request.payload,
            }))
        }
    }

    struct SlowEnvelopeHost {
        delay: std::time::Duration,
    }

    impl HostBridge for SlowEnvelopeHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            std::thread::sleep(self.delay);
            Ok(serde_json::json!({
                "status": "ok",
                "value": Value::Null,
            }))
        }
    }

    struct AsyncOnlyHost;

    impl HostBridge for AsyncOnlyHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(NeovexRuntimeError::Contract(
                "sync host bridge path should not be used for async ops".to_string(),
            ))
        }

        fn call_async(
            &self,
            _request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            Box::pin(async move {
                Ok(serde_json::json!({
                    "status": "ok",
                    "value": "async-host",
                }))
            })
        }
    }

    struct AsyncEchoHost;

    impl HostBridge for AsyncEchoHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(NeovexRuntimeError::Contract(
                "sync host bridge path should not be used for async ops".to_string(),
            ))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            Box::pin(async move {
                Ok(serde_json::json!({
                    "status": "ok",
                    "value": {
                        "operation": request.operation,
                        "payload": request.payload,
                    },
                }))
            })
        }
    }

    #[derive(Default)]
    struct SyncOnlyHost {
        calls: Mutex<Vec<HostCallRequest>>,
    }

    impl HostBridge for SyncOnlyHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            self.calls
                .lock()
                .expect("sync-only host lock should not be poisoned")
                .push(request.clone());
            let value = match request.operation.as_str() {
                "convex.ctx.db.query.start" => Value::String("builder-1".to_string()),
                _ => Value::Null,
            };
            Ok(serde_json::json!({
                "status": "ok",
                "value": value,
            }))
        }

        fn call_async(
            &self,
            _request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            Box::pin(async move {
                Err(NeovexRuntimeError::Contract(
                    "async host bridge path should not be used for sync ops".to_string(),
                ))
            })
        }
    }

    #[tokio::test]
    async fn runtime_loads_bundle_and_invokes_host_bridge() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function (request) {
  const host = globalThis.__neovexRawHostCall("echo", {
    kind: request.kind,
    function_name: request.function_name,
    args: request.args,
  });
  return {
    ok: true,
    host,
  };
};

export {};
"#,
        )
        .expect("bundle should write");

        let host = Arc::new(RecordingHost::default());
        let runtime = NeovexRuntime::new(host.clone());
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: serde_json::json!({ "author": "Ada" }),
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("bundle invocation should succeed");

        assert_eq!(
            result,
            serde_json::json!({
                "ok": true,
                "host": {
                    "operation": "echo",
                    "payload": {
                        "kind": "query",
                        "function_name": "messages:list",
                        "args": { "author": "Ada" },
                    }
                }
            })
        );

        let calls = host
            .calls
            .lock()
            .expect("recording host lock should not be poisoned")
            .clone();
        assert_eq!(
            calls,
            vec![HostCallRequest {
                operation: "echo".to_string(),
                payload: serde_json::json!({
                    "kind": "query",
                    "function_name": "messages:list",
                    "args": { "author": "Ada" },
                }),
            }]
        );
    }

    #[tokio::test]
    async fn runtime_requires_bundle_contract() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(&bundle_path, "export const noop = 1;").expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let error = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Action,
                    function_name: "messages:missing".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("missing global invoke contract should fail");

        assert!(
            error.to_string().contains("__neovexInvoke"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn runtime_awaits_async_bundle_handlers() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function (request) {
  const value = globalThis.__neovexRawHostCall("echo", {
    kind: request.kind,
    function_name: request.function_name,
  });
  return {
    ok: true,
    awaited: await Promise.resolve({
      operation: value.operation,
      payload: value.payload,
    }),
  };
};

export {};
"#,
        )
        .expect("bundle should write");

        let host = Arc::new(RecordingHost::default());
        let runtime = NeovexRuntime::new(host);
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("async bundle invocation should succeed");

        assert_eq!(
            result,
            serde_json::json!({
                "ok": true,
                "awaited": {
                    "operation": "echo",
                    "payload": {
                        "kind": "query",
                        "function_name": "messages:list",
                    }
                }
            })
        );
    }

    #[tokio::test]
    async fn runtime_removes_deno_global_from_bundle_execution() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
if (typeof Deno !== "undefined") {
  throw new Error("Deno should not be exposed to runtime bundles");
}

globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("bundle should execute without exposing Deno");

        assert_eq!(result, serde_json::json!({ "ok": true }));
    }

    #[tokio::test]
    async fn runtime_times_out_infinite_loops() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  while (true) {}
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::with_limits(
            Arc::new(RecordingHost::default()),
            RuntimeLimits {
                execution_timeout: std::time::Duration::from_millis(50),
                ..RuntimeLimits::default()
            },
        );
        let error = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("infinite loop should time out");

        match error {
            NeovexRuntimeError::ExecutionTimeout(timeout) => {
                assert_eq!(timeout, std::time::Duration::from_millis(50));
            }
            other => panic!("unexpected timeout error: {other}"),
        }
        assert_eq!(runtime.policy.metrics_snapshot().timed_out_invocations, 1);
    }

    #[tokio::test]
    async fn runtime_external_cancellation_stops_infinite_loops() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  while (true) {}
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::with_limits(
            Arc::new(RecordingHost::default()),
            RuntimeLimits {
                execution_timeout: std::time::Duration::from_secs(5),
                ..RuntimeLimits::default()
            },
        );
        let cancellation = HostCallCancellation::default();
        let cancellation_clone = cancellation.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            cancellation_clone.cancel();
        });

        let error = runtime
            .invoke_bundle_with_cancellation(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
                Some(cancellation),
            )
            .await
            .expect_err("external cancellation should stop the runtime invocation");

        assert!(matches!(error, NeovexRuntimeError::Cancelled));
    }

    #[tokio::test]
    async fn runtime_times_out_slow_async_host_ops() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  await ctx.db.get("messages", "doc-1");
  return { ok: true };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::with_limits(
            Arc::new(SlowEnvelopeHost {
                delay: std::time::Duration::from_secs(1),
            }),
            RuntimeLimits {
                execution_timeout: std::time::Duration::from_millis(50),
                ..RuntimeLimits::default()
            },
        );
        let error = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("slow async host op should time out");

        match error {
            NeovexRuntimeError::ExecutionTimeout(timeout) => {
                assert_eq!(timeout, std::time::Duration::from_millis(50));
            }
            other => panic!("unexpected timeout error: {other}"),
        }
    }

    #[tokio::test]
    async fn runtime_async_ops_use_async_host_bridge_path() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const value = await ctx.db.get("messages", "doc-1");
  return { value };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(AsyncOnlyHost));
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("async host bridge should satisfy async op");

        assert_eq!(result, serde_json::json!({ "value": "async-host" }));
    }

    #[tokio::test]
    async fn runtime_exposes_verified_identity_extension_separately_from_convex_identity() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const request = arguments[0];
  const ctx = globalThis.__neovexCreateContext({ request });
  return {
    user: await ctx.auth.getUserIdentity(),
    verified: await ctx.auth.getVerifiedIdentity(),
  };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "auth:whoami".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: Some(InvocationAuth::with_identities(
                        RuntimeUserIdentity {
                            token_identifier: "https://issuer.example.com|user-123".to_string(),
                            subject: "user-123".to_string(),
                            issuer: "https://issuer.example.com".to_string(),
                            name: None,
                            given_name: None,
                            family_name: None,
                            nickname: None,
                            preferred_username: None,
                            profile_url: None,
                            picture_url: None,
                            email: None,
                            email_verified: None,
                            gender: None,
                            birthday: None,
                            timezone: None,
                            language: None,
                            phone_number: None,
                            phone_number_verified: None,
                            address: None,
                            updated_at: None,
                            custom_claims: serde_json::from_value(serde_json::json!({
                                "email": "ada@example.com",
                                "given_name": "Ada",
                                "updated_at": 1710000000,
                                "address.formatted": "123 Analytical Engine Way",
                                "role": "admin"
                            }))
                            .expect("custom jwt compat claims should parse"),
                        },
                        VerifiedUserIdentity {
                            kind: VerifiedUserIdentityKind::CustomJwt,
                            token_identifier: "https://issuer.example.com|user-123".to_string(),
                            subject: "user-123".to_string(),
                            issuer: "https://issuer.example.com".to_string(),
                            name: Some("Ada Lovelace".to_string()),
                            given_name: Some("Ada".to_string()),
                            family_name: None,
                            nickname: None,
                            preferred_username: None,
                            profile_url: None,
                            picture_url: None,
                            email: Some("ada@example.com".to_string()),
                            email_verified: None,
                            gender: None,
                            birthday: None,
                            timezone: None,
                            language: None,
                            phone_number: None,
                            phone_number_verified: None,
                            address: Some("123 Analytical Engine Way".to_string()),
                            updated_at: Some("1710000000".to_string()),
                            custom_claims: serde_json::from_value(serde_json::json!({
                                "role": "admin"
                            }))
                            .expect("verified custom claims should parse"),
                        },
                        false,
                    )),
                },
            )
            .await
            .expect("runtime should expose both auth views");

        assert_eq!(
            result,
            serde_json::json!({
                "user": {
                    "tokenIdentifier": "https://issuer.example.com|user-123",
                    "subject": "user-123",
                    "issuer": "https://issuer.example.com",
                    "email": "ada@example.com",
                    "given_name": "Ada",
                    "updated_at": 1710000000,
                    "address.formatted": "123 Analytical Engine Way",
                    "role": "admin"
                },
                "verified": {
                    "kind": "custom_jwt",
                    "tokenIdentifier": "https://issuer.example.com|user-123",
                    "subject": "user-123",
                    "issuer": "https://issuer.example.com",
                    "name": "Ada Lovelace",
                    "givenName": "Ada",
                    "email": "ada@example.com",
                    "address": "123 Analytical Engine Way",
                    "updatedAt": "1710000000",
                    "role": "admin"
                }
            })
        );
    }

    #[tokio::test]
    async fn runtime_query_builder_setup_uses_sync_host_bridge_path() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const builder = ctx
    .db
    .query("messages")
    .withIndex("by_author", (q) => q.eq(q.field("author"), "Ada"))
    .filter((q) => q.eq(q.field("channel"), "general"))
    .order("desc");
  return { builderId: builder.__builderId };
};

export {};
"#,
        )
        .expect("bundle should write");

        let host = Arc::new(SyncOnlyHost::default());
        let runtime = NeovexRuntime::new(host.clone());
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("sync host bridge should satisfy query builder setup");

        assert_eq!(result, serde_json::json!({ "builderId": "builder-1" }));
        let calls = host
            .calls
            .lock()
            .expect("sync-only host lock should not be poisoned")
            .clone();
        assert_eq!(
            calls
                .into_iter()
                .map(|call| call.operation)
                .collect::<Vec<_>>(),
            vec![
                "convex.ctx.db.query.start".to_string(),
                "convex.ctx.db.query.with_index".to_string(),
                "convex.ctx.db.query.filter".to_string(),
                "convex.ctx.db.query.order".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn runtime_async_write_and_scheduler_ops_use_async_host_bridge_path() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const insert = await ctx.db.insert("messages", { body: "hello" });
  const patch = await ctx.db.patch("messages", "doc-1", { body: "updated" });
  const deletion = await ctx.db.delete("messages", "doc-1");
  const runAfter = await ctx.scheduler.runAfter(
    100,
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "scheduled" },
  );
  const runAt = await ctx.scheduler.runAt(
    500,
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "scheduled-at" },
  );
  const cancel = await ctx.scheduler.cancel("job-1");
  return { insert, patch, deletion, runAfter, runAt, cancel };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(AsyncEchoHost));
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Mutation,
                    function_name: "messages:write".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("async host bridge should satisfy write and scheduler ops");

        assert_eq!(
            result,
            serde_json::json!({
                "insert": {
                    "operation": "convex.ctx.db.insert",
                    "payload": {
                        "table": "messages",
                        "fields": { "body": "hello" },
                        "session_id": "session-1",
                    }
                },
                "patch": {
                    "operation": "convex.ctx.db.patch",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "patch": { "body": "updated" },
                        "session_id": "session-1",
                    }
                },
                "deletion": {
                    "operation": "convex.ctx.db.delete",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "session-1",
                    }
                },
                "runAfter": {
                    "operation": "convex.ctx.scheduler.run_after",
                    "payload": {
                        "delay_ms": 100,
                        "name": "messages:storeInternal",
                        "visibility": "internal",
                        "args": { "body": "scheduled" },
                        "session_id": "session-1",
                    }
                },
                "runAt": {
                    "operation": "convex.ctx.scheduler.run_at",
                    "payload": {
                        "timestamp_ms": 500,
                        "name": "messages:storeInternal",
                        "visibility": "internal",
                        "args": { "body": "scheduled-at" },
                        "session_id": "session-1",
                    }
                },
                "cancel": {
                    "operation": "convex.ctx.scheduler.cancel",
                    "payload": {
                        "job_id": "job-1",
                        "session_id": "session-1",
                    }
                }
            })
        );
    }

    #[tokio::test]
    async fn runtime_same_isolate_nested_entry_uses_sync_host_bridge_path() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvokeNamedLocal = async function () {
  return "local-ok";
};

globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  return await ctx.runQuery(
    { name: "messages:list", visibility: "public" },
    { author: "Ada" },
  );
};

export {};
"#,
        )
        .expect("bundle should write");

        let host = Arc::new(SyncOnlyHost::default());
        let runtime = NeovexRuntime::new(host.clone());
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:outer".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("same-isolate nested entry should succeed");

        assert_eq!(result, serde_json::json!("local-ok"));
        let calls = host
            .calls
            .lock()
            .expect("sync-only host lock should not be poisoned")
            .clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].operation, "convex.ctx.runtime.enter_nested_call");
        assert_eq!(
            calls[0].payload,
            serde_json::json!({
                "name": "messages:list",
                "visibility": "public",
                "session_id": "session-1",
            })
        );
    }

    #[tokio::test]
    async fn runtime_async_ctx_run_ops_use_async_host_bridge_path() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const query = await ctx.runQuery(
    { name: "messages:list", visibility: "public" },
    { author: "Ada" },
  );
  const mutation = await ctx.runMutation(
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "hello" },
  );
  const action = await ctx.runAction(
    { name: "messages:sendViaAction", visibility: "public" },
    { body: "wave" },
  );
  return { query, mutation, action };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(AsyncEchoHost));
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Action,
                    function_name: "messages:outer".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("async host bridge should satisfy ctx.run* fallback ops");

        assert_eq!(
            result,
            serde_json::json!({
                "query": {
                    "operation": "convex.ctx.run_query",
                    "payload": {
                        "name": "messages:list",
                        "visibility": "public",
                        "args": { "author": "Ada" },
                        "session_id": "session-1",
                    }
                },
                "mutation": {
                    "operation": "convex.ctx.run_mutation",
                    "payload": {
                        "name": "messages:storeInternal",
                        "visibility": "internal",
                        "args": { "body": "hello" },
                        "session_id": "session-1",
                    }
                },
                "action": {
                    "operation": "convex.ctx.run_action",
                    "payload": {
                        "name": "messages:sendViaAction",
                        "visibility": "public",
                        "args": { "body": "wave" },
                        "session_id": "session-1",
                    }
                }
            })
        );
    }

    #[tokio::test]
    async fn runtime_reports_heap_limit_exceeded() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  let value = "";
  while (true) {
    value += "hello world";
  }
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::with_limits(
            Arc::new(RecordingHost::default()),
            RuntimeLimits {
                max_heap_mb: 8,
                initial_heap_mb: 4,
                execution_timeout: std::time::Duration::from_secs(2),
                max_concurrent_isolates: 1,
                max_nested_runtime_invocations: RuntimeLimits::default()
                    .max_nested_runtime_invocations,
            },
        );
        let error = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("heap growth should trip the runtime heap limit");

        match error {
            NeovexRuntimeError::HeapLimitExceeded(limit) => assert_eq!(limit, 8),
            other => panic!("unexpected heap-limit error: {other}"),
        }
    }

    #[tokio::test]
    async fn runtime_rejects_module_imports_outside_bundle_root() {
        let tempdir = tempdir().expect("tempdir should build");
        let outside_path = tempdir.path().join("outside.mjs");
        let bundle_dir = tempdir.path().join("bundle");
        std::fs::create_dir_all(&bundle_dir).expect("bundle dir should exist");
        let bundle_path = bundle_dir.join("bundle.mjs");

        std::fs::write(&outside_path, "export const secret = 'outside';")
            .expect("outside module should write");
        std::fs::write(
            &bundle_path,
            r#"
import "../outside.mjs";

globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let error = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("outside import should be rejected");

        assert!(
            error.to_string().contains("outside the bundle root"),
            "unexpected loader sandbox error: {error}"
        );
    }

    #[tokio::test]
    async fn runtime_rejects_bundle_integrity_mismatch() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
        )
        .expect("bundle should write");
        let expected_sha256 =
            RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  return { ok: false };
};

export {};
"#,
        )
        .expect("tampered bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let bundle = RuntimeBundle::with_expected_sha256(&bundle_path, expected_sha256)
            .expect("bundle integrity metadata should build");
        let error = runtime
            .invoke_bundle(
                &bundle,
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("tampered bundle should fail integrity verification");

        match error {
            NeovexRuntimeError::BundleIntegrityMismatch(message) => {
                assert!(message.contains("bundle.mjs"));
            }
            other => panic!("unexpected integrity error: {other}"),
        }
    }
}
