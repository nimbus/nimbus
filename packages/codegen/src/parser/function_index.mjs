function buildFunctionIndex(modules) {
  const functionIndex = new Map();
  for (const moduleInfo of modules) {
    for (const fn of moduleInfo.functions) {
      functionIndex.set(fn.name, fn);
    }
  }
  return functionIndex;
}

export { buildFunctionIndex };
