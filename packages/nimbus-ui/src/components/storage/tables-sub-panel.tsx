import { Link, useRouterState } from "@tanstack/react-router";
import { useMemo } from "react";

import { cn } from "@/lib/utils";
import type { TableDoc } from "../../lib/types/table";
import {
  type SubPanelSpec,
  useContributeSubPanel,
  useSubPanelSearch,
} from "../../shell/sub-panel";

const TABLE_ROUTE_PREFIX = "/developer/storage/";

// Both storage routes contribute the *same* Tables panel. The detail route
// used to contribute nothing, so drilling into a table deleted the only table
// navigator in the section and shifted the layout by the panel's width on
// every drill-in. Sharing one spec makes the shell what the layout system
// promises: a persistent list beside distinct detail content.
export function useTablesSubPanel({
  tenant,
  tables,
  hasTenants,
}: {
  tenant: string | null;
  tables: TableDoc[] | undefined;
  hasTenants: boolean | undefined;
}) {
  // Keyed on `tables` itself, never on a derived array: a fresh `sort()` result
  // on every render would give the spec a new identity every render, and
  // `useContributeSubPanel`'s effect re-runs `setSearch("")` on every spec
  // change — which would erase the operator's filter text as they typed it.
  const spec = useMemo<SubPanelSpec>(
    () => ({
      kind: "dynamic",
      title: "Tables",
      search: { placeholder: "Filter tables", rows: tables?.length ?? 0 },
      children: !tenant ? (
        <NoTenantHelp hasTenants={hasTenants} />
      ) : tables === undefined ? (
        <div className="px-3 py-3 text-xs text-text-3">
          <span aria-hidden>·</span>
          <span className="sr-only">loading</span>
        </div>
      ) : (
        <TablesSubPanelList tables={tables} />
      ),
    }),
    [tenant, tables, hasTenants],
  );
  useContributeSubPanel(spec);
}

function NoTenantHelp({ hasTenants }: { hasTenants: boolean | undefined }) {
  return (
    <div className="px-3 py-6 text-xs text-text-3">
      {hasTenants === false ? (
        <>
          <p>No tenants yet.</p>
          <p className="mt-2">
            Use <code className="font-mono text-text-1">Create tenant</code> in
            the sidebar scope row to create one. Tables and documents scope to a
            tenant.
          </p>
        </>
      ) : (
        <>
          <p>Select a tenant.</p>
          <p className="mt-2">
            Pick a tenant from the sidebar selector to see its tables.
          </p>
        </>
      )}
    </div>
  );
}

// Split out so the two hooks it needs (`useSubPanelSearch`, `useRouterState`)
// are called during *render*, not while the parent route builds the spec. That
// keeps the spec a plain element whose identity depends only on `tables`.
function TablesSubPanelList({ tables }: { tables: TableDoc[] }) {
  const search = useSubPanelSearch();
  // Derived from the router rather than from a passed-in `$table` param so the
  // highlight stays correct if the route shape changes — the same source
  // `isItemActive` uses for the static drawer mode.
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const activeName = pathname.startsWith(TABLE_ROUTE_PREFIX)
    ? decodeURIComponent(pathname.slice(TABLE_ROUTE_PREFIX.length))
    : null;

  const sorted = useMemo(
    () =>
      tables.slice().sort((a, b) => (a.name ?? "").localeCompare(b.name ?? "")),
    [tables],
  );
  const query = search.trim().toLowerCase();
  const visible = query
    ? sorted.filter((t) => (t.name ?? t._id).toLowerCase().includes(query))
    : sorted;

  if (sorted.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-text-3">
        <p>No tables yet.</p>
        <p className="mt-2">
          Insert a document or call{" "}
          <code className="font-mono">ctx.db.insert</code> to materialize one.
        </p>
      </div>
    );
  }

  if (visible.length === 0) {
    return (
      <div
        className="px-3 py-6 text-xs text-text-3"
        data-testid="sub-panel-tables-no-match"
      >
        No table matches <span className="font-mono text-text-1">{search}</span>
        .
      </div>
    );
  }

  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {visible.map((table) => {
        const name = table.name ?? table._id;
        const active = name === activeName;
        return (
          <li key={table._id}>
            <Link
              to="/developer/storage/$table"
              params={{ table: name }}
              aria-current={active ? "page" : undefined}
              data-testid={`sub-panel-item-dev-${name}`}
              data-active={active ? "true" : "false"}
              className={cn(
                "flex h-8 items-center gap-2 rounded-md border-l-2 border-transparent px-2 text-sm",
                active
                  ? "bg-bg-raised text-text-1"
                  : "text-text-3 hover:bg-bg-raised hover:text-text-1",
              )}
              style={active ? { borderLeftColor: "var(--accent)" } : undefined}
            >
              <span className="flex-1 truncate font-mono text-xs">{name}</span>
              {typeof table.rowCount === "number" ? (
                <span className="tabular font-mono text-xs text-text-3">
                  {table.rowCount}
                </span>
              ) : null}
            </Link>
          </li>
        );
      })}
    </ul>
  );
}
