import { inferFunctionResultType, renderArgsType } from "./type_inference.mjs";

function helperCall(fn, schema, functionIndex, audit) {
  const argsType = renderArgsType(fn.argsSchema ?? {});
  const result = inferFunctionResultType(fn, schema, functionIndex);
  if (audit && result.source.startsWith("fallback")) {
    audit.push({ name: fn.name, source: result.source });
  }
  return `${helperName(fn.kind)}<${argsType}, ${result.type}>(${JSON.stringify(fn.name)}, ${JSON.stringify(fn.visibility)})`;
}

function helperName(kind) {
  switch (kind) {
    case "query":
      return "makeQueryReference";
    case "paginated_query":
      return "makePaginatedQueryReference";
    case "mutation":
      return "makeMutationReference";
    case "action":
      return "makeActionReference";
    default:
      throw new Error(`unknown convex function kind: ${kind}`);
  }
}

function buildFunctionIndex(modules) {
  const functionIndex = new Map();
  for (const moduleInfo of modules) {
    for (const fn of moduleInfo.functions) {
      functionIndex.set(fn.name, fn);
    }
  }
  return functionIndex;
}

export { buildFunctionIndex, helperCall, helperName };
