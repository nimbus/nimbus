import type { TableSchemaShape } from "../../lib/types/table";
import { DataTable, dataColumns } from "../data-table";

type IndexRow = NonNullable<TableSchemaShape["indexes"]>[number];

const column = dataColumns<IndexRow>();

const INDEX_COLUMNS = [
  column.accessor("name", {
    header: "Name",
    size: 200,
    cell: ({ getValue }) => (
      <span className="font-mono text-xs">{getValue()}</span>
    ),
  }),
  column.accessor((index) => index.fields.join(", "), {
    id: "fields",
    header: "Fields",
    size: 280,
    cell: ({ getValue }) => (
      <span className="font-mono text-xs">{getValue()}</span>
    ),
  }),
  column.accessor((index) => (index.unique ? "yes" : "no"), {
    id: "unique",
    header: "Unique",
    size: 90,
    cell: ({ getValue }) => <span className="text-text-3">{getValue()}</span>,
  }),
];

// The Indexes tab of a table: the indexes the schema declares, read-only.
// Index create and drop endpoints ship after the native index API lands.
export function IndexesTab({ schema }: { schema: TableSchemaShape | null }) {
  const indexes = schema?.indexes ?? [];
  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden"
      data-testid="documents-indexes-tab"
    >
      <p className="max-w-prose text-sm text-text-3">
        Read-only view derived from the table schema. Index REST endpoints
        (create/drop) ship after the native index API lands.
      </p>
      {indexes.length === 0 ? (
        <p
          className="rounded-md border border-border-1 bg-bg-panel px-3 py-8 text-center text-sm text-text-3"
          data-testid="documents-indexes-empty"
        >
          No indexes defined.
        </p>
      ) : (
        <div className="min-h-0 flex-1 overflow-hidden">
          <DataTable
            columns={INDEX_COLUMNS}
            data={indexes}
            getRowId={(index) => index.name}
            ariaLabel="Indexes"
            testid="documents-indexes-table"
            className="h-full"
          />
        </div>
      )}
    </div>
  );
}
