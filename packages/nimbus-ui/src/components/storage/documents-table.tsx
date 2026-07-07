import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import type { DocumentJson, PageResponse } from "../../lib/types/table";
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
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex-1 overflow-auto">
        <table
          className="w-full border-collapse text-sm"
          data-testid="documents-table"
        >
          <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
            <tr>
              <Th className="w-8 px-2">
                <input
                  type="checkbox"
                  aria-label="Select all on page"
                  checked={
                    page.data.length > 0 &&
                    page.data.every((doc) =>
                      selected.has(String(doc._id ?? "")),
                    )
                  }
                  onChange={(e) => onToggleAll(e.target.checked)}
                  data-testid="documents-select-all"
                />
              </Th>
              {columns.map((col) => (
                <Th key={col}>{col}</Th>
              ))}
              <Th align="right">actions</Th>
            </tr>
          </thead>
          <tbody>
            {page.data.map((doc) => {
              const id = String(doc._id ?? "");
              return (
                <tr
                  key={id}
                  className="border-t border-app hover:bg-surface-2"
                  data-testid={`documents-row-${id}`}
                >
                  <Td className="w-8 px-2 align-top">
                    <input
                      type="checkbox"
                      aria-label={`Select document ${shortId(id)}`}
                      checked={selected.has(id)}
                      onChange={(e) => onToggleOne(id, e.target.checked)}
                      data-testid={`documents-select-${id}`}
                    />
                  </Td>
                  {columns.map((col) => (
                    <Td
                      key={col}
                      className="align-top font-mono text-xs text-default"
                    >
                      <CellValue value={doc[col]} field={col} id={id} />
                    </Td>
                  ))}
                  <Td align="right" className="align-top">
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
        <span>
          page {cursorStack.length} · {page.data.length} row
          {page.data.length === 1 ? "" : "s"}
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
              !page.has_more
                ? "text-muted"
                : "text-default hover:bg-surface",
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
