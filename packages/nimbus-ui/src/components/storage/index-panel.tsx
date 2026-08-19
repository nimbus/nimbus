import type { TableSchemaShape } from "../../lib/types/table";
import { PanelHeader } from "../slideover";

// Read-only side panel listing the indexes derived from the table schema.
// Index create/drop endpoints ship once the native index API lands.
export function IndexPanel({
  schema,
  onClose,
}: {
  schema: TableSchemaShape | null;
  onClose: () => void;
}) {
  const indexes = schema?.indexes ?? [];
  return (
    <aside
      // 420px is the preferred width, not a floor. With `shrink-0` it was both,
      // so in a window under ~564px the panel kept all 420px and the row's
      // `overflow-hidden` cut off its right edge — the close button included —
      // leaving no way to dismiss it. `min-w-0` lets the panel go below its
      // min-content width so it stays whole and closeable at any width. It
      // spans the row until the row can afford 320 + 16 gap + 420 = 756px,
      // because the table's `min-w-[20rem]` floor leaves an inspector beside
      // it only 6px at a 390px viewport, and the row stacks the two below
      // that. The schema panel it alternates with in the same toolbar carries
      // the identical pair of constraints; `storage_.$table.tsx` owns the
      // `documents-row` container this query is measured against, and with no
      // such container in scope the rule never matches and `w-full` stands.
      className="@min-[756px]/documents-row:w-[420px] flex w-full min-w-0 flex-col overflow-hidden rounded-md border border-app bg-surface"
      data-testid="documents-indexes-panel"
    >
      <PanelHeader title="Indexes" onClose={onClose} />
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto p-3">
        <p className="font-mono text-xs text-muted">
          Read-only view derived from the table schema. Index REST endpoints
          (create/drop) ship after the native index API lands.
        </p>
        {indexes.length === 0 ? (
          <p
            className="font-mono text-xs text-muted"
            data-testid="documents-indexes-empty"
          >
            No indexes defined.
          </p>
        ) : (
          <table
            className="w-full border-collapse text-xs"
            data-testid="documents-indexes-table"
          >
            <thead className="text-xs uppercase tracking-wide text-muted">
              <tr>
                <th className="px-2 py-1 text-left">Name</th>
                <th className="px-2 py-1 text-left">Fields</th>
                <th className="px-2 py-1 text-left">Unique</th>
              </tr>
            </thead>
            <tbody>
              {indexes.map((idx) => (
                <tr key={idx.name} className="border-t border-app">
                  <td className="px-2 py-1 font-mono text-default">
                    {idx.name}
                  </td>
                  <td className="px-2 py-1 font-mono text-default">
                    {idx.fields.join(", ")}
                  </td>
                  <td className="px-2 py-1 font-mono text-muted">
                    {idx.unique ? "yes" : "no"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </aside>
  );
}
