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

const __nimbusFormatHostError = function __nimbusFormatHostError(error) {
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
};

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
const __nimbusNormalizeFieldName = function __nimbusNormalizeFieldName(field) {
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
};

const __nimbusCreateConstraintBuilder = function __nimbusCreateConstraintBuilder() {
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
};

const __nimbusCollectConstraintFilters = function __nimbusCollectConstraintFilters(builderFn, label) {
  const builder = __nimbusCreateConstraintBuilder();
  const result = builderFn ? builderFn(builder) : builder;
  if (result !== undefined && result !== builder && result?.__filters !== builder.__filters) {
    throw new Error(`ctx.db.${label}(...) must return the provided builder`);
  }
  return [...builder.__filters];
};

const __nimbusCreateQueryBuilder = function __nimbusCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, hostCallSessionId) {
  return Object.freeze({
    __builderId: builderId,
    withIndex(indexName, builderFn) {
      syncHostValue("op_nimbus_ctx_query_with_index", {
        builder_id: builderId,
        index_name: indexName,
        filters: __nimbusCollectConstraintFilters(builderFn, "withIndex"),
      });
      return __nimbusCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, hostCallSessionId);
    },
    filter(builderFn) {
      syncHostValue("op_nimbus_ctx_query_filter", {
        builder_id: builderId,
        filters: __nimbusCollectConstraintFilters(builderFn, "filter"),
      });
      return __nimbusCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, hostCallSessionId);
    },
    order(direction) {
      syncHostValue("op_nimbus_ctx_query_order", {
        builder_id: builderId,
        direction,
      });
      return __nimbusCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, hostCallSessionId);
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
};

const __nimbusNormalizeFunctionReference = function __nimbusNormalizeFunctionReference(functionRef, label) {
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
};

const __nimbusRunNamedFunction = async function __nimbusRunNamedFunction(
  syncHostValue,
  asyncOpName,
  hostCallSessionId,
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
      host_call_session_id: hostCallSessionId,
    });
    return await localInvoker({
      kind,
      function_name: normalized.name,
      args,
      visibility: normalized.visibility,
      hostCallSessionId,
      ...(nestedAuthContext ? { auth: nestedAuthContext } : {}),
    });
  }
  return globalThis.__nimbusAsyncHostValue(asyncOpName, {
    ...normalized,
    args,
    host_call_session_id: hostCallSessionId,
    ...(nestedAuthContext ? { auth: nestedAuthContext } : {}),
  });
};

let __nimbusNextHostCallSessionId = 1;
let __nimbusInvocationGeneration = 0;

