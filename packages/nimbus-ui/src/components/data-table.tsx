/* biome-ignore-all lint/a11y/useSemanticElements: a virtualized table positions each row with a transform, which native table rows cannot take, so the table is built from divs that carry the ARIA table roles */
/* biome-ignore-all lint/a11y/useFocusableInteractive: rows take focus only when they activate; header and cells are static table parts */
import {
  type ColumnDef,
  type ColumnSizingState,
  columnResizingFeature,
  columnSizingFeature,
  createColumnHelper,
  createSortedRowModel,
  flexRender,
  type OnChangeFn,
  type Row,
  type RowData,
  type RowSelectionState,
  rowSelectionFeature,
  rowSortingFeature,
  type SortingState,
  sortFn_alphanumeric,
  sortFn_basic,
  tableFeatures,
  useTable,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp } from "lucide-react";
import {
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  useMemo,
  useRef,
  useState,
} from "react";

import { Checkbox } from "@/components/ui/checkbox";
import { formatCount } from "@/lib/format";
import { cn } from "@/lib/utils";

// DataTable is the one dense table. Every list in the console is this
// component with a column plan, so sorting, resizing, selection, row
// activation, and the empty row look and behave the same on every page.
// Above VIRTUAL_THRESHOLD rows the body is virtualized on the table's own
// scroller, so a 10,000-row list costs what a 30-row list costs.

export const dataTableFeatures = tableFeatures({
  rowSortingFeature,
  columnSizingFeature,
  columnResizingFeature,
  rowSelectionFeature,
  sortedRowModel: createSortedRowModel(),
  sortFns: { alphanumeric: sortFn_alphanumeric, basic: sortFn_basic },
});

export type DataTableFeatures = typeof dataTableFeatures;

// The value type is any because one table mixes accessor types.
export type DataColumn<TData extends RowData> = ColumnDef<
  DataTableFeatures,
  TData,
  // biome-ignore lint/suspicious/noExplicitAny: a column plan mixes value types
  any
>;

// dataColumns is the column helper typed against the table's feature set,
// so a page declares its plan without naming the feature generics.
export function dataColumns<TData extends RowData>() {
  return createColumnHelper<DataTableFeatures, TData>();
}

export const ROW_HEIGHT = 40;
export const VIRTUAL_THRESHOLD = 100;

// selectionColumn is the checkbox column a bulk toolbar needs. It sorts
// nothing, resizes nothing, and names each box after the row.
export function selectionColumn<TData extends RowData>(
  nameOf: (row: TData) => string,
): DataColumn<TData> {
  return {
    id: "select",
    size: 36,
    minSize: 36,
    maxSize: 36,
    enableSorting: false,
    enableResizing: false,
    header: ({ table }) => (
      <Checkbox
        aria-label="Select all rows"
        checked={table.getIsAllRowsSelected()}
        indeterminate={table.getIsSomeRowsSelected()}
        onCheckedChange={(checked) =>
          table.toggleAllRowsSelected(checked === true)
        }
      />
    ),
    cell: ({ row }) => (
      <Checkbox
        aria-label={`Select ${nameOf(row.original)}`}
        checked={row.getIsSelected()}
        onCheckedChange={(checked) => row.toggleSelected(checked === true)}
      />
    ),
  };
}

export type DataTableProps<TData extends RowData> = {
  columns: ReadonlyArray<DataColumn<TData>>;
  data: ReadonlyArray<TData>;
  getRowId: (row: TData) => string;
  // ariaLabel names the grid for a screen reader.
  ariaLabel: string;
  sorting?: SortingState;
  onSortingChange?: OnChangeFn<SortingState>;
  rowSelection?: RowSelectionState;
  onRowSelectionChange?: OnChangeFn<RowSelectionState>;
  // onRowActivate fires on click, Enter, or Space on a row, unless the
  // event started inside a control the row contains.
  onRowActivate?: (row: TData) => void;
  emptyMessage?: ReactNode;
  // virtual forces the virtualizer on or off; by default it turns on past
  // VIRTUAL_THRESHOLD rows.
  virtual?: boolean;
  // maxHeight bounds the scroller; omit it when the parent bounds it.
  maxHeight?: number | string;
  testid?: string;
  className?: string;
  rowClassName?: (row: TData) => string | undefined;
};

