import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import type { DocumentJson, PageResponse } from "../../lib/types/table";
import { Checkbox } from "../checkbox";
import { Td, Th } from "../data-table";
import { CellValue } from "./cell-value";

// The scrollable document grid plus its cursor pager. Purely presentational:
// selection, editing, and deletion are lifted to callbacks so the page
// component keeps ownership of that state.
export function DocumentsTable({
  page,
  columns,
  selected,
  cursorStack,
  onToggleAll,
  onToggleOne,
  onEdit,
  onDelete,
  onPrev,
  onNext,
}: {
  page: PageResponse;
  columns: string[];
  selected: Set<string>;
  cursorStack: Array<string | null>;
  onToggleAll: (checked: boolean) => void;
  onToggleOne: (id: string, checked: boolean) => void;
  onEdit: (doc: DocumentJson) => void;
  onDelete: (ids: string[]) => void;
  onPrev: () => void;
  onNext: () => void;
}) {
  // A document table is wider than any window once a table has more than a
  // handful of fields, so horizontal scrolling is expected. What is not
  // acceptable is losing the row's identity and its actions to that scroll —
  // DESIGN.md requires inline actions to stay reachable. Pin the identity
  // columns to the left edge and the actions column to the right, so the two
  // things you need in order to act on a row are always on screen.
  //
  // `--row-bg` carries the row's own background onto the pinned cells; without
  // it the scrolled columns would show straight through them.
  const PIN_L = "sticky z-10 bg-[var(--row-bg)]";
  const PIN_R = "sticky right-0 z-10 bg-[var(--row-bg)]";

  const pageIds = page.data.map((doc) => String(doc._id ?? ""));
  const selectedOnPage = pageIds.filter((id) => selected.has(id)).length;
  const allSelected = pageIds.length > 0 && selectedOnPage === pageIds.length;

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex-1 overflow-auto">
        <table
          className="w-full border-separate border-spacing-0 text-base"
          data-testid="documents-table"
        >
          <thead className="sticky top-0 z-20 [--row-bg:var(--nimbus-surface-2)] text-[10px] uppercase tracking-[0.14em] text-muted">
            <tr className="bg-surface-2">
              <Th className={cn("left-0 w-px px-3", PIN_L)}>
                <Checkbox
                  label="Select all on page"
                  checked={allSelected}
                  indeterminate={selectedOnPage > 0}
                  onChange={onToggleAll}
                  testid="documents-select-all"
                />
              </Th>
              {columns.map((col, i) => (
                <Th
                  key={col}
                  className={
                    i === 0
                      ? cn("left-[38px] border-r border-app", PIN_L)
                      : undefined
                  }
                >
                  {col}
                </Th>
              ))}
              <Th
                align="right"
                className={cn("w-px border-l border-app", PIN_R)}
              >
                actions
              </Th>
            </tr>
          </thead>
          <tbody>
            {page.data.map((doc) => {
              const id = String(doc._id ?? "");
              return (
                <tr
                  key={id}
                  className={cn(
                    "group h-9 [&>td]:border-t [&>td]:border-app",
                    selected.has(id)
                      ? "bg-surface-2 [--row-bg:var(--nimbus-surface-2)]"
                      : "[--row-bg:var(--nimbus-surface)] hover:bg-surface-2 hover:[--row-bg:var(--nimbus-surface-2)]",
                  )}
                  data-testid={`documents-row-${id}`}
                >
                  <Td className={cn("left-0 w-px px-3", PIN_L)}>
                    <Checkbox
                      label={`Select document ${shortId(id)}`}
                      checked={selected.has(id)}
                      onChange={(checked) => onToggleOne(id, checked)}
                      testid={`documents-select-${id}`}
                    />
                  </Td>
                  {columns.map((col, i) => (
                    <Td
                      key={col}
                      className={cn(
                        "font-mono text-default",
                        i === 0 && cn("left-[38px] border-r border-app", PIN_L),
                      )}
                    >
                      <CellValue value={doc[col]} field={col} id={id} />
                    </Td>
                  ))}
                  {/* Inline actions appear on hover and stay keyboard
                      reachable — `opacity` keeps them in the tab order, and
                      `focus-within` reveals them when tabbed to. */}
                  <Td
                    align="right"
                    className={cn(
                      "w-px whitespace-nowrap border-l border-app",
                      "opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100",
                      PIN_R,
                    )}
                  >
                    <button
                      type="button"
                      onClick={() => onEdit(doc)}
                      className="mr-2 rounded border border-app px-2 py-0.5 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
                      data-testid={`documents-edit-${id}`}
                    >
                      edit
                    </button>
                    <button
                      type="button"
                      onClick={() => onDelete([id])}
                      className="rounded border border-app px-2 py-0.5 font-mono text-[11px] uppercase tracking-wide text-danger hover:bg-surface"
                      data-testid={`documents-delete-${id}`}
                    >
                      delete
                    </button>
                  </Td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <div
        className="flex items-center justify-between border-t border-app bg-surface-2 px-3 py-2 font-mono text-[11px] text-muted"
        data-testid="documents-pagination"
      >
        <span className="tabular">
          page {cursorStack.length} · {page.data.length} row
          {page.data.length === 1 ? "" : "s"}
          {selectedOnPage > 0 ? ` · ${selectedOnPage} selected` : ""}
        </span>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onPrev}
            disabled={cursorStack.length <= 1}
            className={cn(
              "rounded border border-app px-2 py-0.5 uppercase tracking-wide",
              cursorStack.length <= 1
                ? "text-muted"
                : "text-default hover:bg-surface",
            )}
            data-testid="documents-prev-page"
          >
            prev
          </button>
          <button
            type="button"
            onClick={onNext}
            disabled={!page.has_more}
            className={cn(
              "rounded border border-app px-2 py-0.5 uppercase tracking-wide",
              !page.has_more ? "text-muted" : "text-default hover:bg-surface",
            )}
            data-testid="documents-next-page"
          >
            next
          </button>
        </div>
      </div>
    </div>
  );
}
