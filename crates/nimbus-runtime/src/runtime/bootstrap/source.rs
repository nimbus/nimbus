use crate::backends::v8::embedder::JsRuntime;
use crate::error::{NimbusRuntimeError, Result};

const DENO_HOST_CALL_TRANSPORT_SOURCE: &str = r#"
const __nimbusCoreOps = Deno.core.ops;
// Capture Deno.core.runImmediates before POST_BOOTSTRAP_SOURCE deletes
// globalThis.Deno. The spawn-emulation postlude (render.rs) uses this to
// drain async_hooks' deferred destroy queue: a GC'd AsyncResource enqueues an
// unref'd, hook-suppressed "destroy drain" immediate via a FinalizationRegistry,
// and runImmediates() runs drainDestroyAsyncIds() at the top of its body plus
// that suppressed immediate. Because the harness synthesizes the process "exit"
// / mustCall check synchronously, the deferred destroy hooks must be flushed
// here before that check fires. runImmediates() creates no new hooked async
// resource (the drain immediate's before/after/destroy are suppressed), so this
// is a no-op when nothing is pending and never pollutes strict hook accounting.
Object.defineProperty(globalThis, "__nimbusDrainImmediates", {
  value: typeof Deno.core.runImmediates === "function"
    ? Deno.core.runImmediates
    : null,
  configurable: true,
  enumerable: false,
  writable: true,
});
globalThis.__nimbusSyncHostValue = function(opName, payload) {
  const operation = __nimbusCoreOps[opName];
  if (typeof operation !== "function") {
    throw new Error(`Nimbus runtime sync host op not found: ${opName}`);
  }
  const response = operation(payload);
  if (!response || response.status !== "ok") {
    const error = new Error(
      `Nimbus runtime sync host call failed for ${opName}: ${__nimbusFormatHostError(response?.error)}`,
    );
    error.nimbusHostError = response?.error ?? null;
    throw error;
  }
  return response.value;
};

