import { isTrivialValidator, renderValidatorType } from "./schema_types.mjs";

function renderArgsType(argsSchema) {
  const entries = Object.entries(argsSchema ?? {});
  if (entries.length === 0) {
    return "{}";
  }

  const fields = entries.map(
    ([fieldName, validator]) =>
      `  ${JSON.stringify(fieldName)}: ${renderValidatorType(validator, { idSymbol: "Id" })};`,
  );
  return `{\n${fields.join("\n")}\n}`;
}

function inferFunctionResultType(fn, schema, functionIndex, seen = new Set()) {
  if (seen.has(fn.name)) {
    return { type: "unknown", source: "fallback-recursive" };
  }
  const nextSeen = new Set(seen);
  nextSeen.add(fn.name);

  const hasExplicitReturn =
    fn.returnsSchema && !isTrivialValidator(fn.returnsSchema);
  if (hasExplicitReturn) {
    return {
      type: renderValidatorType(fn.returnsSchema, { idSymbol: "Id" }),
      source: "explicit",
    };
  }

  let inferred;
  switch (fn.kind) {
    case "query":
      inferred = inferQueryResultType(fn.plan, schema);
      break;
    case "paginated_query":
      inferred = inferPaginatedItemType(fn.plan, schema);
      break;
    case "mutation":
      inferred = inferMutationResultType(fn.plan, schema);
      break;
    case "action":
      inferred = inferActionResultType(fn.plan, schema, functionIndex, nextSeen);
      break;
    default:
      inferred = "unknown";
  }

  if (inferred !== "unknown") {
    return { type: inferred, source: "plan-inferred" };
  }

  const conventional = inferFromModuleConvention(fn, schema);
  if (conventional) {
    return { type: conventional, source: "convention-inferred" };
  }

  if (fn.returnsSchema) {
    return {
      type: renderValidatorType(fn.returnsSchema, { idSymbol: "Id" }),
      source: "fallback-trivial-validator",
    };
  }

  return { type: "unknown", source: "fallback-no-validator" };
}

function inferQueryResultType(plan, schema) {
  if (isQueryShape(plan)) {
    return `${inferDocumentTypeForTable(plan.table, schema)}[]`;
  }
  if (plan?.type === "get") {
    return `${inferDocumentTypeForTable(plan.table, schema)} | null`;
  }
  if (plan?.type === "first" || plan?.type === "unique") {
    return `${inferQueryResultType(plan.query, schema).replace(/\[\]$/, "")} | null`;
  }
  return "unknown";
}

function inferPaginatedItemType(plan, schema) {
  if (isQueryShape(plan)) {
    return inferDocumentTypeForTable(plan.table, schema);
  }
  return "unknown";
}

function inferMutationResultType(plan, schema) {
  if (isQueryShape(plan) || plan?.type === "get" || plan?.type === "first" || plan?.type === "unique") {
    return inferQueryResultType(plan, schema);
  }
  switch (plan?.type) {
    case "insert":
    case "update":
      return `Id<${JSON.stringify(plan.table ?? "unknown")}>`;
    case "delete":
    case "schedule_cancel":
      return "null";
    case "schedule_run_after":
    case "schedule_run_at":
      return "string";
    default:
      return "unknown";
  }
}

function inferActionResultType(plan, schema, functionIndex, seen) {
  switch (plan?.type) {
    case "query":
      return inferQueryResultType(plan.query, schema);
    case "paginated_query": {
      const itemType = inferPaginatedItemType(plan.query, schema);
      return `{\n  data: ${itemType}[];\n  next_cursor: string | null;\n  has_more: boolean;\n}`;
    }
    case "mutation":
      return inferMutationResultType(plan.mutation, schema);
    case "call_query":
    case "call_mutation":
    case "call_action": {
      const target = functionIndex.get(plan.name);
      if (!target) {
        return "unknown";
      }
      const result = inferFunctionResultType(target, schema, functionIndex, seen);
      return result.type;
    }
    case "schedule_run_after":
    case "schedule_run_at":
      return "string";
    case "schedule_cancel":
      return "null";
    default:
      return "unknown";
  }
}

const LIST_EXPORT_NAMES = new Set(["list", "recent", "all", "active"]);
const SINGLETON_EXPORT_NAMES = new Set(["byId", "get", "current", "first"]);

function inferFromModuleConvention(fn, schema) {
  if (fn.kind !== "query" && fn.kind !== "paginated_query") {
    return null;
  }
  const [moduleName, exportName] = String(fn.name).split(":");
  if (!moduleName || !exportName) {
    return null;
  }
  if (!schema?.tables?.[moduleName]) {
    return null;
  }
  const docType = `Doc<${JSON.stringify(moduleName)}>`;
  if (fn.kind === "paginated_query") {
    return docType;
  }
  if (LIST_EXPORT_NAMES.has(exportName)) {
    return `${docType}[]`;
  }
  if (SINGLETON_EXPORT_NAMES.has(exportName)) {
    return `${docType} | null`;
  }
  return null;
}

function inferDocumentTypeForTable(tableName, schema) {
  if (schema.tables?.[tableName]) {
    return `Doc<${JSON.stringify(tableName)}>`;
  }
  return "unknown";
}

function isQueryShape(value) {
  return (
    value &&
    typeof value === "object" &&
    typeof value.table === "string" &&
    Array.isArray(value.filters) &&
    "order" in value &&
    "limit" in value
  );
}

export { inferFunctionResultType, renderArgsType };
