import { shortId } from "../../lib/format";
import { CopyChip } from "../copy-chip";

// One document-table cell. The `_id` column renders a copyable short-id chip;
// primitives render inline; everything else falls back to a truncated JSON
// preview with the full value in the title attribute.
export function CellValue({
  value,
  field,
  id,
}: {
  value: unknown;
  field: string;
  id: string;
}) {
  if (field === "_id" && id) {
    return (
      <CopyChip
        label="document id"
        value={id}
        testid={`documents-cell-id-${id}`}
      >
        {shortId(id)}
      </CopyChip>
    );
  }
  if (value === undefined || value === null) {
    return <span className="text-muted">—</span>;
  }
  if (typeof value === "string") {
    return (
      <span className="truncate" title={value}>
        {value}
      </span>
    );
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return <span>{String(value)}</span>;
  }
  return (
    <span className="truncate text-muted" title={JSON.stringify(value)}>
      {JSON.stringify(value).slice(0, 60)}
    </span>
  );
}
