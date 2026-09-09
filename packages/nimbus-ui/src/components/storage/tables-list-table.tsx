import { Link, useNavigate } from "@tanstack/react-router";
import { useCallback, useMemo, useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import type { TableDoc } from "../../lib/types/table";
import { CopyChip } from "../copy-chip";
import {
  type DataColumn,
  DataTable,
  dataColumns,
  type RowAnchor,
} from "../data-table";
import { RelativeTime } from "../time";
import { RowContextMenu, type RowMenuItem } from "./row-context-menu";

type TableTab = "schema" | "indexes";
type MenuState = RowAnchor & { name: string };

const NO_TABLES: TableDoc[] = [];
const column = dataColumns<TableDoc>();

function tableName(table: TableDoc): string {
  return table.name ?? table._id;
}

/**
 * The Storage index table.
 *
 * The Tables sub-panel beside it is the section's navigator; this pane earns
 * its space by carrying what the sub-panel cannot — schema state, row counts,
 * last write time, copy affordances, and the row's own action set. Rows
 * behave like every other resource row in the console: click opens,
 * right-click opens the peer menu. Undefined tables paint skeleton rows
 * under the same header, so the list arrives without moving anything.
 */
export function TablesListTable({
  tables,
}: {
  tables: TableDoc[] | undefined;
}) {
  const navigate = useNavigate();
  const [menu, setMenu] = useState<MenuState | null>(null);

  const sorted = useMemo(
    () =>
      (tables ?? NO_TABLES)
        .slice()
        .sort((a, b) => tableName(a).localeCompare(tableName(b))),
    [tables],
  );

  const open = useCallback(
    (name: string, tab?: TableTab) => {
      void navigate({
        to: "/developer/storage/$table",
        params: { table: name },
        search: tab ? { tab } : {},
      });
    },
    [navigate],
  );

  const columns = useMemo<DataColumn<TableDoc>[]>(
    () => [
      column.accessor(tableName, {
        id: "name",
        header: "Table",
        size: 240,
        cell: ({ getValue }) => {
          const name = getValue();
          return (
            <span className="inline-flex min-w-0 items-center">
              <Link
                to="/developer/storage/$table"
                params={{ table: name }}
                className="truncate font-mono text-xs text-text-1 hover:underline"
                data-testid={`tenant-table-link-${name}`}
              >
                {name}
              </Link>
              <span className="ml-2">
                <CopyChip
                  label="table name"
                  value={name}
                  hideUntilHover
                  testid={`tenant-table-copy-${name}`}
                >
                  copy
                </CopyChip>
              </span>
            </span>
          );
        },
      }),
      column.accessor((table) => (table.schema ? "defined" : "any"), {
        id: "schema",
        header: "Schema",
        size: 110,
        cell: ({ getValue }) => (
          <span className="font-mono text-xs">{getValue()}</span>
        ),
      }),
      column.accessor((table) => table.rowCount ?? 0, {
        id: "rows",
        header: "Rows",
        size: 90,
        sortFn: "basic",
        cell: ({ getValue }) => (
          <span className="block text-right font-mono text-xs tabular">
            {getValue()}
          </span>
        ),
      }),
      column.accessor((table) => table.lastWriteAt ?? 0, {
        id: "lastWrite",
        header: "Last write",
        size: 150,
        sortFn: "basic",
        cell: ({ row }) =>
          row.original.lastWriteAt ? (
            <RelativeTime epochMs={row.original.lastWriteAt} />
          ) : (
            <span className="text-text-3">never</span>
          ),
      }),
      {
        id: "actions",
        size: 150,
        minSize: 150,
        maxSize: 150,
        enableSorting: false,
        enableResizing: false,
        header: () => <span className="sr-only">Actions</span>,
        // Inline actions appear on hover and stay keyboard reachable:
        // opacity keeps them in the tab order, and focus-within reveals
        // them when tabbed to.
        cell: ({ row }) => {
          const name = tableName(row.original);
          return (
            <div className="flex justify-end gap-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
              <Button
                type="button"
                variant="ghost"
                size="xs"
                onClick={() => open(name, "schema")}
                data-testid={`tenant-table-schema-${name}`}
              >
                Schema
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="xs"
                onClick={() => open(name)}
                data-testid={`tenant-table-open-${name}`}
              >
                Open
              </Button>
            </div>
          );
        },
      },
    ],
    [open],
  );

  const items = (name: string): RowMenuItem[] => [
    { id: "open", label: "Open table", onSelect: () => open(name) },
    {
      id: "schema",
      label: "Open schema",
      onSelect: () => open(name, "schema"),
    },
    {
      id: "indexes",
      label: "Open indexes",
      onSelect: () => open(name, "indexes"),
    },
    {
      id: "copy",
      label: "Copy table name",
      hint: name,
      onSelect: () => {
        void navigator.clipboard
          .writeText(name)
          .then(() => toast("Copied table name"))
          .catch(() => toast.error("Failed to copy table name"));
      },
    },
  ];

  return (
    <>
      <DataTable
        columns={columns}
        data={sorted}
        getRowId={(table) => table._id}
        ariaLabel="Tables"
        testid="tenant-tables-table"
        rowTestid={(table) => `tenant-table-row-${tableName(table)}`}
        rowClassName={() => "group"}
        loading={tables === undefined}
        onRowActivate={(table) => open(tableName(table))}
        onRowContextMenu={(table, anchor) =>
          setMenu({ ...anchor, name: tableName(table) })
        }
        className="h-full"
      />
      {menu ? (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          label={`Table ${menu.name} actions`}
          items={items(menu.name)}
          restoreFocus={menu.element}
          onClose={() => setMenu(null)}
          testid="tenant-table-row-menu"
        />
      ) : null}
    </>
  );
}
