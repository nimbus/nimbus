export type TableSchemaField = {
  name: string;
  field_type?: string;
  required?: boolean;
};

/** Every state `nimbus_core::IndexState` serializes. */
export const INDEX_STATES = [
  "pending",
  "backfilling",
  "enabled",
  "deleting",
] as const;
export type IndexState = (typeof INDEX_STATES)[number];

// Mirrors `nimbus_core::IndexDefinition`. The id and the state are
// server-assigned: a schema the console sends omits both, and the server
// keeps the id of an index whose name and fields did not change.
export type TableSchemaIndex = {
  id?: string;
  name: string;
  fields: string[];
  state?: IndexState | string;
};

export type TableSchemaShape = {
  table?: string;
  fields?: TableSchemaField[];
  indexes?: TableSchemaIndex[];
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
