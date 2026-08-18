import {
  type DocumentFilter,
  type DocumentOrder,
  FILTER_OPS,
  type FilterOp,
} from "../../lib/api-mutations";
import type { TableSchemaShape } from "../../lib/types/table";

/**
 * The document browser's query model.
 *
 * The wire shapes themselves live with the request that carries them
 * (`lib/api-mutations.ts`), where they mirror `nimbus_core::query::{Filter,
 * FilterOp, OrderBy, OrderDirection}` field for field. This module re-exports
 * them beside the parsing and labelling the query bar needs, so one definition
 * serves both the URL and the request body.
 */
export { FILTER_OPS, type FilterOp };
export type { DocumentFilter, DocumentOrder };

export const FILTER_OP_LABEL: Record<FilterOp, string> = {
  eq: "=",
  neq: "≠",
  gt: ">",
  gte: "≥",
  lt: "<",
  lte: "≤",
};

export function isFilterOp(value: unknown): value is FilterOp {
  return FILTER_OPS.includes(value as FilterOp);
}

// URL is state (DESIGN.md §Cross-Screen Rules), so filters and sort live in the
// search params and survive a refresh or a shared link. Both parsers are total:
// a hand-edited URL degrades to "no filter" rather than throwing inside
// `validateSearch`, which would blank the route.
export function parseFilters(raw: unknown): DocumentFilter[] {
  if (!Array.isArray(raw)) return [];
  const out: DocumentFilter[] = [];
  for (const entry of raw) {
    if (!entry || typeof entry !== "object") continue;
    const { field, op, value } = entry as Record<string, unknown>;
    if (typeof field !== "string" || field === "") continue;
    if (!isFilterOp(op)) continue;
    out.push({ field, op, value });
  }
  return out;
}

export function parseOrder(sort: unknown, dir: unknown): DocumentOrder | null {
  if (typeof sort !== "string" || sort === "") return null;
  return { field: sort, direction: dir === "desc" ? "desc" : "asc" };
}

/**
 * Read a filter value typed by hand. Documents are JSON, so a filter on a
 * numeric or boolean field must send a number or a boolean — sending the string
 * `"42"` silently matches nothing. Quoted input forces the string reading for
 * the case where a field really does hold `"42"`.
 */
export function parseFilterValue(raw: string): unknown {
  const text = raw.trim();
  if (text === "") return "";
  if (text === "true") return true;
  if (text === "false") return false;
  if (text === "null") return null;
  if (/^-?\d+(\.\d+)?([eE][-+]?\d+)?$/.test(text)) return Number(text);
  if (
    (text.startsWith('"') && text.endsWith('"')) ||
    text.startsWith("[") ||
    text.startsWith("{")
  ) {
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  }
  return text;
}

export function formatFilterValue(value: unknown): string {
  if (typeof value === "string") return value === "" ? '""' : value;
  return JSON.stringify(value) ?? String(value);
}

export function describeFilter(filter: DocumentFilter): string {
  return `${filter.field} ${FILTER_OP_LABEL[filter.op]} ${formatFilterValue(filter.value)}`;
}

/**
 * Fields a sort can use without scanning the table.
 *
 * The engine's `sort_documents` runs over the whole filtered set in memory, so
 * ordering by an unindexed field is an unbounded scan. DESIGN.md:269 requires
 * the browser to make index use visible and to refuse unbounded scans, so the
 * header only offers a one-click sort on these; everything else needs an
 * explicit "scan anyway".
 *
 * Only an index's *leading* field is cheap: a composite index on `(a, b)` does
 * not order by `b`.
 */
export function indexBackedFields(
  schema: TableSchemaShape | null,
): Set<string> {
  // `_id` is the primary key and the table's natural order.
  const fields = new Set<string>(["_id"]);
  for (const index of schema?.indexes ?? []) {
    const leading = index.fields?.[0];
    if (leading) fields.add(leading);
  }
  return fields;
}
