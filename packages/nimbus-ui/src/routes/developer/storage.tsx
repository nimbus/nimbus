import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute } from "@tanstack/react-router";

import { api } from "../../../convex/_generated/api";
import { Breadcrumb } from "../../components/breadcrumb";
import { Th } from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { SkeletonRows } from "../../components/loading-state";
import { TablesListTable } from "../../components/storage/tables-list-table";
import { useTablesSubDrawer } from "../../components/storage/tables-sub-drawer";
import { useTenantList } from "../../hooks/use-tenant-list";
import type { TableDoc } from "../../lib/types/table";
import { useUiStore } from "../../store/ui-store";

export const Route = createFileRoute("/developer/storage")({
  component: StoragePage,
});

function StoragePage() {
  const tenant = useUiStore((s) => s.activeTenant);
  const tenantList = useTenantList();
  const tables = useQuery(
    api.tables.list,
    tenant ? { tenantId: tenant, limit: 200 } : "skip",
  ) as TableDoc[] | undefined;

  const hasTenants =
    tenantList.kind === "loaded" ? tenantList.tenants.length > 0 : undefined;

  // Shared with the table detail route, so the Tables list stays beside the
  // documents instead of vanishing on drill-in.
  useTablesSubDrawer({ tenant, tables, hasTenants });

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-tenant-tables"
    >
      <header className="flex flex-col gap-2">
        <Breadcrumb
          segments={
            tenant
              ? [
                  {
                    label: tenant,
                    copyValue: tenant,
                    copyLabel: "tenant id",
                    active: true,
                  },
                ]
              : []
          }
          testid="tenant-breadcrumb"
        />
        <div className="flex items-baseline justify-between">
          <div>
            <h1 className="text-default" style={{ fontSize: "var(--text-xl)" }}>
              {tenant ? (
                <>
                  Tables in <span className="font-mono">{tenant}</span>
                </>
              ) : (
                "Storage"
              )}
            </h1>
            <p className="text-sm text-muted">
              Tables are reactive — they appear here as soon as documents are
              written. A table without a schema accepts any document shape.
            </p>
          </div>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        {!tenant ? (
          hasTenants === false ? (
            <EmptyState
              title="No tenants yet"
              body="Click + CREATE TENANT in the top nav to create one. Tables and documents scope to a tenant — once a tenant exists, you can pick it from the selector to see its tables."
              testid="tenant-tables-empty"
            />
          ) : (
            <EmptyState
              title="Select a tenant"
              body="Pick a tenant from the top-nav selector to see its tables."
              testid="tenant-tables-empty"
            />
          )
        ) : tables === undefined ? (
          // Skeleton rows, not a centered spinner: the header, the panel and
          // the 40px row rhythm all survive the load, so arriving tables move
          // nothing vertically. `table-auto` still re-proportions the columns
          // on arrival — the bars are not the content it measures. No
          // `rowContentHeight`: `Td`'s 40px row floor already sizes the real
          // and the placeholder rows alike (measured 40.00px in both states).
          <SkeletonRows
            columns={5}
            head={<TablesTableHead />}
            label="Loading tables…"
            testid="tenant-tables-loading"
          />
        ) : tables.length === 0 ? (
          <EmptyState
            title="No tables"
            body={`Insert a document via POST /api/tenants/${tenant}/documents or call ctx.db.insert("<table>", ...) from a registered function. Tables appear here as soon as they receive their first write.`}
            testid="tenant-tables-empty"
          />
        ) : (
          <TablesListTable tables={tables} />
        )}
      </div>
    </section>
  );
}

/**
 * The Tables list header, declared here because the loading branch and the
 * loaded table are owned by different files: `TablesListTable` renders the
 * rows, this route renders the placeholder that has to match them. Keep the
 * two column sets in step — the skeleton is only honest while it is.
 */
function TablesTableHead() {
  return (
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
  );
}
