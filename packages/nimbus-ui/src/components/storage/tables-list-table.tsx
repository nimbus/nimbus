import { Link, useNavigate } from "@tanstack/react-router";
import { useMemo, useRef, useState } from "react";
import { toast } from "sonner";

import { cn } from "../../lib/cn";
import type { TableDoc } from "../../lib/types/table";
import { CopyChip } from "../copy-chip";
import { Td, Th } from "../data-table";
import { RelativeTime } from "../time";
import { RowContextMenu, type RowMenuItem } from "./row-context-menu";

const INTERACTIVE = "button, a, input, label, [role='menuitem']";

type MenuState = {
  x: number;
  y: number;
  name: string;
  anchor: HTMLElement | null;
};

/**
 * The Storage index table.
 *
 * The Tables sub-drawer beside it is the section's navigator; this pane earns
 * its space by carrying what the drawer cannot — schema state, row counts, last
 * write time, copy affordances, and the row's own action set. Rows behave like
 * every other resource row in the console: click opens, right-click opens the
 * peer menu (DESIGN.md:1117).
 */
export function TablesListTable({ tables }: { tables: TableDoc[] }) {
  const navigate = useNavigate();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [focusRow, setFocusRow] = useState(0);
  const rowRefs = useRef<Array<HTMLTableRowElement | null>>([]);

  const sorted = useMemo(
    () =>
      tables.slice().sort((a, b) => (a.name ?? "").localeCompare(b.name ?? "")),
    [tables],
  );

  const open = (name: string, panel?: "schema" | "indexes") => {
    void navigate({
      to: "/developer/storage/$table",
      params: { table: name },
      search: panel ? { panel } : {},
    });
  };

  const items = (name: string): RowMenuItem[] => [
    { id: "open", label: "Open table", onSelect: () => open(name) },
    {
      id: "schema",
      label: "Open schema panel",
      onSelect: () => open(name, "schema"),
    },
    {
      id: "indexes",
      label: "Open indexes panel",
      onSelect: () => open(name, "indexes"),
    },
    {
      id: "copy",
      label: "Copy table name",
      hint: name,
      onSelect: () => {
        void navigator.clipboard
          .writeText(name)
          .then(() => toast(`Copied table name`))
          .catch(() => toast.error("Failed to copy table name"));
      },
    },
  ];

  const moveFocus = (from: number, delta: number) => {
    const next = Math.min(Math.max(from + delta, 0), sorted.length - 1);
    setFocusRow(next);
    rowRefs.current[next]?.focus();
  };

  return (
    <div className="overflow-auto">
      <table
        className="w-full border-collapse text-sm"
        data-testid="tenant-tables-table"
      >
        <thead className="sticky top-0 bg-surface-2 text-xs uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Table</Th>
            <Th>Schema</Th>
            <Th align="right">Rows</Th>
            <Th>Last write</Th>
            <Th align="right" className="w-px">
              actions
            </Th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((table, index) => {
            const name = table.name ?? table._id;
            return (
              <tr
                key={table._id}
                ref={(el) => {
                  rowRefs.current[index] = el;
                }}
                tabIndex={index === focusRow ? 0 : -1}
                className={cn(
                  "group h-9 cursor-pointer border-t border-app outline-none hover:bg-surface-2",
                  "focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-[color:var(--nimbus-accent)]",
                )}
                data-testid={`tenant-table-row-${name}`}
                onFocus={() => setFocusRow(index)}
                onClick={(event) => {
                  if ((event.target as HTMLElement).closest(INTERACTIVE))
                    return;
                  open(name);
                }}
                onContextMenu={(event) => {
                  event.preventDefault();
                  setMenu({
                    x: event.clientX,
                    y: event.clientY,
                    name,
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
                    open(name);
                  } else if (
                    event.key === "ContextMenu" ||
                    (event.key === "F10" && event.shiftKey)
                  ) {
                    event.preventDefault();
                    const rect =
                      rowRefs.current[index]?.getBoundingClientRect();
                    setMenu({
                      x: rect ? rect.left + 24 : 0,
                      y: rect ? rect.bottom : 0,
                      name,
                      anchor: rowRefs.current[index],
                    });
                  }
                }}
              >
                <Td>
                  <Link
                    to="/developer/storage/$table"
                    params={{ table: name }}
                    className="font-mono text-default hover:underline"
                    data-testid={`tenant-table-link-${name}`}
                  >
                    {name}
                  </Link>
                  <span className="ml-2 align-middle">
                    <CopyChip
                      label="table name"
                      value={name}
                      hideUntilHover
                      testid={`tenant-table-copy-${name}`}
                    >
                      copy
                    </CopyChip>
                  </span>
                </Td>
                <Td mono>{table.schema ? "defined" : "any"}</Td>
                <Td align="right" mono>
                  {table.rowCount ?? 0}
                </Td>
                <Td>
                  {table.lastWriteAt ? (
                    <RelativeTime epochMs={table.lastWriteAt} />
                  ) : (
                    <span className="text-muted">never</span>
                  )}
                </Td>
                <Td
                  align="right"
                  className="w-px whitespace-nowrap opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100"
                >
                  <button
                    type="button"
                    onClick={() => open(name, "schema")}
                    className="mr-2 rounded border border-app px-2 py-0.5 font-mono text-xs uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
                    data-testid={`tenant-table-schema-${name}`}
                  >
                    schema
                  </button>
                  <button
                    type="button"
                    onClick={() => open(name)}
                    className="rounded border border-app px-2 py-0.5 font-mono text-xs uppercase tracking-wide text-default hover:bg-surface"
                    data-testid={`tenant-table-open-${name}`}
                  >
                    open
                  </button>
                </Td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {menu ? (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          label={`Table ${menu.name} actions`}
          items={items(menu.name)}
          restoreFocus={menu.anchor}
          onClose={() => setMenu(null)}
          testid="tenant-table-row-menu"
        />
      ) : null}
    </div>
  );
}