globalThis.__nimbusCreateContext = function(options = {}) {
  const myGeneration = __nimbusInvocationGeneration;

  const guardStale = () => {
    if (__nimbusInvocationGeneration !== myGeneration) {
      throw new Error(
        "This ctx object is from a previous invocation and cannot be reused"
      );
    }
  };
  const hostCallSessionId =
    typeof options.hostCallSessionId === "string" && options.hostCallSessionId.length > 0
      ? options.hostCallSessionId
      : `host-call-session-${__nimbusNextHostCallSessionId++}`;
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
  const syncHostValue = (opName, payload) => {
    guardStale();
    return globalThis.__nimbusSyncHostValue(opName, {
      host_call_session_id: hostCallSessionId,
      ...(payload ?? {}),
    });
  };

  const asyncHostValue = (opName, payload) => {
    guardStale();
    return globalThis.__nimbusAsyncHostValue(opName, {
      host_call_session_id: hostCallSessionId,
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
              host_call_session_id: hostCallSessionId,
            });
          }
          throw new Error(
            "Nimbus runtime ctx.db.get currently requires table and id at runtime",
          );
        }
        return globalThis.__nimbusAsyncHostValue("op_nimbus_document_get", {
          table: tableOrId,
          id: maybeId,
          host_call_session_id: hostCallSessionId,
        });
      },
      query(table) {
        const builderId = syncHostValue("op_nimbus_ctx_query_start", { table });
        return __nimbusCreateQueryBuilder(syncHostValue, asyncHostValue, builderId, hostCallSessionId);
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
          hostCallSessionId,
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
          hostCallSessionId,
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
          hostCallSessionId,
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

const __nimbusRuntimeEnvOverlay = function __nimbusRuntimeEnvOverlay() {
  return globalThis[__nimbusRuntimeEnvOverlaySymbol];
};

// Node rejects accessor descriptors and partial data descriptors on
// `process.env` with an `ERR_INVALID_OBJECT_DEFINE_PROPERTY` TypeError. Build
// the same error shape (a TypeError carrying that `code`) for the proxy's
// defineProperty trap.
const __nimbusErrInvalidObjectDefineProperty = function __nimbusErrInvalidObjectDefineProperty(message) {
  const error = new TypeError(message);
  error.code = "ERR_INVALID_OBJECT_DEFINE_PROPERTY";
  return error;
};

const __nimbusCreateProcessEnvProxy = function __nimbusCreateProcessEnvProxy() {
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
      if (typeof value !== "string") {
        // Node emits the DEP0104 deprecation warning when a non-string value is
        // assigned to a process.env property; the value is still coerced to a
        // string. Mirror that so the warning fires before the coercion.
        const runtimeProcess = globalThis.process;
        if (runtimeProcess && typeof runtimeProcess.emitWarning === "function") {
          runtimeProcess.emitWarning(
            "Assigning any value other than a string, number, or boolean to a " +
              "process.env property is deprecated. Please make sure to convert the value " +
              "to a string before setting process.env with it.",
            "DeprecationWarning",
            "DEP0104",
          );
        }
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
    defineProperty(_currentTarget, property, descriptor) {
      // Node rejects accessor descriptors and any data descriptor that is not
      // fully configurable/writable/enumerable, then writes accepted values
      // through to the environment (matching the order in deno's process.env
      // polyfill).
      if (descriptor.get || descriptor.set) {
        throw __nimbusErrInvalidObjectDefineProperty(
          "'process.env' does not accept an accessor(getter/setter) descriptor",
        );
      }
      if (
        !descriptor.configurable ||
        !descriptor.enumerable ||
        !descriptor.writable
      ) {
        throw __nimbusErrInvalidObjectDefineProperty(
          "'process.env' only accepts a configurable, writable, and enumerable data descriptor",
        );
      }
      if (typeof property === "symbol") {
        return Reflect.defineProperty(target, property, descriptor);
      }
      const stringValue = String(descriptor.value);
      const overlay = __nimbusRuntimeEnvOverlay();
      overlay[property] = stringValue;
      target[property] = stringValue;
      return true;
    },
  });
};

const __nimbusDefineNodeFeature = function __nimbusDefineNodeFeature(target, property, value) {
  Object.defineProperty(target, property, {
    value,
    configurable: true,
    enumerable: true,
    writable: true,
  });
};

const __nimbusDefineNodeFeatureGetter = function __nimbusDefineNodeFeatureGetter(target, property, value) {
  Object.defineProperty(target, property, {
    get() {
      return value;
    },
    configurable: true,
    enumerable: true,
  });
};

const __nimbusNodeFeatureBoolean = function __nimbusNodeFeatureBoolean(source, property) {
  return source && typeof source === "object" && source[property] === true;
};

const __nimbusCreateNodeProcessFeatures = function __nimbusCreateNodeProcessFeatures(source, nodeMajor) {
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
    __nimbusDefineNodeFeatureGetter(
      features,
      "typescript",
      typeof sourceTypescript === "string" ? sourceTypescript : sourceTypescript === true,
    );
  }
  return features;
};

const __nimbusSyncNodeProcessFeatures = function __nimbusSyncNodeProcessFeatures(target, source) {
  if (!target || typeof target !== "object") {
    return source;
  }
  for (const property of Reflect.ownKeys(target)) {
    if (!Reflect.has(source, property)) {
      try {
        delete target[property];
      } catch (_error) {}
    }
  }
  for (const property of Reflect.ownKeys(source)) {
    const descriptor = Object.getOwnPropertyDescriptor(source, property);
    if (descriptor) {
      try {
        Object.defineProperty(target, property, descriptor);
      } catch (_error) {}
    }
  }
  return target;
};

