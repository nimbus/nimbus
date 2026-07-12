// Captured at bootstrap, before any guest code evaluates: the transport
// script (which runs earlier in BOOTSTRAP_SCRIPTS) defines this helper, and
// binding it here means a guest that later shadows or tampers with the
// global name cannot re-attach caller AsyncLocalStorage context to locally
// dispatched ctx.run* callees.
const __nimbusDetachedNestedCall = globalThis.__nimbusCallDetachedFromInvocationContext;

// Host-captured callee-lane lookup for the nested ctx.run* dispatcher. The
// generated bundle evaluates after this bootstrap script but before any guest
// handler runs, and registers its per-function `runtime_environment` lookup
// exactly once through this registrar (see
// packages/codegen/src/emit/runtime_bundle_dispatch_global_invoke.mjs). The
// dispatcher below consults this captured reference — never a guest-shadowable
// global read at call time — so guest code that deletes or reassigns a global
// name cannot redirect lane routing. The registrar is one-shot: a second call
// throws, so an import-time package or handler cannot replace a legitimately
// registered lookup with an always-"same lane" impostor. The registrar itself
// is non-writable and non-configurable, so it cannot be swapped out either.
let __nimbusCapturedCalleeLaneLookup = null;
let __nimbusCalleeLaneLookupRegistered = false;
Object.defineProperty(globalThis, "__nimbusRegisterLocalFunctionRuntimeEnvironment", {
  value: function __nimbusRegisterLocalFunctionRuntimeEnvironment(lookup) {
    if (__nimbusCalleeLaneLookupRegistered) {
      throw new Error(
        "nimbus callee-lane lookup is already registered for this isolate",
      );
    }
    if (typeof lookup !== "function") {
      throw new TypeError("nimbus callee-lane lookup must be a function");
    }
    __nimbusCalleeLaneLookupRegistered = true;
    __nimbusCapturedCalleeLaneLookup = lookup;
  },
  configurable: false,
  enumerable: false,
  writable: false,
});

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

