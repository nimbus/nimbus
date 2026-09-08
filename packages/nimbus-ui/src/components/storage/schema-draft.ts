import type { TableSchemaShape } from "../../lib/types/table";

/** Every type `nimbus_core::FieldType` accepts, as it serializes. */
export const FIELD_TYPES = [
  "string",
  "number",
  "boolean",
  "array",
  "object",
  "any",
] as const;

export type SchemaDraftResult =
  | { ok: true; schema: TableSchemaShape }
  | { ok: false; error: string };

/**
 * Reads the schema editor's draft into the shape the apply route accepts.
 *
 * The server validates the same things, but its answer names a serde path,
 * not the field the operator typed. Checking the shape here turns "invalid
 * type: string, expected a boolean at line 6" into "field 'age': required
 * must be true or false", and it never sends a draft the server would refuse
 * on shape alone. The document scan stays server-side: that is the check
 * the console cannot make.
 */
export function parseSchemaDraft(
  json: string,
  table: string,
): SchemaDraftResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch (err) {
    return { ok: false, error: `Invalid JSON: ${(err as Error).message}` };
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { ok: false, error: "Schema must be a JSON object" };
  }
  const draft = parsed as Record<string, unknown>;

  // The route checks the name against the path; an omitted name means the
  // table the editor is open on.
  const name = draft.table ?? table;
  if (name !== table) {
    return {
      ok: false,
      error: `"table" must be "${table}" (the table this editor is open on), got ${JSON.stringify(name)}`,
    };
  }

  const fields = draft.fields ?? [];
  if (!Array.isArray(fields)) {
    return { ok: false, error: '"fields" must be an array' };
  }
  const seenFields = new Set<string>();
  const parsedFields: NonNullable<TableSchemaShape["fields"]> = [];
  for (const [position, entry] of fields.entries()) {
    const where = `fields[${position}]`;
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
      return { ok: false, error: `${where} must be an object` };
    }
    const field = entry as Record<string, unknown>;
    if (typeof field.name !== "string" || field.name === "") {
      return {
        ok: false,
        error: `${where}: "name" must be a non-empty string`,
      };
    }
    if (seenFields.has(field.name)) {
      return { ok: false, error: `field '${field.name}' is listed twice` };
    }
    seenFields.add(field.name);
    const fieldType = field.field_type ?? "any";
    if (!FIELD_TYPES.includes(fieldType as (typeof FIELD_TYPES)[number])) {
      return {
        ok: false,
        error: `field '${field.name}': "field_type" must be one of ${FIELD_TYPES.join(", ")}, got ${JSON.stringify(fieldType)}`,
      };
    }
    const required = field.required ?? false;
    if (typeof required !== "boolean") {
      return {
        ok: false,
        error: `field '${field.name}': "required" must be true or false`,
      };
    }
    parsedFields.push({
      name: field.name,
      field_type: fieldType as string,
      required,
    });
  }

  const indexes = draft.indexes ?? [];
  if (!Array.isArray(indexes)) {
    return { ok: false, error: '"indexes" must be an array' };
  }
  const seenIndexes = new Set<string>();
  const parsedIndexes: NonNullable<TableSchemaShape["indexes"]> = [];
  for (const [position, entry] of indexes.entries()) {
    const where = `indexes[${position}]`;
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) {
      return { ok: false, error: `${where} must be an object` };
    }
    const index = entry as Record<string, unknown>;
    if (typeof index.name !== "string" || index.name === "") {
      return {
        ok: false,
        error: `${where}: "name" must be a non-empty string`,
      };
    }
    if (seenIndexes.has(index.name)) {
      return { ok: false, error: `index '${index.name}' is listed twice` };
    }
    seenIndexes.add(index.name);
    if (
      !Array.isArray(index.fields) ||
      index.fields.length === 0 ||
      !index.fields.every((f) => typeof f === "string" && f !== "")
    ) {
      return {
        ok: false,
        error: `index '${index.name}': "fields" must list at least one field name`,
      };
    }
    // The id and state are the server's. A draft that carries them back
    // unchanged is fine; a draft that invents them is not, so only the id
    // of a known index travels.
    parsedIndexes.push({
      ...(typeof index.id === "string" ? { id: index.id } : {}),
      name: index.name,
      fields: index.fields as string[],
    });
  }

  return {
    ok: true,
    schema: { table, fields: parsedFields, indexes: parsedIndexes },
  };
}

/**
 * The editor's draft for a schema: the server's copy without the fields it
 * owns. Index ids and states are server-assigned, so they are noise in an
 * editor and wrong the moment the operator copies a row to make a new one.
 */
export function draftFromSchema(
  schema: TableSchemaShape | null,
  table: string,
): string {
  const draft: TableSchemaShape = {
    table,
    fields: (schema?.fields ?? []).map((field) => ({
      name: field.name,
      field_type: field.field_type ?? "any",
      required: field.required ?? false,
    })),
    indexes: (schema?.indexes ?? []).map((index) => ({
      name: index.name,
      fields: index.fields,
    })),
  };
  return JSON.stringify(draft, null, 2);
}
