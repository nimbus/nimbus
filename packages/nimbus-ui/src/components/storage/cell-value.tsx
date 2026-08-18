import { cn } from "../../lib/cn";
import {
  formatAbsoluteTime,
  looksLikeEpochMs,
  shortId,
} from "../../lib/format";
import { CopyChip } from "../copy-chip";

// One document-table cell. The `_id` column renders a copyable short-id chip;
// primitives render inline; containers render as a typed chip.
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
  onExpand,
}: {
  value: unknown;
  field: string;
  id: string;
  /** Opens the whole-document drawer. Turns container chips into buttons. */
  onExpand?: () => void;
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
  // A document browser's job is type fidelity, so the three "there is nothing
  // here" cases must not collapse into one glyph. `—` means the field is
  // *absent from this document*; `null` and `""` are values the document
  // actually holds, and telling them apart is a routine schema question.
  if (value === undefined) {
    return (
      <span className="text-muted" title="field not present in this document">
        —
      </span>
    );
  }
  if (value === null) {
    return (
      <span className="italic text-muted" title="null">
        null
      </span>
    );
  }
  if (typeof value === "string") {
    if (value === "") {
      return (
        <span className="text-muted" title="empty string">
          &quot;&quot;
        </span>
      );
    }
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
  return <ContainerChip value={value} onExpand={onExpand} />;
}

// Objects and arrays used to render as `JSON.stringify(...)` cut at 60
// characters — an object and an array looked alike, the cut landed mid-token
// with no signal that anything was removed, and the muted colour made real
// data read as the placeholder used for absent data. A typed summary states
// the shape instead of previewing an arbitrary prefix of it.
function ContainerChip({
  value,
  onExpand,
}: {
  value: unknown;
  onExpand?: () => void;
}) {
  const json = safeStringify(value);
  const summary = describeContainer(value);
  const className = cn(
    "inline-flex max-w-[38ch] items-baseline gap-1 truncate rounded border border-app px-1.5 text-xs text-default",
    onExpand && "cursor-pointer hover:bg-surface-2",
  );
  if (!onExpand) {
    return (
      <span className={className} title={json}>
        {summary}
      </span>
    );
  }
  return (
    <button type="button" className={className} title={json} onClick={onExpand}>
      {summary}
    </button>
  );
}

function describeContainer(value: unknown): string {
  if (Array.isArray(value)) {
    if (value.length === 0) return "[]";
    return `[…] ${value.length} item${value.length === 1 ? "" : "s"}`;
  }
  const keys = Object.keys(value as Record<string, unknown>);
  if (keys.length === 0) return "{}";
  return `{…} ${keys.length} key${keys.length === 1 ? "" : "s"}`;
}

function safeStringify(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? String(value);
  } catch {
    return String(value);
  }
}