function __nimbusFormatHostError(error) {
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

globalThis.__nimbusAsyncHostValue = async function(opName, payload) {
  const operation = __nimbusCoreOps[opName];
  if (typeof operation !== "function") {
    throw new Error(`Nimbus runtime async host op not found: ${opName}`);
  }
  const response = await operation(payload);
  if (!response || response.status !== "ok") {
    const error = new Error(
      `Nimbus runtime async host call failed for ${opName}: ${__nimbusFormatHostError(response?.error)}`,
    );
    error.nimbusHostError = response?.error ?? null;
    throw error;
  }
  return response.value;
};
"#;

const NIMBUS_CONTEXT_CONTRACT_SOURCE: &str = r#"
function __nimbusNormalizeFieldName(field) {
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

function __nimbusCreateConstraintBuilder() {
  const filters = [];
  const builder = {
    field(name) {
      return { __fieldName: __nimbusNormalizeFieldName(name) };
    },
    eq(field, value) {
      filters.push({ field: __nimbusNormalizeFieldName(field), op: "eq", value });
      return builder;
    },
    neq(field, value) {
      filters.push({ field: __nimbusNormalizeFieldName(field), op: "neq", value });
      return builder;
    },
    gt(field, value) {
      filters.push({ field: __nimbusNormalizeFieldName(field), op: "gt", value });
      return builder;
    },
    gte(field, value) {
      filters.push({ field: __nimbusNormalizeFieldName(field), op: "gte", value });
      return builder;
    },
    lt(field, value) {
      filters.push({ field: __nimbusNormalizeFieldName(field), op: "lt", value });
      return builder;
    },
    lte(field, value) {
      filters.push({ field: __nimbusNormalizeFieldName(field), op: "lte", value });
      return builder;
    },
  };
  return Object.assign(builder, { __filters: filters });
}

function __nimbusCollectConstraintFilters(builderFn, label) {
  const builder = __nimbusCreateConstraintBuilder();
  const result = builderFn ? builderFn(builder) : builder;
  if (result !== undefined && result !== builder && result?.__filters !== builder.__filters) {
    throw new Error(`ctx.db.${label}(...) must return the provided builder`);
  }
  return [...builder.__filters];
}

function __nimbusCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, sessionId) {
  return Object.freeze({
    __builderId: builderId,
    withIndex(indexName, builderFn) {
      syncHostValue("op_nimbus_ctx_query_with_index", {
        builder_id: builderId,
        index_name: indexName,
        filters: __nimbusCollectConstraintFilters(builderFn, "withIndex"),
      });
      return __nimbusCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, sessionId);
    },
    filter(builderFn) {
      syncHostValue("op_nimbus_ctx_query_filter", {
        builder_id: builderId,
        filters: __nimbusCollectConstraintFilters(builderFn, "filter"),
      });
      return __nimbusCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, sessionId);
    },
    order(direction) {
      syncHostValue("op_nimbus_ctx_query_order", {
        builder_id: builderId,
        direction,
      });
      return __nimbusCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, sessionId);
    },
    collect() {
      return asyncHostValue("op_nimbus_ctx_query_collect", {
        builder_id: builderId,
      });
    },
    take(limit) {
      return asyncHostValue("op_nimbus_ctx_query_take", {
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
      const page = await asyncHostValue("op_nimbus_ctx_query_paginate", {
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
      return asyncHostValue("op_nimbus_ctx_query_first", {
        builder_id: builderId,
      });
    },
    unique() {
      return asyncHostValue("op_nimbus_ctx_query_unique", {
        builder_id: builderId,
      });
    },
  });
}

function __nimbusNormalizeFunctionReference(functionRef, label) {
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

async function __nimbusRunNamedFunction(
  syncHostValue,
  asyncOpName,
  sessionId,
  authContext,
  kind,
  label,
  functionRef,
  args = {},
) {
  const normalized = __nimbusNormalizeFunctionReference(functionRef, label);
  const localInvoker = globalThis.__nimbusInvokeNamedLocal;
  const nestedAuthContext = authContext
    ? {
        ...authContext,
        throw_on_missing_identity: false,
      }
    : null;
  if (typeof localInvoker === "function") {
    syncHostValue("op_nimbus_ctx_runtime_enter_nested_call", {
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
  return globalThis.__nimbusAsyncHostValue(asyncOpName, {
    ...normalized,
    args,
    session_id: sessionId,
    ...(nestedAuthContext ? { auth: nestedAuthContext } : {}),
  });
}

let __nimbusNextSessionId = 1;
let __nimbusInvocationGeneration = 0;

function __nimbusCloneServiceEndpoint(endpoint) {
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

function __nimbusCloneServiceBinding(binding) {
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
      const clonedEndpoint = __nimbusCloneServiceEndpoint(endpoint);
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

function __nimbusCreateServiceBindings(services) {
  const clonedServices = Object.create(null);
  if (services === null || typeof services !== "object") {
    return clonedServices;
  }
  for (const [serviceName, binding] of Object.entries(services)) {
    const clonedBinding = __nimbusCloneServiceBinding(binding);
    if (clonedBinding !== null) {
      clonedServices[serviceName] = clonedBinding;
    }
  }
  return clonedServices;
}

function __nimbusCreateServiceRegistry(guardStale, asyncHostValue, services) {
  const cache = __nimbusCreateServiceBindings(services);
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
      const lookedUpBinding = __nimbusCloneServiceBinding(
        await asyncHostValue("op_nimbus_ctx_service_lookup", {
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

globalThis.__nimbusCreateContext = function(options = {}) {
  const myGeneration = __nimbusInvocationGeneration;

  const guardStale = () => {
    if (__nimbusInvocationGeneration !== myGeneration) {
      throw new Error(
        "This ctx object is from a previous invocation and cannot be reused"
      );
    }
  };
  const sessionId =
    typeof options.sessionId === "string" && options.sessionId.length > 0
      ? options.sessionId
      : `session-${__nimbusNextSessionId++}`;
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
    return globalThis.__nimbusSyncHostValue(opName, {
      session_id: sessionId,
      ...(payload ?? {}),
    });
  };

  const asyncHostValue = (opName, payload) => {
    guardStale();
    return globalThis.__nimbusAsyncHostValue(opName, {
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
    services: __nimbusCreateServiceRegistry(guardStale, asyncHostValue, services),
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
            return globalThis.__nimbusAsyncHostValue("op_nimbus_document_get", {
              table: tableOrId.table,
              id: tableOrId.id,
              session_id: sessionId,
            });
          }
          throw new Error(
            "Nimbus runtime ctx.db.get currently requires table and id at runtime",
          );
        }
        return globalThis.__nimbusAsyncHostValue("op_nimbus_document_get", {
          table: tableOrId,
          id: maybeId,
          session_id: sessionId,
        });
      },
      query(table) {
        const builderId = syncHostValue("op_nimbus_ctx_query_start", { table });
        return __nimbusCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, sessionId);
      },
      insert(table, fields) {
        return asyncHostValue("op_nimbus_document_insert", {
          table,
          fields,
        });
      },
      patch(table, id, patch) {
        return asyncHostValue("op_nimbus_document_patch", {
          table,
          id,
          patch,
        });
      },
      delete(table, id) {
        return asyncHostValue("op_nimbus_document_delete", {
          table,
          id,
        });
      },
    },
    scheduler: {
      runAfter(delayMs, functionRef, args = {}) {
        const normalized = __nimbusNormalizeFunctionReference(functionRef, "scheduler.runAfter");
        return asyncHostValue("op_nimbus_ctx_scheduler_run_after", {
          delay_ms: delayMs,
          ...normalized,
          args,
        });
      },
      runAt(timestampMs, functionRef, args = {}) {
        const normalized = __nimbusNormalizeFunctionReference(functionRef, "scheduler.runAt");
        return asyncHostValue("op_nimbus_ctx_scheduler_run_at", {
          timestamp_ms: timestampMs,
          ...normalized,
          args,
        });
      },
      cancel(jobId) {
        return asyncHostValue("op_nimbus_ctx_scheduler_cancel", {
          job_id: jobId,
        });
      },
    },
      runQuery(functionRef, args = {}) {
        guardStale();
        return __nimbusRunNamedFunction(
          syncHostValue,
          "op_nimbus_ctx_run_query",
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
        return __nimbusRunNamedFunction(
          syncHostValue,
          "op_nimbus_ctx_run_mutation",
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
        return __nimbusRunNamedFunction(
          syncHostValue,
          "op_nimbus_ctx_run_action",
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

Object.freeze(globalThis.__nimbusSyncHostValue);
Object.freeze(globalThis.__nimbusAsyncHostValue);
Object.freeze(globalThis.__nimbusCreateContext);
"#;

const DENO_RUNTIME_GLOBALS_SOURCE: &str = r#"
const __nimbusRuntimeEnvOverlaySymbol = Symbol.for("nimbus.runtimeEnvOverlay");
const __nimbusRuntimeEnvDeletedMarker = Symbol.for("nimbus.runtimeEnvDeleted");
if (globalThis[__nimbusRuntimeEnvOverlaySymbol] === undefined) {
  Object.defineProperty(globalThis, __nimbusRuntimeEnvOverlaySymbol, {
    value: Object.create(null),
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

function __nimbusRuntimeEnvOverlay() {
  return globalThis[__nimbusRuntimeEnvOverlaySymbol];
}

function __nimbusCreateProcessEnvProxy() {
  const snapshot = __nimbusCoreOps.op_nimbus_runtime_env_snapshot();
  const target = Object.assign(Object.create(null), snapshot);
  return new Proxy(target, {
    get(currentTarget, property) {
      if (typeof property !== "string") {
        return Reflect.get(currentTarget, property);
      }
      const overlay = __nimbusRuntimeEnvOverlay();
      if (Object.prototype.hasOwnProperty.call(overlay, property)) {
        const value = overlay[property];
        return value === __nimbusRuntimeEnvDeletedMarker ? undefined : value;
      }
      const result = __nimbusCoreOps.op_nimbus_runtime_env_get(property);
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
      const overlay = __nimbusRuntimeEnvOverlay();
      if (Object.prototype.hasOwnProperty.call(overlay, property)) {
        return overlay[property] !== __nimbusRuntimeEnvDeletedMarker;
      }
      const result = __nimbusCoreOps.op_nimbus_runtime_env_get(property);
      return result?.status === "allowed";
    },
    ownKeys(currentTarget) {
      const keys = new Set(Reflect.ownKeys(currentTarget));
      for (const property of Reflect.ownKeys(__nimbusRuntimeEnvOverlay())) {
        if (
          typeof property === "string" &&
          __nimbusRuntimeEnvOverlay()[property] === __nimbusRuntimeEnvDeletedMarker
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
      const overlay = __nimbusRuntimeEnvOverlay();
      if (Object.prototype.hasOwnProperty.call(overlay, property)) {
        if (overlay[property] === __nimbusRuntimeEnvDeletedMarker) {
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
      const overlay = __nimbusRuntimeEnvOverlay();
      overlay[property] = stringValue;
      target[property] = stringValue;
      return true;
    },
    deleteProperty(currentTarget, property) {
      if (typeof property === "symbol") {
        return true;
      }
      const overlay = __nimbusRuntimeEnvOverlay();
      overlay[property] = __nimbusRuntimeEnvDeletedMarker;
      delete currentTarget[property];
      return true;
    },
  });
}

function __nimbusDefineNodeFeature(target, property, value) {
  Object.defineProperty(target, property, {
    value,
    configurable: true,
    enumerable: true,
    writable: true,
  });
}

function __nimbusNodeFeatureBoolean(source, property) {
  return source && typeof source === "object" && source[property] === true;
}

function __nimbusCreateNodeProcessFeatures(source, nodeMajor) {
  const features = {};
  __nimbusDefineNodeFeature(
    features,
    "inspector",
    __nimbusNodeFeatureBoolean(source, "inspector"),
  );
  __nimbusDefineNodeFeature(
    features,
    "debug",
    __nimbusNodeFeatureBoolean(source, "debug"),
  );
  __nimbusDefineNodeFeature(features, "uv", __nimbusNodeFeatureBoolean(source, "uv"));
  __nimbusDefineNodeFeature(features, "ipv6", __nimbusNodeFeatureBoolean(source, "ipv6"));
  if (nodeMajor === "20") {
    __nimbusDefineNodeFeature(
      features,
      "require_module",
      __nimbusNodeFeatureBoolean(source, "require_module"),
    );
  } else {
    __nimbusDefineNodeFeature(
      features,
      "openssl_is_boringssl",
      __nimbusNodeFeatureBoolean(source, "openssl_is_boringssl"),
    );
    if (nodeMajor === "24" || nodeMajor === "26") {
      __nimbusDefineNodeFeature(
        features,
        "quic",
        source && typeof source === "object" ? source.quic : undefined,
      );
    }
  }
  __nimbusDefineNodeFeature(
    features,
    "tls_alpn",
    __nimbusNodeFeatureBoolean(source, "tls_alpn"),
  );
  __nimbusDefineNodeFeature(
    features,
    "tls_sni",
    __nimbusNodeFeatureBoolean(source, "tls_sni"),
  );
  __nimbusDefineNodeFeature(
    features,
    "tls_ocsp",
    __nimbusNodeFeatureBoolean(source, "tls_ocsp"),
  );
  __nimbusDefineNodeFeature(features, "tls", __nimbusNodeFeatureBoolean(source, "tls"));
  __nimbusDefineNodeFeature(
    features,
    "cached_builtins",
    __nimbusNodeFeatureBoolean(source, "cached_builtins"),
  );
  if (nodeMajor !== "20") {
    __nimbusDefineNodeFeature(
      features,
      "require_module",
      __nimbusNodeFeatureBoolean(source, "require_module"),
    );
    const sourceTypescript =
      source && typeof source === "object" ? source.typescript : undefined;
    __nimbusDefineNodeFeature(
      features,
      "typescript",
      typeof sourceTypescript === "string" ? sourceTypescript : sourceTypescript === true,
    );
  }
  return features;
}

function __nimbusInstallRuntimeContractGlobals(contract) {
  if (!contract || typeof contract !== "object") {
    return;
  }
  const compatibilityTarget = contract.compatibility_target;
  const nodeApiContract =
    contract.node_api_contract && typeof contract.node_api_contract === "object"
      ? contract.node_api_contract
      : null;
  const nodeMajorMatch =
    typeof compatibilityTarget === "string"
      ? /^node(\d+)$/.exec(compatibilityTarget)
      : null;
  if (nodeApiContract || nodeMajorMatch) {
    const nodeMajor = nodeMajorMatch ? nodeMajorMatch[1] : null;
    const nodeVersion =
      typeof nodeApiContract?.version === "string"
        ? nodeApiContract.version
        : `v${nodeMajor ?? "0"}.0.0-nimbus`;
    const nodeVersionNumber =
      typeof nodeApiContract?.version_number === "string"
        ? nodeApiContract.version_number
        : nodeVersion.replace(/^v/, "");
    const nodeModuleVersion =
      typeof nodeApiContract?.module_version === "string"
        ? nodeApiContract.module_version
        : undefined;
    const nodeReleaseName =
      typeof nodeApiContract?.release_name === "string"
        ? nodeApiContract.release_name
        : "node";
    const nodeReleaseLts =
      typeof nodeApiContract?.release_lts === "string"
        ? nodeApiContract.release_lts
        : undefined;
    if (typeof globalThis.global === "undefined") {
      globalThis.global = globalThis;
    }
    const cwd = typeof contract.paths?.cwd === "string" ? contract.paths.cwd : "/";
    const env = __nimbusCreateProcessEnvProxy();
    const processBase = globalThis.process ?? {};
    // Deno's Node shim owns non-configurable process fields such as
    // process.release, so install a Nimbus-owned wrapper for lane metadata.
    const processTarget = Object.create(
      Object.getPrototypeOf(processBase) ?? Object.prototype,
    );
    const processValue = new Proxy(processTarget, {
      set(target, property, value, receiver) {
        const result = Reflect.set(target, property, value, receiver);
        if (
          result &&
          processBase &&
          typeof processBase === "object"
        ) {
          const baseDescriptor = Object.getOwnPropertyDescriptor(processBase, property);
          if (
            !baseDescriptor ||
            baseDescriptor.writable === true ||
            typeof baseDescriptor.set === "function"
          ) {
            try {
              Reflect.set(processBase, property, value);
            } catch (_error) {}
          }
        }
        return result;
      },
      defineProperty(target, property, descriptor) {
        const result = Reflect.defineProperty(target, property, descriptor);
        if (
          result &&
          processBase &&
          typeof processBase === "object"
        ) {
          const baseDescriptor = Object.getOwnPropertyDescriptor(processBase, property);
          if (!baseDescriptor || baseDescriptor.configurable === true) {
            try {
              Reflect.defineProperty(processBase, property, descriptor);
            } catch (_error) {}
          }
        }
        return result;
      },
      deleteProperty(target, property) {
        const result = Reflect.deleteProperty(target, property);
        if (
          result &&
          processBase &&
          typeof processBase === "object"
        ) {
          const baseDescriptor = Object.getOwnPropertyDescriptor(processBase, property);
          if (!baseDescriptor || baseDescriptor.configurable === true) {
            try {
              Reflect.deleteProperty(processBase, property);
            } catch (_error) {}
          }
        }
        return result;
      },
    });
    for (const property of Reflect.ownKeys(processBase)) {
      if (
        property === "cwd" ||
        property === "env" ||
        property === "features" ||
        property === "version" ||
        property === "versions" ||
        property === "release"
      ) {
        continue;
      }
      const descriptor = Object.getOwnPropertyDescriptor(processBase, property);
      if (descriptor) {
        Object.defineProperty(processValue, property, descriptor);
      }
    }
    const existingVersions =
      processBase.versions && typeof processBase.versions === "object"
        ? processBase.versions
        : {};
    const nextVersions = {
      ...existingVersions,
      node: nodeVersionNumber,
    };
    if (nodeModuleVersion !== undefined) {
      nextVersions.modules = nodeModuleVersion;
    }
    const versions = Object.freeze(nextVersions);
    const existingRelease =
      processBase.release && typeof processBase.release === "object"
        ? processBase.release
        : {};
    const nextRelease = {
      ...existingRelease,
      name: nodeReleaseName,
    };
    if (nodeReleaseLts === undefined) {
      delete nextRelease.lts;
    } else {
      nextRelease.lts = nodeReleaseLts;
    }
    const release = Object.freeze(nextRelease);
    const features = __nimbusCreateNodeProcessFeatures(
      processBase.features,
      nodeMajor,
    );
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
    Object.defineProperty(processValue, "features", {
      value: features,
      configurable: false,
      enumerable: true,
      writable: false,
    });
    Object.defineProperty(processValue, "version", {
      value: nodeVersion,
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
    Object.defineProperty(processValue, "release", {
      value: release,
      configurable: true,
      enumerable: true,
      writable: false,
    });
    Object.defineProperty(processValue, Symbol.toStringTag, {
      value: "process",
      configurable: false,
      enumerable: false,
      writable: true,
    });
    if (processBase && typeof processBase === "object") {
      const baseFeaturesDescriptor =
        Object.getOwnPropertyDescriptor(processBase, "features");
      if (!baseFeaturesDescriptor || baseFeaturesDescriptor.configurable === true) {
        try {
          Reflect.defineProperty(processBase, "features", {
            value: features,
            configurable: true,
            enumerable: true,
            writable: false,
          });
        } catch (_error) {}
      }
    }
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

Object.freeze(__nimbusInstallRuntimeContractGlobals);
"#;

// Keep Deno cleanup out of the bootstrap sources. Those sources are executed
// during startup-snapshot creation, and moving `delete globalThis.Deno` into
// them has already regressed snapshot-backed Locker runtime startup in the
// repaired deno_core fork. The cleanup must remain a separate post-bootstrap
// step until the fork exposes an explicit snapshot-safe alternative. Node22
// now binds its internal substrate against `__bootstrap.ext_node_denoGlobals`,
// so ordinary bundles should not observe the public `globalThis.Deno` contract
// after finalize_bootstrap() completes.
const POST_BOOTSTRAP_SOURCE: &str = r#"
const __nimbusRuntimeContract =
  __nimbusCoreOps.op_nimbus_runtime_contract();
if (globalThis.__nimbusRetainDenoForNodeLazyScripts !== true) {
  delete globalThis.Deno;
}
delete globalThis.__nimbusRetainDenoForNodeLazyScripts;
delete globalThis.__bootstrap;
delete globalThis.bootstrap;
__nimbusInstallRuntimeContractGlobals(__nimbusRuntimeContract);
if (Promise.reject.__nimbusDomainAware !== true) {
  const __nimbusOriginalPromiseReject = Promise.reject;
  function __nimbusDomainAwarePromiseReject(reason) {
    const promise = __nimbusOriginalPromiseReject.apply(this, arguments);
    const domain = globalThis.process?.domain;
    if (domain !== null && domain !== undefined) {
      Object.defineProperty(promise, "domain", {
        configurable: true,
        enumerable: false,
        value: domain,
        writable: true,
      });
      if (reason !== null && typeof reason === "object") {
        Object.defineProperty(reason, "domain", {
          configurable: true,
          enumerable: false,
          value: domain,
          writable: true,
        });
        if (reason.domainThrown === undefined) {
          reason.domainThrown = true;
        }
      }
    }
    return promise;
  }
  Object.defineProperty(__nimbusDomainAwarePromiseReject, "__nimbusDomainAware", {
    configurable: false,
    enumerable: false,
    value: true,
    writable: false,
  });
  Object.defineProperty(Promise, "reject", {
    configurable: true,
    enumerable: false,
    value: __nimbusDomainAwarePromiseReject,
    writable: true,
  });
}
"#;

const RESET_BOOTSTRAP_INVOCATION_STATE_SOURCE: &str =
    "__nimbusNextSessionId = 1; __nimbusInvocationGeneration++;";

pub(crate) fn install_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
    runtime
        .execute_script(
            "<nimbus-runtime:bootstrap:deno-host-call-transport>",
            DENO_HOST_CALL_TRANSPORT_SOURCE,
        )
        .map_err(|error| NimbusRuntimeError::JavaScript(error.to_string()))?;
    runtime
        .execute_script(
            "<nimbus-runtime:bootstrap:context-contract>",
            NIMBUS_CONTEXT_CONTRACT_SOURCE,
        )
        .map_err(|error| NimbusRuntimeError::JavaScript(error.to_string()))?;
    runtime
        .execute_script(
            "<nimbus-runtime:bootstrap:deno-runtime-globals>",
            DENO_RUNTIME_GLOBALS_SOURCE,
        )
        .map_err(|error| NimbusRuntimeError::JavaScript(error.to_string()))?;
    Ok(())
}

pub(crate) fn finalize_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
    // This stays as an intentional second step instead of being folded into
    // install_bootstrap(), because the snapshot path also executes
    // the bootstrap sources during snapshot creation.
    runtime
        .execute_script("<nimbus-runtime:bootstrap:finalize>", POST_BOOTSTRAP_SOURCE)
        .map_err(|error| NimbusRuntimeError::JavaScript(error.to_string()))?;
    Ok(())
}

pub(crate) fn reset_bootstrap_invocation_state(runtime: &mut JsRuntime) -> Result<()> {
    runtime
        .execute_script(
            "<nimbus-runtime:bootstrap:reset>",
            RESET_BOOTSTRAP_INVOCATION_STATE_SOURCE,
        )
        .map_err(|error| NimbusRuntimeError::JavaScript(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DENO_HOST_CALL_TRANSPORT_SOURCE, DENO_RUNTIME_GLOBALS_SOURCE,
        NIMBUS_CONTEXT_CONTRACT_SOURCE,
    };

    #[test]
    fn context_contract_source_does_not_bind_deno_ops() {
        assert!(NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("__nimbusCreateContext"));
        assert!(NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("__nimbusSyncHostValue"));
        assert!(NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("__nimbusAsyncHostValue"));
        assert!(!NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("Deno.core.ops"));
        assert!(!NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("__nimbusCoreOps"));
    }

    #[test]
    fn deno_transport_source_injects_host_call_primitives_only() {
        assert!(DENO_HOST_CALL_TRANSPORT_SOURCE.contains("Deno.core.ops"));
        assert!(DENO_HOST_CALL_TRANSPORT_SOURCE.contains("__nimbusSyncHostValue"));
        assert!(DENO_HOST_CALL_TRANSPORT_SOURCE.contains("__nimbusAsyncHostValue"));
        assert!(!DENO_HOST_CALL_TRANSPORT_SOURCE.contains("__nimbusCreateContext"));
        assert!(!DENO_HOST_CALL_TRANSPORT_SOURCE.contains("__nimbusInstallRuntimeContractGlobals"));
    }

    #[test]
    fn deno_runtime_globals_source_stays_outside_context_contract() {
        assert!(DENO_RUNTIME_GLOBALS_SOURCE.contains("__nimbusInstallRuntimeContractGlobals"));
        assert!(DENO_RUNTIME_GLOBALS_SOURCE.contains("op_nimbus_runtime_env_snapshot"));
        assert!(!NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("__nimbusInstallRuntimeContractGlobals"));
        assert!(!NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("op_nimbus_runtime_env_snapshot"));
    }
}