// isInnerControl says whether an event started inside a control the row
// holds (a link, a button, a checkbox, an input), so activating the row
// does not also fire for the control the reader meant.
export function isInnerControl(target: EventTarget | null, row: HTMLElement) {
  let node = target instanceof Element ? target : null;
  while (node && node !== row) {
    if (
      node instanceof HTMLAnchorElement ||
      node instanceof HTMLButtonElement ||
      node instanceof HTMLInputElement ||
      node instanceof HTMLSelectElement ||
      node instanceof HTMLTextAreaElement ||
      node.getAttribute("role") === "checkbox" ||
      node.getAttribute("role") === "button"
    ) {
      return true;
    }
    node = node.parentElement;
  }
  return false;
}

export function DataTable<TData extends RowData>({
  columns,
  data,
  getRowId,
  ariaLabel,
  sorting: sortingProp,
  onSortingChange,
  rowSelection: rowSelectionProp,
  onRowSelectionChange,
  onRowActivate,
  emptyMessage = "Nothing to show.",
  virtual,
  maxHeight,
  testid,
  className,
  rowClassName,
}: DataTableProps<TData>) {
  const [sortingState, setSortingState] = useState<SortingState>([]);
  const [selectionState, setSelectionState] = useState<RowSelectionState>({});
  const [columnSizing, setColumnSizing] = useState<ColumnSizingState>({});
  const sorting = sortingProp ?? sortingState;
  const rowSelection = rowSelectionProp ?? selectionState;
  const columnDefs = useMemo(() => [...columns], [columns]);
  const rows = useMemo(() => [...data], [data]);

  const table = useTable({
    features: dataTableFeatures,
    columns: columnDefs,
    data: rows,
    getRowId: (row) => getRowId(row),
    state: { sorting, rowSelection, columnSizing },
    onSortingChange: onSortingChange ?? setSortingState,
    onRowSelectionChange: onRowSelectionChange ?? setSelectionState,
    onColumnSizingChange: setColumnSizing,
    columnResizeMode: "onChange",
    enableRowSelection: true,
    // The first click sorts ascending for every column. TanStack's automatic
    // direction reads the filtered row model, which is empty here because
    // the filter feature is not loaded, and it would fall back to descending.
    sortDescFirst: false,
  });

  const rowModel = table.getRowModel().rows;
  const isVirtual = virtual ?? rowModel.length > VIRTUAL_THRESHOLD;
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: rowModel.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
    enabled: isVirtual,
  });

  const headerGroups = table.getHeaderGroups();
  const template = table
    .getAllLeafColumns()
    .map((column) =>
      column.columnDef.enableResizing === false || column.columnDef.maxSize
        ? `${column.getSize()}px`
        : `minmax(${column.getSize()}px, 1fr)`,
    )
    .join(" ");
  const gridStyle: CSSProperties = { gridTemplateColumns: template };

  const activate = (row: Row<DataTableFeatures, TData>) => {
    onRowActivate?.(row.original);
  };
  const onRowKeyDown = (
    event: KeyboardEvent<HTMLDivElement>,
    row: Row<DataTableFeatures, TData>,
  ) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    if (isInnerControl(event.target, event.currentTarget)) return;
    event.preventDefault();
    activate(row);
  };
  const onRowClick = (
    event: MouseEvent<HTMLDivElement>,
    row: Row<DataTableFeatures, TData>,
  ) => {
    if (isInnerControl(event.target, event.currentTarget)) return;
    activate(row);
  };

  const renderRow = (
    row: Row<DataTableFeatures, TData>,
    index: number,
    style?: CSSProperties,
  ) => (
    <div
      key={row.id}
      role="row"
      aria-rowindex={index + 2}
      aria-selected={row.getIsSelected() || undefined}
      data-testid={testid ? `${testid}-row` : undefined}
      data-row-id={row.id}
      tabIndex={onRowActivate ? 0 : undefined}
      onClick={onRowActivate ? (event) => onRowClick(event, row) : undefined}
      onKeyDown={
        onRowActivate ? (event) => onRowKeyDown(event, row) : undefined
      }
      className={cn(
        "grid items-center border-b border-border-1 outline-none last:border-b-0 focus-visible:border-accent",
        onRowActivate && "cursor-pointer hover:bg-bg-hover",
        row.getIsSelected() && "bg-accent-tint",
        rowClassName?.(row.original),
      )}
      style={{ ...gridStyle, height: ROW_HEIGHT, ...style }}
    >
      {row.getAllCells().map((cell) => (
        <div
          key={cell.id}
          role="cell"
          className="min-w-0 truncate px-3 text-sm text-text-1"
        >
          {flexRender(cell.column.columnDef.cell, cell.getContext())}
        </div>
      ))}
    </div>
  );

  return (
    <div
      role="table"
      aria-label={ariaLabel}
      aria-rowcount={rowModel.length + 1}
      aria-colcount={table.getAllLeafColumns().length}
      data-testid={testid}
      data-virtual={isVirtual ? "true" : undefined}
      className={cn(
        "flex min-h-0 flex-col overflow-hidden rounded-md border border-border-1 bg-bg-panel",
        className,
      )}
    >
      <div
        ref={scrollRef}
        className="min-h-0 flex-1 overflow-auto"
        style={maxHeight !== undefined ? { maxHeight } : undefined}
      >
        <div role="rowgroup" className="sticky top-0 z-10 bg-bg-panel">
          {headerGroups.map((headerGroup) => (
            <div
              key={headerGroup.id}
              role="row"
              aria-rowindex={1}
              className="grid h-8 items-center border-b border-border-2"
              style={gridStyle}
            >
              {headerGroup.headers.map((header) => {
                const column = header.column;
                const sortable = column.getCanSort();
                const sorted = column.getIsSorted();
                const label = flexRender(
                  column.columnDef.header,
                  header.getContext(),
                );
                return (
                  <div
                    key={header.id}
                    role="columnheader"
                    aria-sort={
                      sortable
                        ? sorted === "asc"
                          ? "ascending"
                          : sorted === "desc"
                            ? "descending"
                            : "none"
                        : undefined
                    }
                    className="relative flex min-w-0 items-center px-3 text-xs font-medium text-text-3"
                  >
                    {sortable ? (
                      <button
                        type="button"
                        onClick={column.getToggleSortingHandler()}
                        className="inline-flex min-w-0 items-center gap-1 truncate rounded-xs outline-none hover:text-text-1 focus-visible:text-text-1"
                      >
                        <span className="truncate">{label}</span>
                        {sorted === "asc" && (
                          <ArrowUp aria-hidden className="size-3 shrink-0" />
                        )}
                        {sorted === "desc" && (
                          <ArrowDown aria-hidden className="size-3 shrink-0" />
                        )}
                      </button>
                    ) : (
                      <span className="truncate">{label}</span>
                    )}
                    {column.getCanResize() && (
                      // The grip is a pointer-only affordance; the column
                      // keeps its size for assistive technology.
                      <div
                        aria-hidden
                        onMouseDown={header.getResizeHandler()}
                        onTouchStart={header.getResizeHandler()}
                        onDoubleClick={() => column.resetSize()}
                        className={cn(
                          "absolute top-0 right-0 h-full w-1 cursor-col-resize select-none touch-none hover:bg-border-3",
                          column.getIsResizing() && "bg-accent",
                        )}
                      />
                    )}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
        <div
          role="rowgroup"
          data-testid={testid ? `${testid}-body` : undefined}
          style={
            isVirtual
              ? { height: virtualizer.getTotalSize(), position: "relative" }
              : undefined
          }
        >
          {rowModel.length === 0 ? (
            <div role="row" aria-rowindex={2}>
              <div
                role="cell"
                aria-colspan={table.getAllLeafColumns().length}
                data-testid={testid ? `${testid}-empty` : undefined}
                className="px-3 py-8 text-center text-sm text-text-3"
              >
                {emptyMessage}
              </div>
            </div>
          ) : isVirtual ? (
            virtualizer.getVirtualItems().map((item) => {
              const row = rowModel[item.index];
              if (!row) return null;
              return renderRow(row, item.index, {
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                transform: `translateY(${item.start}px)`,
              });
            })
          ) : (
            rowModel.map((row, index) => renderRow(row, index))
          )}
        </div>
      </div>
    </div>
  );
}

// DataTableFooter reports what the table holds against what the request
// could have returned, so an operator can tell a short list from a
// truncated one.
export function DataTableFooter({
  loaded,
  pageSize,
  hasMore,
  noun,
  className,
}: {
  loaded: number;
  pageSize: number;
  hasMore?: boolean;
  noun: { one: string; many: string };
  className?: string;
}) {
  const parts = [
    `${formatCount(loaded)} ${loaded === 1 ? noun.one : noun.many} loaded`,
    `${formatCount(pageSize)} per request`,
  ];
  if (hasMore) parts.push("more exist past the bound");
  return (
    <p
      data-testid="data-table-footer"
      className={cn("px-3 py-2 font-mono text-xs text-text-3", className)}
    >
      {parts.join(" · ")}
    </p>
  );
}
