import fs from "node:fs/promises";
import path from "node:path";

import { evaluateCompileTimeExpressionSource } from "./compile_time_interpreter.mjs";
import { unsupportedError } from "./errors.mjs";
import { extractCallExpression } from "./syntax.mjs";

async function loadSchemaDefinition(convexDir) {
  const schemaPath = path.join(convexDir, "schema.ts");
  try {
    const source = await fs.readFile(schemaPath, "utf8");
    return parseSchemaSource(source, schemaPath);
  } catch (error) {
    if (error && typeof error === "object" && error.code === "ENOENT") {
      return { tables: {} };
    }
    throw error;
  }
}

function parseSchemaSource(source, filePath) {
  const exportMatch = /export\s+default\s+defineSchema\b/.exec(source);
  if (!exportMatch) {
    throw unsupportedError(filePath, "schema must use export default defineSchema(...)");
  }

  const callStart = exportMatch.index + exportMatch[0].lastIndexOf("defineSchema");
  const callExpression = extractCallExpression(source, callStart, filePath);
  const schema = evaluateSchemaExpression(callExpression, filePath);
  return sanitizeSchemaDefinition(schema, filePath);
}

function evaluateSchemaExpression(callExpression, filePath) {
  return evaluateCompileTimeExpressionSource(
    callExpression,
    {
      defineSchema: defineConvexSchema,
      defineTable: defineConvexTable,
      v: convexValidators,
    },
    filePath,
    "schema",
  );
}

function defineConvexSchema(tables) {
  return { tables };
}

function defineConvexTable(fields) {
  const indexes = [];
  const table = {
    kind: "table",
    fields,
    indexes,
    index(name, fieldNames) {
      indexes.push({
        name,
        fields: [...fieldNames],
      });
      return table;
    },
  };
  return table;
}

const convexValidators = {
  any() {
    return { kind: "any" };
  },
  null() {
    return { kind: "null" };
  },
  string() {
    return { kind: "string" };
  },
  number() {
    return { kind: "number" };
  },
  boolean() {
    return { kind: "boolean" };
  },
  id(tableName) {
    return { kind: "id", tableName };
  },
  literal(value) {
    return { kind: "literal", value };
  },
  array(element) {
    return { kind: "array", element };
  },
  object(fields) {
    return { kind: "object", fields };
  },
  optional(inner) {
    return { kind: "optional", inner };
  },
  union(...members) {
    return { kind: "union", members };
  },
};

function sanitizeSchemaDefinition(schema, filePath) {
  if (!schema || typeof schema !== "object" || Array.isArray(schema)) {
    throw unsupportedError(filePath, "schema must resolve to an object");
  }
  if (!schema.tables || typeof schema.tables !== "object" || Array.isArray(schema.tables)) {
    throw unsupportedError(filePath, "schema must contain a tables object");
  }

  const tables = {};
  for (const [tableName, table] of Object.entries(schema.tables)) {
    if (!table || typeof table !== "object" || table.kind !== "table") {
      throw unsupportedError(filePath, `schema table "${tableName}" must use defineTable`);
    }
    if (!table.fields || typeof table.fields !== "object" || Array.isArray(table.fields)) {
      throw unsupportedError(filePath, `schema table "${tableName}" must have fields`);
    }

    const fields = {};
    for (const [fieldName, validator] of Object.entries(table.fields)) {
      fields[fieldName] = sanitizeValidator(validator, filePath);
    }

    const seenIndexes = new Set();
    const indexes = (table.indexes ?? []).map((index) => {
      if (!index || typeof index !== "object") {
        throw unsupportedError(filePath, `schema table "${tableName}" has invalid index`);
      }
      if (typeof index.name !== "string" || index.name.length === 0) {
        throw unsupportedError(filePath, `schema table "${tableName}" has invalid index name`);
      }
      if (seenIndexes.has(index.name)) {
        throw unsupportedError(filePath, `schema table "${tableName}" has duplicate index "${index.name}"`);
      }
      seenIndexes.add(index.name);
      if (!Array.isArray(index.fields) || index.fields.length === 0) {
        throw unsupportedError(filePath, `schema index "${index.name}" must include fields`);
      }
      for (const fieldName of index.fields) {
        if (typeof fieldName !== "string" || !(fieldName in fields)) {
          throw unsupportedError(
            filePath,
            `schema index "${index.name}" references unknown field "${String(fieldName)}"`,
          );
        }
      }
      return {
        name: index.name,
        fields: [...index.fields],
      };
    });

    tables[tableName] = { fields, indexes };
  }

  return { tables };
}

function sanitizeValidator(validator, filePath) {
  if (!validator || typeof validator !== "object" || Array.isArray(validator)) {
    throw unsupportedError(
      filePath,
      "schema validators must be created with convex/values or neovex/values",
    );
  }

  switch (validator.kind) {
    case "any":
    case "null":
    case "string":
    case "number":
    case "boolean":
      return { kind: validator.kind };
    case "id":
      return {
        kind: "id",
        tableName:
          typeof validator.tableName === "string" ? validator.tableName : undefined,
      };
    case "literal":
      return {
        kind: "literal",
        value: sanitizeLiteralValue(validator.value, filePath),
      };
    case "array":
      return {
        kind: "array",
        element: sanitizeValidator(validator.element, filePath),
      };
    case "object": {
      if (!validator.fields || typeof validator.fields !== "object" || Array.isArray(validator.fields)) {
        throw unsupportedError(filePath, "object validator must contain fields");
      }
      const fields = {};
      for (const [fieldName, nested] of Object.entries(validator.fields)) {
        fields[fieldName] = sanitizeValidator(nested, filePath);
      }
      return { kind: "object", fields };
    }
    case "optional":
      return {
        kind: "optional",
        inner: sanitizeValidator(validator.inner, filePath),
      };
    case "union":
      if (!Array.isArray(validator.members) || validator.members.length === 0) {
        throw unsupportedError(filePath, "union validator must include members");
      }
      return {
        kind: "union",
        members: validator.members.map((member) => sanitizeValidator(member, filePath)),
      };
    default:
      throw unsupportedError(filePath, `unsupported schema validator "${String(validator.kind)}"`);
  }
}

function sanitizeLiteralValue(value, filePath) {
  if (
    value === null ||
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((entry) => sanitizeLiteralValue(entry, filePath));
  }
  if (value && typeof value === "object" && Object.getPrototypeOf(value) === Object.prototype) {
    const objectValue = {};
    for (const [key, nested] of Object.entries(value)) {
      objectValue[key] = sanitizeLiteralValue(nested, filePath);
    }
    return objectValue;
  }
  throw unsupportedError(filePath, "literal validator must use JSON-safe values");
}

export { convexValidators, loadSchemaDefinition, sanitizeValidator };
