function renderTableNames(tableNames) {
  if (tableNames.length === 0) {
    return "never";
  }
  return tableNames.map((tableName) => JSON.stringify(tableName)).join(" | ");
}

function renderDocumentType(tableName, fields, depth) {
  const indent = "  ".repeat(depth);
  const lines = [
    `${indent}_id: Id<${JSON.stringify(tableName)}>;`,
    `${indent}_creationTime: number;`,
    `${indent}_updateTime: number;`,
  ];
  for (const [fieldName, validator] of Object.entries(fields)) {
    lines.push(`${indent}${JSON.stringify(fieldName)}: ${renderValidatorType(validator)};`);
  }
  return `{\n${lines.join("\n")}\n${"  ".repeat(depth - 1)}}`;
}

function renderIndexUnion(indexes) {
  if (indexes.length === 0) {
    return "never";
  }
  return indexes.map((index) => JSON.stringify(index.name)).join(" | ");
}

function renderObjectBlock(entries) {
  if (entries.length === 0) {
    return "{}";
  }
  return `{\n${entries.join("\n")}\n}`;
}

function isTrivialValidator(validator) {
  if (!validator || typeof validator !== "object") {
    return true;
  }
  switch (validator.kind) {
    case "any":
      return true;
    case "array":
      return isTrivialValidator(validator.element);
    case "optional":
      return isTrivialValidator(validator.inner);
    case "union":
      // A union is "trivial" when at least one member is itself trivial —
      // the trivial member (e.g. `v.any()`) widens the whole union to
      // `JsonValue`, so the validator provides no real type information.
      // This intentionally captures `union(v.any(), v.null())`
      // (system:status's textbook shape) and similar `any | X` shapes.
      return (
        Array.isArray(validator.members) &&
        validator.members.length > 0 &&
        validator.members.some(isTrivialValidator)
      );
    default:
      return false;
  }
}

function renderValidatorType(validator, options = { idSymbol: "GenericId" }) {
  switch (validator.kind) {
    case "any":
      return "JsonValue";
    case "null":
      return "null";
    case "string":
      return "string";
    case "number":
      return "number";
    case "boolean":
      return "boolean";
    case "id":
      return `${options.idSymbol}<${JSON.stringify(validator.tableName ?? "unknown")}>`;
    case "literal":
      return JSON.stringify(validator.value);
    case "array":
      return `(${renderValidatorType(validator.element, options)})[]`;
    case "object": {
      const lines = Object.entries(validator.fields).map(
        ([fieldName, nested]) =>
          `  ${JSON.stringify(fieldName)}: ${renderValidatorType(nested, options)};`,
      );
      return `{\n${lines.join("\n")}\n}`;
    }
    case "optional":
      return `${renderValidatorType(validator.inner, options)} | undefined`;
    case "union":
      return validator.members.map((member) => renderValidatorType(member, options)).join(" | ");
    default:
      throw new Error(`unknown schema validator kind: ${validator.kind}`);
  }
}

export {
  isTrivialValidator,
  renderDocumentType,
  renderIndexUnion,
  renderObjectBlock,
  renderTableNames,
  renderValidatorType,
};
