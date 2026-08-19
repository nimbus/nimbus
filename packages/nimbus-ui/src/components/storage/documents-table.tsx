import { ArrowDown, ArrowUp, ChevronsUpDown } from "lucide-react";
import { type MouseEvent as ReactMouseEvent, useRef, useState } from "react";
import { toast } from "sonner";

import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import type { DocumentJson, PageResponse } from "../../lib/types/table";
import { Checkbox } from "../checkbox";
import { PIN_L, PIN_R, Td, Th } from "../data-table";
import { CellValue } from "./cell-value";
import { RowContextMenu, type RowMenuItem } from "./row-context-menu";
import type { DocumentOrder } from "./table-query";

// Anything that handles its own click. A row click must not hijack the
// checkbox, the `_id` copy chip, a container-value chip, or the row's own
// action buttons — all of which sit inside the row.
const INTERACTIVE = "button, a, input, label, [role='menuitem']";

/** Rows shown while a page is in flight, matching PAGE_SIZE. */
const SKELETON_ROWS = 25;

type MenuState = {
  x: number;
  y: number;
  doc: DocumentJson;
  anchor: HTMLElement | null;
};

// The scrollable document grid plus its cursor pager. Purely presentational:
// selection, editing, deletion, and the query live with the page component, so
// this file owns only the grid's own interaction surface — sort clicks, row
// activation, the context menu, and roving focus.
export function DocumentsTable({
  page,
  columns,
  selected,
  pageNumber,
  loading,
  order,
  indexBacked,
  onSort,
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
  pageNumber: number;
  /** A page is in flight: show skeleton rows rather than another page's data. */
  loading: boolean;
  order: DocumentOrder | null;
  indexBacked: Set<string>;
  onSort: (field: string) => void;
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

  const pageIds = page.data.map((doc) => String(doc._id ?? ""));
  const selectedOnPage = pageIds.filter((id) => selected.has(id)).length;
  const allSelected = pageIds.length > 0 && selectedOnPage === pageIds.length;

  const [menu, setMenu] = useState<MenuState | null>(null);
  // ARIA grid roving tabindex: one row is in the tab order at a time. Giving
  // all 25 rows `tabIndex={0}` would inject 25 stops into the page's tab order,
  // which is worse for keyboard users than having none.
  const [focusRow, setFocusRow] = useState(0);
  const rowRefs = useRef<Array<HTMLTableRowElement | null>>([]);

  const moveFocus = (from: number, delta: number) => {
    const next = Math.min(Math.max(from + delta, 0), page.data.length - 1);
    setFocusRow(next);
    rowRefs.current[next]?.focus();
  };

  const openMenuForRow = (index: number, doc: DocumentJson) => {
    const el = rowRefs.current[index];
    const rect = el?.getBoundingClientRect();
    // A keyboard-raised menu has no pointer position — `clientX`/`clientY` are
    // 0,0 — so anchor it to the focused row's box instead.
    setMenu({
      x: rect ? rect.left + 24 : 0,
      y: rect ? rect.bottom : 0,
      doc,
      anchor: el ?? null,
    });
  };

  const menuItems = (doc: DocumentJson): RowMenuItem[] => {
    const id = String(doc._id ?? "");
    return [
      { id: "edit", label: "Edit document", onSelect: () => onEdit(doc) },
      {
        id: "copy-id",
        label: "Copy _id",
        hint: shortId(id, 10),
        onSelect: () => void copyText(id, "document id"),
      },
      {
        id: "copy-json",
        label: "Copy document JSON",
        onSelect: () =>
          void copyText(JSON.stringify(doc, null, 2), "document JSON"),
      },
      {
        id: "delete",
        label: "Delete document",
        danger: true,
        onSelect: () => onDelete([id]),
      },
    ];
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <div className="flex-1 overflow-auto">
        <table
          className="w-full border-separate border-spacing-0 text-base"
          data-testid="documents-table"
        >
          <thead className="sticky top-0 z-20 [--row-bg:var(--nimbus-surface-2)] text-xs uppercase tracking-[0.14em] text-muted">
            <tr className="bg-surface-2">
              <Th className={cn("left-0 w-px px-3", PIN_L)}>
                <Checkbox
                  label="Select all on page"
                  checked={!loading && allSelected}
                  indeterminate={!loading && selectedOnPage > 0}
                  // While a page is in flight the rows on screen are
                  // placeholders: a select-all here would select the documents
                  // of the page being replaced.
                  onChange={(checked) => {
                    if (!loading) onToggleAll(checked);
                  }}
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
                  <SortHeader
                    field={col}
                    order={order}
                    indexed={indexBacked.has(col)}
                    onSort={onSort}
                  />
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
            {loading ? (
              <SkeletonRows
                rows={page.data.length > 0 ? page.data.length : SKELETON_ROWS}
                columns={columns}
              />
            ) : null}
            {loading
              ? null
              : page.data.map((doc, index) => {
                  const id = String(doc._id ?? "");
                  const isSelected = selected.has(id);
                  return (
                    <tr
                      key={id}
                      ref={(el) => {
                        rowRefs.current[index] = el;
                      }}
                      tabIndex={index === focusRow ? 0 : -1}
                      aria-selected={isSelected}
                      data-selected={isSelected || undefined}
                      className={cn(
                        // No `outline-none`: the row keeps the console-wide
                        // `:focus-visible` outline — 2px of `--focus` at
                        // offset 2px — which is the only ring in the system
                        // tuned to clear WCAG 2.2 SC 1.4.11's 3:1 non-text
                        // floor on every ground (3.42:1 warm light on
                        // `--surface-2`, its worst case). The inset `--accent`
                        // hairline it replaces measured 1.71:1 there, and read
                        // worse than that: `PIN_L`/`PIN_R` are `z-10` over an
                        // opaque `--row-bg`, so they covered the ring's left
                        // and right segments outright.
                        "group h-9 cursor-pointer [&>td]:border-t [&>td]:border-app",
                        // Same reason the row has to outrank those pinned
                        // cells: at offset 2px the outline is drawn inside the
                        // neighbouring rows' band, so their `z-10` cells eat
                        // the segment behind them — 155px of this row at the
                        // default column widths. `15`, not `20` — the sticky
                        // header is `z-20` and equal z-index resolves by tree
                        // order, so a `z-20` row would slide over the header
                        // as it scrolls under it.
                        "focus-visible:relative focus-visible:z-[15]",
                        isSelected
                          ? // DESIGN.md:654 gives `--surface-2` the "selected rows"
                            // job, with `--accent` as the selection identity in the
                            // sanctioned left-bar form. The hover variant is
                            // *replaced*, not stacked: emitted after the base
                            // utilities, it would otherwise erase the selection cue
                            // exactly while the operator is aiming at the row.
                            "bg-surface-2 shadow-[inset_2px_0_0_var(--nimbus-accent)] [--row-bg:var(--nimbus-surface-2)]"
                          : "[--row-bg:var(--nimbus-surface)] hover:bg-surface-2 hover:[--row-bg:var(--nimbus-surface-2)]",
                      )}
                      data-testid={`documents-row-${id}`}
                      onFocus={() => setFocusRow(index)}
                      onClick={(
                        event: ReactMouseEvent<HTMLTableRowElement>,
                      ) => {
                        if (
                          (event.target as HTMLElement).closest(INTERACTIVE)
                        ) {
                          return;
                        }
                        onEdit(doc);
                      }}
                      onContextMenu={(event) => {
                        event.preventDefault();
                        setMenu({
                          x: event.clientX,
                          y: event.clientY,
                          doc,
                          anchor: rowRefs.current[index],
                        });
                      }}
                      onKeyDown={(event) => {
                        if (event.key === "ArrowDown") {
                          event.preventDefault();
                          moveFocus(index, 1);
                        } else if (event.key === "ArrowUp") {
                          event.preventDefault();
                          moveFocus(index, -1);
                        } else if (
                          (event.key === "Enter" || event.key === " ") &&
                          event.target === event.currentTarget
                        ) {
                          event.preventDefault();
                          onEdit(doc);
                        } else if (
                          event.key === "ContextMenu" ||
                          (event.key === "F10" && event.shiftKey)
                        ) {
                          event.preventDefault();
                          openMenuForRow(index, doc);
                        }
                      }}
                    >
                      <Td className={cn("left-0 w-px px-3", PIN_L)}>
                        <Checkbox
                          label={`Select document ${shortId(id)}`}
                          checked={isSelected}
                          onChange={(checked) => onToggleOne(id, checked)}
                          testid={`documents-select-${id}`}
                        />
                      </Td>
                      {columns.map((col, i) => (
                        <Td
                          key={col}
                          className={cn(
                            "font-mono text-default",
                            i === 0 &&
                              cn("left-[38px] border-r border-app", PIN_L),
                          )}
                        >
                          <CellValue
                            value={doc[col]}
                            field={col}
                            id={id}
                            onExpand={() => onEdit(doc)}
                          />
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
                          className="mr-2 rounded border border-app px-2 py-0.5 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
                          data-testid={`documents-edit-${id}`}
                        >
                          edit
                        </button>
                        <button
                          type="button"
                          onClick={() => onDelete([id])}
                          className="rounded border border-app px-2 py-0.5 font-mono text-xs uppercase tracking-wide text-danger hover:bg-surface"
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
      {menu ? (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          label={`Document ${shortId(String(menu.doc._id ?? ""))} actions`}
          items={menuItems(menu.doc)}
          restoreFocus={menu.anchor}
          onClose={() => setMenu(null)}
          testid="documents-row-menu"
        />
      ) : null}
      <div
        className="flex items-center justify-between border-t border-app bg-surface-2 px-3 py-2 font-mono text-xs text-muted"
        data-testid="documents-pagination"
      >
        <span className="tabular">
          page {pageNumber} ·{" "}
          {loading ? (
            "loading…"
          ) : (
            <>
              {page.data.length} row{page.data.length === 1 ? "" : "s"}
              {selectedOnPage > 0 ? ` · ${selectedOnPage} selected` : ""}
            </>
          )}
        </span>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onPrev}
            disabled={loading || pageNumber <= 1}
            className={cn(
              "rounded border border-app px-2 py-0.5 uppercase tracking-wide",
              loading || pageNumber <= 1
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
            disabled={loading || !page.has_more}
            className={cn(
              "rounded border border-app px-2 py-0.5 uppercase tracking-wide",
              loading || !page.has_more
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

// DESIGN.md:889 — a loading state preserves the table's geometry. These rows
// carry the real column set at the real row height, so a page that arrives
// shifts nothing, and no scope ever paints another scope's documents while it
// waits. Deterministic widths keep the placeholder from animating on rerender.
const BAR_WIDTHS = ["62%", "84%", "46%", "72%"];

function SkeletonRows({ rows, columns }: { rows: number; columns: string[] }) {
  return (
    <>
      {Array.from({ length: rows }, (_, row) => (
        // biome-ignore lint/a11y/noAriaHiddenOnFocusable: a placeholder row carries no tabIndex, so it is not focusable — the pager is what announces the load
        <tr
          // biome-ignore lint/suspicious/noArrayIndexKey: placeholders have no identity beyond their position
          key={row}
          aria-hidden="true"
          className="h-9 [--row-bg:var(--nimbus-surface)] [&>td]:border-t [&>td]:border-app"
          data-testid="documents-skeleton-row"
        >
          <Td className={cn("left-0 w-px px-3", PIN_L)}>
            <span className="block size-3.5 rounded-sm bg-surface-2" />
          </Td>
          {columns.map((col, i) => (
            <Td
              key={col}
              className={cn(
                i === 0 && cn("left-[38px] border-r border-app", PIN_L),
              )}
            >
              <span
                className="block h-2 rounded bg-surface-2"
                style={{ width: BAR_WIDTHS[(row + i) % BAR_WIDTHS.length] }}
              />
            </Td>
          ))}
          <Td align="right" className={cn("w-px border-l border-app", PIN_R)}>
            <span className="block h-2 w-10 rounded bg-surface-2" />
          </Td>
        </tr>
      ))}
    </>
  );
}

// Headers used to be inert text. A sort control belongs on the header itself,
// and DESIGN.md:269 wants index use visible: an index-backed column carries a
// brand dot, everything else routes through the page's scan confirmation.
function SortHeader({
  field,
  order,
  indexed,
  onSort,
}: {
  field: string;
  order: DocumentOrder | null;
  indexed: boolean;
  onSort: (field: string) => void;
}) {
  const active = order?.field === field;
  const Icon = !active
    ? ChevronsUpDown
    : order.direction === "asc"
      ? ArrowUp
      : ArrowDown;
  return (
    <button
      type="button"
      onClick={() => onSort(field)}
      aria-label={`Sort by ${field}`}
      title={
        indexed
          ? `Sort by ${field} — index-backed`
          : `Sort by ${field} — no index leads with this field, so sorting scans the table`
      }
      className="group/sort flex w-full items-center gap-1 uppercase tracking-[0.14em] text-muted hover:text-default"
      data-testid={`documents-sort-${field}`}
      data-active={active ? "true" : "false"}
    >
      <span className={cn("truncate", active && "text-default")}>{field}</span>
      {indexed ? (
        <span aria-hidden className="size-1.5 shrink-0 rounded-full bg-brand" />
      ) : null}
      <Icon
        size={11}
        aria-hidden
        className={cn(
          "shrink-0",
          active
            ? "text-default"
            : "opacity-0 transition-opacity group-hover/sort:opacity-60",
        )}
      />
    </button>
  );
}

async function copyText(value: string, label: string) {
  try {
    await navigator.clipboard.writeText(value);
    toast(`Copied ${label}`);
  } catch {
    toast.error(`Failed to copy ${label}`);
  }
}
