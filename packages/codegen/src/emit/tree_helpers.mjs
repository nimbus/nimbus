function ensureModuleTree(tree, moduleName) {
  const parts = moduleName.split(".");
  let node = tree;
  for (const part of parts) {
    node[part] ??= {};
    node = node[part];
  }
  return node;
}

function renderTree(node, depth) {
  const indent = "  ".repeat(depth);
  const entries = Object.entries(node);
  if (entries.length === 0) {
    return "{}";
  }
  const lines = entries.map(([key, value]) => {
    if (typeof value === "string") {
      return `${indent}  ${key}: ${value}`;
    }
    return `${indent}  ${key}: ${renderTree(value, depth + 1)}`;
  });
  return `{\n${lines.join(",\n")}\n${indent}}`;
}

export { ensureModuleTree, renderTree };
