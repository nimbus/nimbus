import { renderValidatorType } from "./schema_types.mjs";

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
    return "unknown";
  }
  const nextSeen = new Set(seen);
  nextSeen.add(fn.name);

  if (fn.returnsSchema) {
    return renderValidatorType(fn.returnsSchema, { idSymbol: "Id" });
  }

  switch (fn.kind) {
    case "query":
      return inferQueryResultType(fn.plan, schema);
    case "paginated_query":
      return inferPaginatedItemType(fn.plan, schema);
    case "mutation":
      return inferMutationResultType(fn.plan, schema);
    case "action":
      return inferActionResultType(fn.plan, schema, functionIndex, nextSeen);
    default:
      return "unknown";
  }
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
      return target
        ? inferFunctionResultType(target, schema, functionIndex, seen)
        : "unknown";
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
