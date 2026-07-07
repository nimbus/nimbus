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

// A single document as returned by the paginated query endpoint. Widens to an
// arbitrary field bag with the reserved system columns pulled out by name.
export type DocumentJson = Record<string, unknown> & {
  _id?: string;
  _creationTime?: number;
  _updateTime?: number;
};

// One page of the tenant `query/paginated` response: the documents plus the
// opaque cursor and a has-more flag that drive the storage-table pager.
export type PageResponse = {
  data: DocumentJson[];
  next_cursor: string | null;
  has_more: boolean;
};
