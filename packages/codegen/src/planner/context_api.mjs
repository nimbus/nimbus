import {
  createDatabaseProxy,
  createSchedulerProxy,
  normalizeCallableFunctionReference,
} from "./operations.mjs";
import { recordOperation } from "./request_bindings.mjs";

function createContextProxy(filePath, schema, kind, operationLog, argsSchema) {
  const unsupported = (property) => {
    throw new Error(`ctx.${String(property)} requires the Phase 4C runtime`);
  };
  const capabilities = contextCapabilities(kind);

  return {
    db: capabilities.db
      ? createDatabaseProxy(
          filePath,
          schema,
          capabilities.dbWrite,
          operationLog,
          argsSchema,
        )
      : createUnsupportedContextApi("db", unsupported),
    scheduler: capabilities.scheduler
      ? createSchedulerProxy(filePath, operationLog, kind === "http_action")
      : createUnsupportedContextApi("scheduler", unsupported),
    runQuery:
      (kind === "action" || kind === "http_action")
        ? (functionRef, args = {}) =>
            recordOperation(operationLog, {
              type: "call_query",
              ...normalizeCallableFunctionReference(
                functionRef,
                args,
                filePath,
                "ctx.runQuery",
                "query",
              ),
            }, kind === "http_action")
        : () => unsupported("runQuery"),
    runMutation:
      (kind === "action" || kind === "http_action")
        ? (functionRef, args = {}) =>
            recordOperation(operationLog, {
              type: "call_mutation",
              ...normalizeCallableFunctionReference(
                functionRef,
                args,
                filePath,
                "ctx.runMutation",
                "mutation",
              ),
            }, kind === "http_action")
        : () => unsupported("runMutation"),
    runAction:
      (kind === "action" || kind === "http_action")
        ? (functionRef, args = {}) =>
            recordOperation(operationLog, {
              type: "call_action",
              ...normalizeCallableFunctionReference(
                functionRef,
                args,
                filePath,
                "ctx.runAction",
                "action",
              ),
            }, kind === "http_action")
        : () => unsupported("runAction"),
  };
}

function contextCapabilities(kind) {
  switch (kind) {
    case "query":
    case "paginated_query":
      return { db: true, dbWrite: false, scheduler: false };
    case "mutation":
      return { db: true, dbWrite: true, scheduler: true };
    case "action":
    case "http_action":
      return { db: false, dbWrite: false, scheduler: true };
    default:
      return { db: false, dbWrite: false, scheduler: false };
  }
}

function createUnsupportedContextApi(name, unsupported) {
  return new Proxy(
    {},
    {
      get(_target, property) {
        if (typeof property === "symbol") {
          return undefined;
        }
        return new Proxy(() => undefined, {
          apply() {
            unsupported(`${name}.${String(property)}`);
          },
          get() {
            unsupported(`${name}.${String(property)}`);
          },
        });
      },
    },
  );
}

export { createContextProxy };
