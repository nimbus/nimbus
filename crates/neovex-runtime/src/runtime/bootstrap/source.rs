use crate::backends::v8::embedder::JsRuntime;
use crate::error::{NeovexRuntimeError, Result};

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

function __neovexCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, sessionId) {
  return Object.freeze({
    __builderId: builderId,
    withIndex(indexName, builderFn) {
      syncHostValue("op_neovex_ctx_query_with_index", {
        builder_id: builderId,
        index_name: indexName,
        filters: __neovexCollectConstraintFilters(builderFn, "withIndex"),
      });
      return __neovexCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, sessionId);
    },
    filter(builderFn) {
      syncHostValue("op_neovex_ctx_query_filter", {
        builder_id: builderId,
        filters: __neovexCollectConstraintFilters(builderFn, "filter"),
      });
      return __neovexCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, sessionId);
    },
    order(direction) {
      syncHostValue("op_neovex_ctx_query_order", {
        builder_id: builderId,
        direction,
      });
      return __neovexCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, sessionId);
    },
    collect() {
      return asyncHostValue("op_neovex_ctx_query_collect", {
        builder_id: builderId,
      });
    },
    take(limit) {
      return asyncHostValue("op_neovex_ctx_query_take", {
        builder_id: builderId,
        limit,
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
      const page = await asyncHostValue("op_neovex_ctx_query_paginate", {
        builder_id: builderId,
        page_size: paginationOpts.numItems,
        cursor,
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
      return asyncHostValue("op_neovex_ctx_query_first", {
        builder_id: builderId,
      });
    },
    unique() {
      return asyncHostValue("op_neovex_ctx_query_unique", {
        builder_id: builderId,
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
let __neovexInvocationGeneration = 0;

function __neovexCloneServiceEndpoint(endpoint) {
  if (endpoint === null || typeof endpoint !== "object") {
    return null;
  }
  if (typeof endpoint.host !== "string" || endpoint.host.length === 0) {
    return null;
  }
  if (!Number.isInteger(endpoint.port)) {
    return null;
  }
  const protocol =
    typeof endpoint.protocol === "string" && endpoint.protocol.length > 0
      ? endpoint.protocol
      : "tcp";
  return Object.freeze({
    host: endpoint.host,
    port: endpoint.port,
    protocol,
  });
}

function __neovexCloneServiceBinding(binding) {
  if (binding === null || typeof binding !== "object") {
    return null;
  }
  if (typeof binding.host !== "string" || binding.host.length === 0) {
    return null;
  }
  if (!Number.isInteger(binding.port)) {
    return null;
  }
  const endpoints = Object.create(null);
  if (binding.endpoints !== null && typeof binding.endpoints === "object") {
    for (const [endpointName, endpoint] of Object.entries(binding.endpoints)) {
      const clonedEndpoint = __neovexCloneServiceEndpoint(endpoint);
      if (clonedEndpoint !== null) {
        endpoints[endpointName] = clonedEndpoint;
      }
    }
  }
  const protocol =
    typeof binding.protocol === "string" && binding.protocol.length > 0
      ? binding.protocol
      : "tcp";
  return Object.freeze({
    host: binding.host,
    port: binding.port,
    protocol,
    endpoints: Object.freeze(endpoints),
  });
}

function __neovexCreateServiceBindings(services) {
  const clonedServices = Object.create(null);
  if (services === null || typeof services !== "object") {
    return clonedServices;
  }
  for (const [serviceName, binding] of Object.entries(services)) {
    const clonedBinding = __neovexCloneServiceBinding(binding);
    if (clonedBinding !== null) {
      clonedServices[serviceName] = clonedBinding;
    }
  }
  return clonedServices;
}

function __neovexCreateServiceRegistry(guardStale, asyncHostValue, services) {
  const cache = __neovexCreateServiceBindings(services);
  const hasOwn = (property) =>
    Object.prototype.hasOwnProperty.call(cache, property);
  const target = Object.create(null);

  Object.defineProperty(target, "get", {
    enumerable: false,
    configurable: false,
    writable: false,
    value: async (serviceName) => {
      guardStale();
      if (typeof serviceName !== "string" || serviceName.length === 0) {
        return undefined;
      }
      if (hasOwn(serviceName)) {
        return cache[serviceName];
      }
      const lookedUpBinding = __neovexCloneServiceBinding(
        await asyncHostValue("op_neovex_ctx_service_lookup", {
          service_name: serviceName,
        }),
      );
      if (lookedUpBinding !== null) {
        cache[serviceName] = lookedUpBinding;
        return lookedUpBinding;
      }
      return undefined;
    },
  });

  return new Proxy(target, {
    get(target, property) {
      guardStale();
      if (property === "get") {
        return target.get;
      }
      if (typeof property !== "string") {
        return undefined;
      }
      if (hasOwn(property)) {
        return cache[property];
      }
      return undefined;
    },
    has(target, property) {
      guardStale();
      if (property === "get") {
        return true;
      }
      if (typeof property !== "string") {
        return false;
      }
      return hasOwn(property);
    },
    ownKeys() {
      guardStale();
      return [...Reflect.ownKeys(target), ...Reflect.ownKeys(cache)];
    },
    getOwnPropertyDescriptor(target, property) {
      guardStale();
      if (property === "get") {
        return Reflect.getOwnPropertyDescriptor(target, property);
      }
      if (typeof property !== "string" || !hasOwn(property)) {
        return undefined;
      }
      return {
        value: cache[property],
        enumerable: true,
        configurable: true,
        writable: false,
      };
    },
    set() {
      return false;
    },
    defineProperty() {
      return false;
    },
    deleteProperty() {
      return false;
    },
    getPrototypeOf() {
      return null;
    },
    setPrototypeOf() {
      return false;
    },
    isExtensible() {
      return true;
    },
    preventExtensions() {
      return false;
    },
  });
}

globalThis.__neovexCreateContext = function(options = {}) {
  const myGeneration = __neovexInvocationGeneration;

  const guardStale = () => {
    if (__neovexInvocationGeneration !== myGeneration) {
      throw new Error(
        "This ctx object is from a previous invocation and cannot be reused"
      );
    }
  };
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
  const services =
    options.request !== null &&
    typeof options.request === "object" &&
    options.request.services !== null &&
    typeof options.request.services === "object"
      ? options.request.services
      : null;

  const syncHostValue = (opName, payload) => {
    guardStale();
    return globalThis.__neovexSyncHostValue(opName, {
      session_id: sessionId,
      ...(payload ?? {}),
    });
  };

  const asyncHostValue = (opName, payload) => {
    guardStale();
    return globalThis.__neovexAsyncHostValue(opName, {
      session_id: sessionId,
      ...(payload ?? {}),
    });
  };

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
        guardStale();
        return cloneAuthIdentityOrThrow(authIdentity);
      },
      async getVerifiedIdentity() {
        guardStale();
        return cloneAuthIdentityOrThrow(verifiedAuthIdentity);
      },
    }),
    services: __neovexCreateServiceRegistry(guardStale, asyncHostValue, services),
    db: {
      async get(tableOrId, maybeId) {
        guardStale();
        if (maybeId === undefined) {
          if (
            tableOrId &&
            typeof tableOrId === "object" &&
            typeof tableOrId.table === "string" &&
            typeof tableOrId.id === "string"
          ) {
            return globalThis.__neovexAsyncHostValue("op_neovex_document_get", {
              table: tableOrId.table,
              id: tableOrId.id,
              session_id: sessionId,
            });
          }
          throw new Error(
            "Neovex runtime ctx.db.get currently requires table and id at runtime",
          );
        }
        return globalThis.__neovexAsyncHostValue("op_neovex_document_get", {
          table: tableOrId,
          id: maybeId,
          session_id: sessionId,
        });
      },
      query(table) {
        const builderId = syncHostValue("op_neovex_ctx_query_start", { table });
        return __neovexCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, sessionId);
      },
      insert(table, fields) {
        return asyncHostValue("op_neovex_document_insert", {
          table,
          fields,
        });
      },
      patch(table, id, patch) {
        return asyncHostValue("op_neovex_document_patch", {
          table,
          id,
          patch,
        });
      },
      delete(table, id) {
        return asyncHostValue("op_neovex_document_delete", {
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
        guardStale();
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
        guardStale();
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
        guardStale();
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

const __neovexRuntimeEnvOverlaySymbol = Symbol.for("neovex.runtimeEnvOverlay");
const __neovexRuntimeEnvDeletedMarker = Symbol.for("neovex.runtimeEnvDeleted");
if (globalThis[__neovexRuntimeEnvOverlaySymbol] === undefined) {
  Object.defineProperty(globalThis, __neovexRuntimeEnvOverlaySymbol, {
    value: Object.create(null),
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

function __neovexRuntimeEnvOverlay() {
  return globalThis[__neovexRuntimeEnvOverlaySymbol];
}

function __neovexCreateProcessEnvProxy() {
  const snapshot = __neovexCoreOps.op_neovex_runtime_env_snapshot();
  const target = Object.assign(Object.create(null), snapshot);
  return new Proxy(target, {
    get(currentTarget, property) {
      if (typeof property !== "string") {
        return Reflect.get(currentTarget, property);
      }
      const overlay = __neovexRuntimeEnvOverlay();
      if (Object.prototype.hasOwnProperty.call(overlay, property)) {
        const value = overlay[property];
        return value === __neovexRuntimeEnvDeletedMarker ? undefined : value;
      }
      const result = __neovexCoreOps.op_neovex_runtime_env_get(property);
      if (!result || typeof result !== "object") {
        return undefined;
      }
      if (result.status === "allowed") {
        currentTarget[property] = result.value;
        return result.value;
      }
      if (result.status === "missing" || result.status === "denied") {
        delete currentTarget[property];
        return undefined;
      }
      throw new Error(result.message ?? `runtime env capability denied for ${property}`);
    },
    has(currentTarget, property) {
      if (typeof property !== "string") {
        return Reflect.has(currentTarget, property);
      }
      const overlay = __neovexRuntimeEnvOverlay();
      if (Object.prototype.hasOwnProperty.call(overlay, property)) {
        return overlay[property] !== __neovexRuntimeEnvDeletedMarker;
      }
      const result = __neovexCoreOps.op_neovex_runtime_env_get(property);
      return result?.status === "allowed";
    },
    ownKeys(currentTarget) {
      const keys = new Set(Reflect.ownKeys(currentTarget));
      for (const property of Reflect.ownKeys(__neovexRuntimeEnvOverlay())) {
        if (
          typeof property === "string" &&
          __neovexRuntimeEnvOverlay()[property] === __neovexRuntimeEnvDeletedMarker
        ) {
          keys.delete(property);
          continue;
        }
        keys.add(property);
      }
      return [...keys];
    },
    getOwnPropertyDescriptor(currentTarget, property) {
      if (typeof property !== "string") {
        return Reflect.getOwnPropertyDescriptor(currentTarget, property);
      }
      const overlay = __neovexRuntimeEnvOverlay();
      if (Object.prototype.hasOwnProperty.call(overlay, property)) {
        if (overlay[property] === __neovexRuntimeEnvDeletedMarker) {
          return undefined;
        }
        return {
          configurable: true,
          enumerable: true,
          writable: true,
          value: overlay[property],
        };
      }
      if (!Object.prototype.hasOwnProperty.call(currentTarget, property)) {
        return undefined;
      }
      return {
        configurable: true,
        enumerable: true,
        writable: true,
        value: currentTarget[property],
      };
    },
    set(_currentTarget, property, value) {
      if (typeof property === "symbol" || typeof value === "symbol") {
        throw new TypeError("Cannot convert a Symbol value to a string");
      }
      const stringValue = String(value);
      const overlay = __neovexRuntimeEnvOverlay();
      overlay[property] = stringValue;
      target[property] = stringValue;
      return true;
    },
    deleteProperty(currentTarget, property) {
      if (typeof property === "symbol") {
        return true;
      }
      const overlay = __neovexRuntimeEnvOverlay();
      overlay[property] = __neovexRuntimeEnvDeletedMarker;
      delete currentTarget[property];
      return true;
    },
  });
}

function __neovexInstallRuntimeContractGlobals(contract) {
  if (!contract || typeof contract !== "object") {
    return;
  }
  const compatibilityTarget = contract.compatibility_target;
  const nodeMajorMatch =
    typeof compatibilityTarget === "string"
      ? /^node(\d+)$/.exec(compatibilityTarget)
      : null;
  if (nodeMajorMatch) {
    const nodeMajor = nodeMajorMatch[1];
    if (typeof globalThis.global === "undefined") {
      globalThis.global = globalThis;
    }
    const cwd = typeof contract.paths?.cwd === "string" ? contract.paths.cwd : "/";
    const env = __neovexCreateProcessEnvProxy();
    const processValue = globalThis.process ?? {};
    const existingVersions =
      processValue.versions && typeof processValue.versions === "object"
        ? processValue.versions
        : {};
    const versions = Object.freeze({
      ...existingVersions,
      node: `${nodeMajor}.0.0-neovex`,
    });
    Object.defineProperty(processValue, "cwd", {
      value() {
        return cwd;
      },
      configurable: true,
      enumerable: false,
      writable: false,
    });
    Object.defineProperty(processValue, "env", {
      value: env,
      configurable: true,
      enumerable: true,
      writable: false,
    });
    Object.defineProperty(processValue, "version", {
      value: `v${nodeMajor}.0.0-neovex`,
      configurable: true,
      enumerable: true,
      writable: false,
    });
    Object.defineProperty(processValue, "versions", {
      value: versions,
      configurable: true,
      enumerable: true,
      writable: false,
    });
    Object.defineProperty(globalThis, "process", {
      value: processValue,
      configurable: true,
      enumerable: false,
      writable: false,
    });
    return;
  }
  delete globalThis.Buffer;
  delete globalThis.global;
  delete globalThis.process;
}

Object.freeze(__neovexInstallRuntimeContractGlobals);
"#;

// Keep Deno cleanup out of BOOTSTRAP_SOURCE. That source is executed during
// startup-snapshot creation, and moving `delete globalThis.Deno` into it has
// already regressed snapshot-backed Locker runtime startup in the repaired
// deno_core fork. The cleanup must remain a separate post-bootstrap step until
// the fork exposes an explicit snapshot-safe alternative. Node22 now binds its
// internal substrate against `__bootstrap.ext_node_denoGlobals`, so ordinary
// bundles should not observe the public `globalThis.Deno` contract after
// finalize_bootstrap() completes.
const POST_BOOTSTRAP_SOURCE: &str = r#"
const __neovexRuntimeContract =
  __neovexCoreOps.op_neovex_runtime_contract();
delete globalThis.Deno;
delete globalThis.__bootstrap;
delete globalThis.bootstrap;
__neovexInstallRuntimeContractGlobals(__neovexRuntimeContract);
"#;

const RESET_BOOTSTRAP_INVOCATION_STATE_SOURCE: &str =
    "__neovexNextSessionId = 1; __neovexInvocationGeneration++;";

pub(crate) fn install_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
    runtime
        .execute_script("<neovex-runtime:bootstrap>", BOOTSTRAP_SOURCE)
        .map_err(|error| NeovexRuntimeError::JavaScript(error.to_string()))?;
    Ok(())
}

pub(crate) fn finalize_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
    // This stays as an intentional second step instead of being folded into
    // install_bootstrap(), because the snapshot path also executes
    // BOOTSTRAP_SOURCE during snapshot creation.
    runtime
        .execute_script("<neovex-runtime:bootstrap:finalize>", POST_BOOTSTRAP_SOURCE)
        .map_err(|error| NeovexRuntimeError::JavaScript(error.to_string()))?;
    Ok(())
}

pub(crate) fn reset_bootstrap_invocation_state(runtime: &mut JsRuntime) -> Result<()> {
    runtime
        .execute_script(
            "<neovex-runtime:bootstrap:reset>",
            RESET_BOOTSTRAP_INVOCATION_STATE_SOURCE,
        )
        .map_err(|error| NeovexRuntimeError::JavaScript(error.to_string()))?;
    Ok(())
}
