function ensureModuleTree(tree, moduleName) {
  const parts = moduleName.split(".");
  let node = tree;
  for (const part of parts) {
    if (!Object.hasOwn(node, part)) {
      setTreeValue(node, part, Object.create(null));
    }
    node = node[part];
  }
  return node;
}

function setTreeValue(node, key, value) {
  Object.defineProperty(node, key, {
    value,
    enumerable: true,
    configurable: true,
    writable: true,
  });
}

function renderTree(node, depth) {
  const indent = "  ".repeat(depth);
  const entries = Object.entries(node);
  if (entries.length === 0) {
    return "{}";
  }
  const lines = entries.map(([key, value]) => {
    const renderedKey = renderPropertyKey(key);
    if (typeof value === "string") {
      return `${indent}  ${renderedKey}: ${value}`;
    }
    return `${indent}  ${renderedKey}: ${renderTree(value, depth + 1)}`;
  });
  return `{\n${lines.join(",\n")}\n${indent}}`;
}

function renderPropertyKey(key) {
  if (key === "__proto__") {
    return `[${JSON.stringify(key)}]`;
  }
  return /^[A-Za-z_$][A-Za-z0-9_$]*$/u.test(key) ? key : JSON.stringify(key);
}

export { ensureModuleTree, renderTree, setTreeValue };