// Single-argument ctx.db calls receive a table-scoped document id
// (`<table>:<key>`), the protocol contract shared with the host bridge. The
// table derived here is advisory only: the bridge re-resolves the scoped id
// and rejects any mismatch, so a forged prefix cannot redirect the operation.
const __nimbusTableFromScopedId = function __nimbusTableFromScopedId(id, label) {
  if (typeof id !== "string") {
    throw new TypeError(
      `ctx.${label}(...) requires a table-scoped document id string`,
    );
  }
  const separator = id.indexOf(":");
  if (separator <= 0 || separator >= id.length - 1) {
    throw new TypeError(
      `ctx.${label}(...) requires a table-scoped document id like "tasks:...", got "${id}"`,
    );
  }
  return id.slice(0, separator);
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
  // Same-isolate local dispatch is an optimization that is only correct when
  // the callee executes on this isolate's runtime lane. Generated bundles are
  // shared across every V8 lane of an app (default web-standard isolate and
  // "use node" isolates alike), so a nested call whose callee is declared for
  // a different lane must go through HOST dispatch: the engine path resolves
  // the callee's own lane, semantics profile, and capability set. Running it
  // locally would execute e.g. a default-lane mutation under Node/Host
  // semantics (unfrozen clock, unseeded random) or make a "use node" action
  // import node builtins inside the web isolate.
  const useLocalDispatch = (() => {
    if (typeof localInvoker !== "function") {
      return false;
    }
    // `__nimbusRuntimeEnvironmentLane` is frozen at bootstrap
    // (deno_runtime_globals.js), so reading it here is not guest-shadowable.
    const currentLane = globalThis.__nimbusRuntimeEnvironmentLane;
    if (typeof currentLane !== "string") {
      // No lane signal anywhere on this isolate: a genuine pre-lane isolate
      // (no runtime contract installed, so no lane metadata exists at all).
      // Preserve the historical behavior — local dispatch whenever a
      // dispatcher exists. Real deployed isolates always freeze a lane string
      // at bootstrap and never take this branch.
      return true;
    }
    // The lane is known, so a callee-lane lookup MUST have been registered by
    // the bundle at eval time, before any guest code ran. If it is missing —
    // never registered, or the tampering end state where a guest deleted the
    // old global — fail SAFE to host dispatch, which resolves the callee's own
    // lane and semantics. Never assume local: local dispatch under a mismatched
    // lane runs a callee under the wrong clock/seed/capability profile.
    const calleeLaneLookup = __nimbusCapturedCalleeLaneLookup;
    if (typeof calleeLaneLookup !== "function") {
      return false;
    }
    const calleeLane = calleeLaneLookup(normalized.name);
    // Unknown callee (not in this bundle's manifest) also goes to the host,
    // which can resolve registry-native functions the bundle cannot.
    return typeof calleeLane === "string" && calleeLane === currentLane;
  })();
  if (useLocalDispatch) {
    syncHostValue("op_nimbus_ctx_runtime_enter_nested_call", {
      name: normalized.name,
      visibility: normalized.visibility,
      kind,
      host_call_session_id: hostCallSessionId,
    });
    const invokeLocal = () =>
      localInvoker({
        kind,
        function_name: normalized.name,
        args,
        visibility: normalized.visibility,
        hostCallSessionId,
        ...(nestedAuthContext ? { auth: nestedAuthContext } : {}),
      });
    // Start the nested handler from the root async context (as the host
    // dispatch path below does) so caller AsyncLocalStorage state never
    // propagates into ctx.run* callees. The helper reference was captured at
    // bootstrap; guest reassignment of the global cannot bypass detachment.
    return await (typeof __nimbusDetachedNestedCall === "function"
      ? __nimbusDetachedNestedCall(invokeLocal)
      : invokeLocal());
  }
  return globalThis.__nimbusAsyncHostValue(asyncOpName, {
    ...normalized,
    args,
    host_call_session_id: hostCallSessionId,
    ...(nestedAuthContext ? { auth: nestedAuthContext } : {}),
  });
};

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
  const hostCallSessionId = __nimbusCurrentHostCallSessionId();
  const requestedHostCallSessionId =
    typeof options.hostCallSessionId === "string" && options.hostCallSessionId.length > 0
      ? options.hostCallSessionId
      : null;
  if (requestedHostCallSessionId !== null && requestedHostCallSessionId !== hostCallSessionId) {
    throw new Error("Nimbus runtime host-call session is stale or forged");
  }
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

  const requestKindFromSessionId = (() => {
    const separator = hostCallSessionId.indexOf(":");
    return separator > 0 ? hostCallSessionId.slice(0, separator) : null;
  })();
  const requestKind =
    options.request !== null &&
    typeof options.request === "object" &&
    typeof options.request.kind === "string"
      ? options.request.kind
      : requestKindFromSessionId;
  const capabilities = (() => {
    switch (requestKind) {
      case "query":
      case "paginated_query":
        return {
          db: true,
          dbWrite: false,
          scheduler: false,
          nestedCalls: {
            query: true,
            mutation: false,
            action: false,
          },
        };
      case "mutation":
        return {
          db: true,
          dbWrite: true,
          scheduler: true,
          nestedCalls: {
            query: true,
            mutation: true,
            action: false,
          },
        };
      case "action":
      case "http_action":
        return {
          db: false,
          dbWrite: false,
          scheduler: true,
          nestedCalls: {
            query: true,
            mutation: true,
            action: true,
          },
        };
      default:
        return {
          db: true,
          dbWrite: true,
          scheduler: true,
          nestedCalls: {
            query: true,
            mutation: true,
            action: true,
          },
        };
    }
  })();
  const unsupported = (label) => {
    throw new Error(
      `Nimbus runtime ctx.${label} is not available for ${requestKind ?? "dynamic"} handlers`,
    );
  };

  const db = capabilities.db
    ? {
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
            return globalThis.__nimbusAsyncHostValue("op_nimbus_document_get", {
              table: __nimbusTableFromScopedId(tableOrId, "db.get"),
              id: tableOrId,
              host_call_session_id: hostCallSessionId,
            });
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
          if (!capabilities.dbWrite) {
            unsupported("db.insert");
          }
          return asyncHostValue("op_nimbus_document_insert", {
            table,
            fields,
          });
        },
        patch(tableOrId, idOrPatch, maybePatch) {
          if (!capabilities.dbWrite) {
            unsupported("db.patch");
          }
          if (maybePatch === undefined) {
            return asyncHostValue("op_nimbus_document_patch", {
              table: __nimbusTableFromScopedId(tableOrId, "db.patch"),
              id: tableOrId,
              patch: idOrPatch,
            });
          }
          return asyncHostValue("op_nimbus_document_patch", {
            table: tableOrId,
            id: idOrPatch,
            patch: maybePatch,
          });
        },
        delete(tableOrId, maybeId) {
          if (!capabilities.dbWrite) {
            unsupported("db.delete");
          }
          if (maybeId === undefined) {
            return asyncHostValue("op_nimbus_document_delete", {
              table: __nimbusTableFromScopedId(tableOrId, "db.delete"),
              id: tableOrId,
            });
          }
          return asyncHostValue("op_nimbus_document_delete", {
            table: tableOrId,
            id: maybeId,
          });
        },
      }
    : {
        get() {
          unsupported("db.get");
        },
        query() {
          unsupported("db.query");
        },
        insert() {
          unsupported("db.insert");
        },
        patch() {
          unsupported("db.patch");
        },
        delete() {
          unsupported("db.delete");
        },
      };

  const scheduler = capabilities.scheduler
    ? {
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
      }
    : {
        runAfter() {
          unsupported("scheduler.runAfter");
        },
        runAt() {
          unsupported("scheduler.runAt");
        },
        cancel() {
          unsupported("scheduler.cancel");
        },
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
    db,
    scheduler,
    runQuery(functionRef, args = {}) {
      guardStale();
      if (!capabilities.nestedCalls.query) {
        unsupported("runQuery");
      }
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
      if (!capabilities.nestedCalls.mutation) {
        unsupported("runMutation");
      }
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
      if (!capabilities.nestedCalls.action) {
        unsupported("runAction");
      }
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
