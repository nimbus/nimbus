// The runner pre-fills one field per argument of the function's validator.
// Two validator shapes reach the console: the Nimbus SDK records
// `{kind: "object", fields: {name: {kind: "string"}}}` (packages/nimbus/src/
// values.ts and the codegen planner), and a Convex-format bundle records
// `{type: "object", value: {name: {fieldType: {type: "string"}, optional}}}`.
// Both resolve to the same ArgField list; everything the parser does not
// understand degrades to a JSON field rather than to no form at all.

export type ArgInput = "text" | "number" | "boolean" | "json";

export type ArgField = {
  name: string;
  // type is the label the form shows next to the field, e.g. "string?".
  type: string;
  optional: boolean;
  input: ArgInput;
};

type Leaf = { type: string; input: ArgInput };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function leafOfKind(validator: Record<string, unknown>): Leaf {
  const kind = String(validator.kind ?? validator.type ?? "any");
  switch (kind) {
    case "string":
    case "id":
      return {
        type: kind === "id" ? `id<${validator.tableName ?? "?"}>` : kind,
        input: "text",
      };
    case "number":
    case "float64":
    case "int64":
      return { type: "number", input: "number" };
    case "boolean":
      return { type: "boolean", input: "boolean" };
    case "literal":
      return {
        type: `literal ${JSON.stringify(validator.value)}`,
        input: "json",
      };
    case "array":
      return { type: "array", input: "json" };
    case "object":
      return { type: "object", input: "json" };
    case "union":
      return { type: "union", input: "json" };
    case "null":
      return { type: "null", input: "json" };
    default:
      return { type: kind, input: "json" };
  }
}

/**
 * parseArgsValidator returns the argument fields of an object validator, or
 * null when the schema is absent or is not an object validator. Null tells
 * the runner to offer JSON mode only.
 */
export function parseArgsValidator(schema: unknown): ArgField[] | null {
  if (!isRecord(schema)) return null;

  // Nimbus SDK shape.
  if (schema.kind === "object" && isRecord(schema.fields)) {
    return Object.entries(schema.fields).map(([name, raw]) => {
      let validator = isRecord(raw) ? raw : {};
      let optional = false;
      if (validator.kind === "optional" && isRecord(validator.inner)) {
        optional = true;
        validator = validator.inner;
      }
      const leaf = leafOfKind(validator);
      return {
        name,
        type: optional ? `${leaf.type}?` : leaf.type,
        optional,
        input: leaf.input,
      };
    });
  }

  // Convex JSON validator shape.
  if (schema.type === "object" && isRecord(schema.value)) {
    return Object.entries(schema.value).map(([name, raw]) => {
      const entry = isRecord(raw) ? raw : {};
      const fieldType = isRecord(entry.fieldType) ? entry.fieldType : {};
      const optional = entry.optional === true;
      const leaf = leafOfKind(fieldType);
      return {
        name,
        type: optional ? `${leaf.type}?` : leaf.type,
        optional,
        input: leaf.input,
      };
    });
  }

  return null;
}

export type FieldValues = Record<string, string | boolean>;

export type BuildArgsResult =
  | { ok: true; args: Record<string, unknown> }
  | { ok: false; errors: Record<string, string> };

/**
 * buildArgs turns the form values into the argument object. Text is sent as
 * a string, number as a number, boolean as a boolean, and a JSON field is
 * parsed. An empty optional field is omitted; an empty required text field
 * is sent as "" because the server owns that validation and reports it
 * better than the console can.
 */
export function buildArgs(
  fields: ReadonlyArray<ArgField>,
  values: FieldValues,
): BuildArgsResult {
  const args: Record<string, unknown> = {};
  const errors: Record<string, string> = {};
  for (const field of fields) {
    const value = values[field.name];
    if (field.input === "boolean") {
      if (value === true) args[field.name] = true;
      else if (!field.optional) args[field.name] = false;
      continue;
    }
    const text = typeof value === "string" ? value : "";
    if (text.trim() === "") {
      if (!field.optional && field.input === "text") args[field.name] = "";
      continue;
    }
    if (field.input === "text") {
      args[field.name] = text;
      continue;
    }
    if (field.input === "number") {
      const num = Number(text);
      if (Number.isNaN(num)) {
        errors[field.name] = "Enter a number.";
        continue;
      }
      args[field.name] = num;
      continue;
    }
    try {
      args[field.name] = JSON.parse(text);
    } catch {
      errors[field.name] = "Enter valid JSON.";
    }
  }
  if (Object.keys(errors).length > 0) return { ok: false, errors };
  return { ok: true, args };
}

/**
 * valuesFromArgs is the reverse of buildArgs for the switch from JSON mode
 * back to form mode: each known field takes the value from the object when
 * it is there and its input can hold it.
 */
export function valuesFromArgs(
  fields: ReadonlyArray<ArgField>,
  args: Record<string, unknown>,
): FieldValues {
  const values: FieldValues = {};
  for (const field of fields) {
    const value = args[field.name];
    if (value === undefined) {
      values[field.name] = field.input === "boolean" ? false : "";
      continue;
    }
    if (field.input === "boolean") {
      values[field.name] = value === true;
    } else if (field.input === "text" && typeof value === "string") {
      values[field.name] = value;
    } else if (field.input === "number" && typeof value === "number") {
      values[field.name] = String(value);
    } else {
      values[field.name] = JSON.stringify(value);
    }
  }
  return values;
}
