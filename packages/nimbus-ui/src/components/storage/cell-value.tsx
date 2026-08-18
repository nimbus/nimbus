import {
  formatAbsoluteTime,
  looksLikeEpochMs,
  shortId,
} from "../../lib/format";
import { CopyChip } from "../copy-chip";

// One document-table cell. The `_id` column renders a copyable short-id chip;
// primitives render inline; everything else falls back to a truncated JSON
// preview with the full value in the title attribute.
//
// Every variant is a *block* box with its own max-width. `truncate` is inert on
// an inline span — only `white-space: nowrap` applies, so the cell keeps its
// full intrinsic width and the table grows past its scroller instead of
// clipping. Constraining the box here is what makes the ellipsis real and what
// keeps the row height at one line.
const CELL_BOX = "block max-w-[38ch] truncate";

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
      <span className={CELL_BOX} title={value}>
        {value}
      </span>
    );
  }
  if (typeof value === "number") {
    // A schema-less browser cannot know a bare number is a wall-clock time, so
    // `looksLikeEpochMs` requires the field name and the value range to agree
    // before reformatting. The raw number stays in the title either way.
    if (looksLikeEpochMs(field, value)) {
      return (
        <span className={`${CELL_BOX} tabular`} title={String(value)}>
          {formatAbsoluteTime(value)}
        </span>
      );
    }
    return (
      <span className={`${CELL_BOX} tabular`} title={String(value)}>
        {value}
      </span>
    );
  }
  if (typeof value === "boolean") {
    return (
      <span className={value ? "text-default" : "text-muted"}>
        {String(value)}
      </span>
    );
  }
  const json = JSON.stringify(value);
  return (
    <span className={`${CELL_BOX} text-muted`} title={json}>
      {json}
    </span>
  );
}
