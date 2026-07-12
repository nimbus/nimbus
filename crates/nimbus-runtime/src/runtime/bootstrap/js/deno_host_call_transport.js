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
// Nested-call context detachment: a locally-dispatched nested invocation
// (globalThis.__nimbusInvokeNamedLocal) must start from the same async
// context a host-dispatched one would — the root frame captured here at
// bootstrap — so AsyncLocalStorage data never propagates into
// ctx.runQuery/runMutation/runAction (the documented Convex default-runtime
// caveat, and dispatch-path parity between the local and host paths).
{
  const nimbusGetAsyncContext = Deno.core.getAsyncContext;
  const nimbusSetAsyncContext = Deno.core.setAsyncContext;
  const supported =
    typeof nimbusGetAsyncContext === "function" &&
    typeof nimbusSetAsyncContext === "function";
  const rootAsyncContext = supported ? nimbusGetAsyncContext() : undefined;
  // Non-writable, non-configurable: the context contract captures this
  // reference at bootstrap, and the global itself must also be immune to
  // guest reassignment so no name-based caller can be redirected to a
  // version that leaks caller ALS context into callees.
  Object.defineProperty(globalThis, "__nimbusCallDetachedFromInvocationContext", {
    value: supported
      ? function __nimbusCallDetachedFromInvocationContext(fn) {
          const previous = nimbusGetAsyncContext();
          nimbusSetAsyncContext(rootAsyncContext);
          try {
            return fn();
          } finally {
            nimbusSetAsyncContext(previous);
          }
        }
      : function __nimbusCallDetachedFromInvocationContext(fn) {
          return fn();
        },
    configurable: false,
    enumerable: false,
    writable: false,
  });
}

const __nimbusContextHostCallOps = new Set([
  "op_nimbus_ctx_query_start",
  "op_nimbus_ctx_query_with_index",
  "op_nimbus_ctx_query_filter",
  "op_nimbus_ctx_query_order",
  "op_nimbus_ctx_query",
  "op_nimbus_ctx_paginated_query",
  "op_nimbus_ctx_mutation",
  "op_nimbus_ctx_action",
  "op_nimbus_document_get",
  "op_nimbus_document_insert",
  "op_nimbus_document_patch",
  "op_nimbus_document_delete",
  "op_nimbus_ctx_query_collect",
  "op_nimbus_ctx_query_take",
  "op_nimbus_ctx_query_paginate",
  "op_nimbus_ctx_query_first",
  "op_nimbus_ctx_query_unique",
  "op_nimbus_ctx_scheduler_run_after",
  "op_nimbus_ctx_scheduler_run_at",
  "op_nimbus_ctx_scheduler_cancel",
  "op_nimbus_ctx_runtime_enter_nested_call",
  "op_nimbus_ctx_resolve_callee_lane",
  "op_nimbus_ctx_run_query",
  "op_nimbus_ctx_run_mutation",
  "op_nimbus_ctx_run_action",
  "op_nimbus_ctx_service_lookup",
  "op_nimbus_cf_kv_get",
  "op_nimbus_cf_kv_put",
  "op_nimbus_cf_kv_delete",
  "op_nimbus_cf_kv_list",
]);

const __nimbusCurrentHostCallSessionId = function __nimbusCurrentHostCallSessionId() {
  const operation = __nimbusCoreOps.op_nimbus_runtime_host_call_session_id;
  if (typeof operation !== "function") {
    throw new Error("Nimbus runtime host-call session op not found");
  }
  return operation();
};

const __nimbusBindHostCallPayload = function __nimbusBindHostCallPayload(opName, payload) {
  if (!__nimbusContextHostCallOps.has(opName)) {
    return payload;
  }
  if (payload !== null && payload !== undefined && (typeof payload !== "object" || Array.isArray(payload))) {
    throw new Error(`Nimbus runtime host-call payload must be an object for ${opName}`);
  }
  const currentSessionId = __nimbusCurrentHostCallSessionId();
  const providedSessionId = payload?.host_call_session_id;
  if (
    providedSessionId !== undefined &&
    providedSessionId !== null &&
    providedSessionId !== "" &&
    providedSessionId !== currentSessionId
  ) {
    throw new Error(`Nimbus runtime host-call session is stale or forged for ${opName}`);
  }
  return {
    ...(payload ?? {}),
    host_call_session_id: currentSessionId,
  };
};

// The host-call transports carry trust decisions the runtime acts on (e.g. the
// host-authoritative callee lane for nested ctx.run* dispatch), so the globals
// themselves must be immune to guest reassignment: a name-based caller that
// could swap in an impostor would forge the host's answer. Frozen non-writable/
// non-configurable, matching __nimbusCallDetachedFromInvocationContext above.
Object.defineProperty(globalThis, "__nimbusSyncHostValue", {
  configurable: false,
  enumerable: false,
  writable: false,
  value: function(opName, payload) {
    const operation = __nimbusCoreOps[opName];
    if (typeof operation !== "function") {
      throw new Error(`Nimbus runtime sync host op not found: ${opName}`);
    }
    const response = operation(__nimbusBindHostCallPayload(opName, payload));
    if (!response || response.status !== "ok") {
      const error = new Error(
        `Nimbus runtime sync host call failed for ${opName}: ${__nimbusFormatHostError(response?.error)}`,
      );
      error.nimbusHostError = response?.error ?? null;
      throw error;
    }
    return response.value;
  },
});

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

Object.defineProperty(globalThis, "__nimbusAsyncHostValue", {
  configurable: false,
  enumerable: false,
  writable: false,
  value: async function(opName, payload) {
    const operation = __nimbusCoreOps[opName];
    if (typeof operation !== "function") {
      throw new Error(`Nimbus runtime async host op not found: ${opName}`);
    }
    const response = await operation(__nimbusBindHostCallPayload(opName, payload));
    if (!response || response.status !== "ok") {
      const error = new Error(
        `Nimbus runtime async host call failed for ${opName}: ${__nimbusFormatHostError(response?.error)}`,
      );
      error.nimbusHostError = response?.error ?? null;
      throw error;
    }
    return response.value;
  },
});

let __nimbusWaitUntilQueue = [];

globalThis.__nimbusWaitUntil = function(promise) {
  if (promise === null || promise === undefined || typeof promise.then !== "function") {
    throw new TypeError("Nimbus waitUntil requires a Promise-like value");
  }
  const markPending = __nimbusCoreOps.op_nimbus_runtime_wait_until_pending;
  if (typeof markPending === "function") {
    markPending();
  }
  const tracked = Promise.resolve(promise);
  tracked.catch(() => {});
  __nimbusWaitUntilQueue.push(tracked);
};

globalThis.__nimbusDrainWaitUntil = async function() {
  let rejected = 0;
  while (__nimbusWaitUntilQueue.length > 0) {
    const batch = __nimbusWaitUntilQueue;
    __nimbusWaitUntilQueue = [];
    const settled = await Promise.allSettled(batch);
    for (const result of settled) {
      if (result.status === "rejected") {
        rejected++;
      }
    }
  }
  return { rejected };
};

globalThis.__nimbusResetWaitUntil = function() {
  __nimbusWaitUntilQueue = [];
};
