export type TableSchemaField = {
  name: string;
  field_type?: string;
  required?: boolean;
};

export type TableSchemaShape = {
  table?: string;
  fields?: TableSchemaField[];
  indexes?: Array<{
    name: string;
    fields: string[];
    unique?: boolean;
    type?: string;
  }>;
};

// UI view of a `tables` row as returned by `api.tables.list` — the superset of
// fields the three storage/tenant routes read. Kept hand-rolled rather than
// aliased to `Doc<"tables">` because the list endpoint widens `_id` to a plain
// string and drops the generated required fields the console never consumes.
export type TableDoc = {
  _id: string;
  tenantId?: string;
  name?: string;
  schema?: TableSchemaShape | null;
  rowCount?: number;
  lastWriteAt?: number;
};
