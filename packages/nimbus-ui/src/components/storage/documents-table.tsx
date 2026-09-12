import type {
  OnChangeFn,
  RowSelectionState,
  SortingState,
} from "@tanstack/react-table";
import { useMemo, useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { cn } from "@/lib/utils";
import { shortId } from "../../lib/format";
import type { DocumentJson, PageResponse } from "../../lib/types/table";
import {
  type DataColumn,
  DataTable,
  dataColumns,
  type RowAnchor,
} from "../data-table";
import { CellValue } from "./cell-value";
import { RowContextMenu, type RowMenuItem } from "./row-context-menu";
import type { DocumentOrder } from "./table-query";

/** Placeholder rows painted while the first page of a table is in flight. */
const SKELETON_ROWS = 12;
const NO_ROWS: DocumentJson[] = [];

type MenuState = RowAnchor & { doc: DocumentJson };

const column = dataColumns<DocumentJson>();

function docId(doc: DocumentJson): string {
  return String(doc._id ?? "");
}

// The document grid plus its cursor pager, on the shared DataTable. The
// table virtualizes a page past the row threshold, keeps its header sticky,
// and owns row focus, activation, and the context-menu request. Selection,
// editing, deletion, and the query live with the page component: this file
// maps them onto the table's column plan and reports the table's events
// back in the page's own terms.
export function DocumentsTable({
  page,
  columns,
  selected,
  pageNumber,
  loading,
  order,
  indexBacked,
  onSort,
  onSelectionChange,
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
  /** The full set of selected ids after a checkbox change. */
  onSelectionChange: (ids: string[]) => void;
  onEdit: (doc: DocumentJson) => void;
  onDelete: (ids: string[]) => void;
  onPrev: () => void;
  onNext: () => void;
}) {
  const rows = loading ? NO_ROWS : page.data;
  const pageIds = page.data.map(docId);
  const selectedOnPage = pageIds.filter((id) => selected.has(id)).length;

  const [menu, setMenu] = useState<MenuState | null>(null);

  const rowSelection = useMemo<RowSelectionState>(
    () => Object.fromEntries(Array.from(selected, (id) => [id, true])),
    [selected],
  );
  const onRowSelectionChange: OnChangeFn<RowSelectionState> = (updater) => {
    const next =
      typeof updater === "function" ? updater(rowSelection) : updater;
    onSelectionChange(Object.keys(next).filter((id) => next[id]));
  };

  // The server sorts, so the table reports the click and reorders nothing.
  // TanStack cycles a column asc, desc, off; the page treats "off" on the
  // active column as another flip, so the field is the one that changed or,
  // when the cycle cleared it, the one that was active.
  const sorting = useMemo<SortingState>(
    () =>
      order ? [{ id: order.field, desc: order.direction === "desc" }] : [],
    [order],
  );
  const onSortingChange: OnChangeFn<SortingState> = (updater) => {
    const next = typeof updater === "function" ? updater(sorting) : updater;
    const field = next[0]?.id ?? sorting[0]?.id;
    if (field) onSort(field);
  };

  const columnDefs = useMemo<DataColumn<DocumentJson>[]>(
    () => [
      {
        id: "select",
        size: 36,
        minSize: 36,
        maxSize: 36,
        enableSorting: false,
        enableResizing: false,
        header: ({ table }) => (
          <Checkbox
            aria-label="Select all on page"
            checked={!loading && table.getIsAllRowsSelected()}
            indeterminate={!loading && table.getIsSomeRowsSelected()}
            // While a page is in flight the rows on screen are
            // placeholders: a select-all here would select the documents
            // of the page being replaced.
            onCheckedChange={(checked) => {
              if (loading) return;
              table.toggleAllRowsSelected(checked === true);
            }}
            data-testid="documents-select-all"
          />
        ),
        cell: ({ row }) => (
          <Checkbox
            aria-label={`Select document ${shortId(docId(row.original))}`}
            checked={row.getIsSelected()}
            onCheckedChange={(checked) => row.toggleSelected(checked === true)}
            data-testid={`documents-select-${docId(row.original)}`}
          />
        ),
      },
      ...columns.map(
        (field): DataColumn<DocumentJson> =>
          column.accessor((doc) => doc[field], {
            id: field,
            size: field === "_id" ? 150 : 180,
            header: () => (
              <SortLabel
                field={field}
                active={order?.field === field}
                indexed={indexBacked.has(field)}
              />
            ),
            cell: ({ row }) => (
              <div className="font-mono text-xs">
                <CellValue
                  value={row.original[field]}
                  field={field}
                  id={docId(row.original)}
                  onExpand={() => onEdit(row.original)}
                />
              </div>
            ),
          }),
      ),
      {
        id: "actions",
        size: 128,
        minSize: 128,
        maxSize: 128,
        enableSorting: false,
        enableResizing: false,
        header: () => <span className="sr-only">Actions</span>,
        // Inline actions appear on hover and stay keyboard reachable:
        // opacity keeps them in the tab order, and focus-within reveals
        // them when tabbed to.
        cell: ({ row }) => (
          <div className="flex justify-end gap-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
            <Button
              type="button"
              variant="ghost"
              size="xs"
              onClick={() => onEdit(row.original)}
              data-testid={`documents-edit-${docId(row.original)}`}
            >
              Edit
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="xs"
              className="text-error hover:text-error"
              onClick={() => onDelete([docId(row.original)])}
              data-testid={`documents-delete-${docId(row.original)}`}
            >
              Delete
            </Button>
          </div>
        ),
      },
    ],
    [columns, order, indexBacked, loading, onEdit, onDelete],
  );

  const menuItems = (doc: DocumentJson): RowMenuItem[] => {
    const id = docId(doc);
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
      <DataTable
        columns={columnDefs}
        data={rows}
        getRowId={docId}
        ariaLabel="Documents"
        testid="documents-table"
        rowTestid={(doc) => `documents-row-${docId(doc)}`}
        rowClassName={() => "group"}
        sorting={sorting}
        onSortingChange={onSortingChange}
        manualSorting
        rowSelection={rowSelection}
        onRowSelectionChange={onRowSelectionChange}
        onRowActivate={onEdit}
        onRowContextMenu={(doc, anchor) => setMenu({ ...anchor, doc })}
        loading={loading}
        skeletonRows={page.data.length > 0 ? page.data.length : SKELETON_ROWS}
        className="min-h-0 flex-1 rounded-none border-0"
      />
      {menu ? (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          label={`Document ${shortId(docId(menu.doc))} actions`}
          items={menuItems(menu.doc)}
          restoreFocus={menu.element}
          onClose={() => setMenu(null)}
          testid="documents-row-menu"
        />
      ) : null}
      <div
        className="flex items-center justify-between border-t border-border-2 bg-bg-raised px-3 py-2 font-mono text-xs text-text-3"
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
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={onPrev}
            disabled={loading || pageNumber <= 1}
            data-testid="documents-prev-page"
          >
            Prev
          </Button>
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={onNext}
            disabled={loading || !page.has_more}
            data-testid="documents-next-page"
          >
            Next
          </Button>
        </div>
      </div>
    </div>
  );
}

// The header label of a document column. The shared table wraps it in the
// sort control and draws the direction arrow; this label carries the field
// name, the index dot DESIGN.md wants visible, and the cost of the sort in
// its title, so an operator knows before the click whether it scans.
function SortLabel({
  field,
  active,
  indexed,
}: {
  field: string;
  active: boolean;
  indexed: boolean;
}) {
  return (
    <span
      className={cn(
        "inline-flex min-w-0 items-center gap-1 truncate font-mono",
        active && "text-text-1",
      )}
      title={
        indexed
          ? `Sort by ${field} — index-backed`
          : `Sort by ${field} — no index leads with this field, so sorting scans the table`
      }
      data-testid={`documents-sort-${field}`}
      data-active={active ? "true" : "false"}
    >
      <span className="truncate">{field}</span>
      {indexed ? (
        <span
          aria-hidden
          className="size-1.5 shrink-0 rounded-full bg-accent-edge"
        />
      ) : null}
    </span>
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
