import { inferFunctionResultType, renderArgsType } from "./type_inference.mjs";

function helperCall(fn, schema, functionIndex) {
  const argsType = renderArgsType(fn.argsSchema ?? {});
  const resultType = inferFunctionResultType(fn, schema, functionIndex);
  return `${helperName(fn.kind)}<${argsType}, ${resultType}>(${JSON.stringify(fn.name)}, ${JSON.stringify(fn.visibility)})`;
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