const __nimbusInstallRuntimeContractGlobals = function __nimbusInstallRuntimeContractGlobals(contract) {
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
    const processValue =
      processBase && typeof processBase === "object" ? processBase : {};
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
    const desiredFeatures = __nimbusCreateNodeProcessFeatures(
      processBase.features,
      nodeMajor,
    );
    const features = __nimbusSyncNodeProcessFeatures(
      processBase.features,
      desiredFeatures,
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
    let globalProcessValue = processValue;
    Object.defineProperty(globalThis, "process", {
      get() {
        return globalProcessValue;
      },
      set(value) {
        globalProcessValue = value;
      },
      configurable: true,
      enumerable: false,
    });
    return;
  }
  delete globalThis.Buffer;
  delete globalThis.global;
  delete globalThis.process;
};

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
const __nimbusCompatibilityTarget =
  __nimbusRuntimeContract?.compatibility_target;
const __nimbusCompatibilityMatch =
  typeof __nimbusCompatibilityTarget === "string"
    ? /^node(\d+)$/.exec(__nimbusCompatibilityTarget)
    : null;
if (__nimbusCompatibilityMatch !== null) {
  const __nimbusWasmStreamingCore = Deno.core;
  const __nimbusWasmStreamingFetchModule =
    globalThis.__nimbusDenoFetchModule ??
      __nimbusWasmStreamingCore.loadExtScript("ext:deno_fetch/26_fetch.js");
  __nimbusWasmStreamingCore.setWasmStreamingCallback(
    function __nimbusWasmStreamingCallback(source, rid) {
      return __nimbusWasmStreamingFetchModule.handleWasmStreaming(source, rid);
    },
  );
}
delete globalThis.__nimbusDenoFetchModule;
if (globalThis.__nimbusRetainDenoForNodeLazyScripts !== true) {
  delete globalThis.Deno;
}
delete globalThis.__nimbusRetainDenoForNodeLazyScripts;
delete globalThis.__bootstrap;
delete globalThis.bootstrap;
__nimbusInstallRuntimeContractGlobals(__nimbusRuntimeContract);
const __nimbusNodeVersion =
  __nimbusRuntimeContract?.node_api_contract?.version_number;
const __nimbusNodeRuntimeMajor = __nimbusCompatibilityMatch
  ? Number.parseInt(__nimbusCompatibilityMatch[1], 10)
  : typeof __nimbusNodeVersion === "string"
    ? Number.parseInt(__nimbusNodeVersion, 10)
    : undefined;
Object.defineProperty(globalThis, "__nimbusNodeRuntimeMajor", {
  value: __nimbusNodeRuntimeMajor,
  configurable: true,
  enumerable: false,
  writable: true,
});
if (globalThis.process && typeof globalThis.process === "object") {
  Object.defineProperty(globalThis.process, "__nimbusNodeRuntimeMajor", {
    value: __nimbusNodeRuntimeMajor,
    configurable: true,
    enumerable: false,
    writable: true,
  });
}
if (Promise.reject.__nimbusDomainAware !== true) {
  const __nimbusOriginalPromiseReject = Promise.reject;
  const __nimbusDomainAwarePromiseReject = function __nimbusDomainAwarePromiseReject(reason) {
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
  };
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
{
  for (const __nimbusGlobalName of Object.keys(globalThis)) {
    if (!__nimbusGlobalName.startsWith("__nimbus")) {
      continue;
    }
    const __nimbusGlobalDescriptor =
      Object.getOwnPropertyDescriptor(globalThis, __nimbusGlobalName);
    if (
      __nimbusGlobalDescriptor &&
      __nimbusGlobalDescriptor.configurable === true &&
      __nimbusGlobalDescriptor.enumerable === true
    ) {
      Object.defineProperty(globalThis, __nimbusGlobalName, {
        ...__nimbusGlobalDescriptor,
        enumerable: false,
      });
    }
  }
}
"#;

const RESET_BOOTSTRAP_INVOCATION_STATE_SOURCE: &str = r#"
__nimbusNextHostCallSessionId = 1;
__nimbusInvocationGeneration++;
{
  const __nimbusRuntimeExecPath = __nimbusCoreOps.op_nimbus_runtime_exec_path();
  if (
    globalThis.process &&
    typeof globalThis.process === "object" &&
    typeof __nimbusRuntimeExecPath === "string" &&
    __nimbusRuntimeExecPath.length > 0
  ) {
    globalThis.process.execPath = __nimbusRuntimeExecPath;
    if (Array.isArray(globalThis.process.argv) && globalThis.process.argv.length > 0) {
      globalThis.process.argv[0] = __nimbusRuntimeExecPath;
    }
  }
}
"#;

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
