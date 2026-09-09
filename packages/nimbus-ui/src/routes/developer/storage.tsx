import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute } from "@tanstack/react-router";

import { api } from "../../../convex/_generated/api";
import { Breadcrumb } from "../../components/breadcrumb";
import { EmptyState } from "../../components/empty-state";
import { LoadingState } from "../../components/loading-state";
import { PageHeader } from "../../components/page-header";
import { TablesListTable } from "../../components/storage/tables-list-table";
import { useTablesSubPanel } from "../../components/storage/tables-sub-panel";
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
  useTablesSubPanel({ tenant, tables, hasTenants });

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-tenant-tables"
    >
      {/* Breadcrumb above, shared header molecule below — the same two-part
          shape the drill-in route `storage_.$table.tsx` uses, so the two pages
          keep one measure and one title rhythm across the navigation. */}
      <div className="flex flex-col gap-2">
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
        {/* The tenant id loses its mono span here, matching how the drill-in
            route titles a bare table name. The breadcrumb directly above still
            carries the id in mono with its copy affordance, so the identifier
            treatment is not lost — it stops being duplicated in two faces. */}
        <PageHeader
          title={tenant ? `Tables in ${tenant}` : "Storage"}
          subtitle="Tables appear as soon as documents are written. A table without a schema takes any document shape."
          testid="tenant-tables-header"
        />
      </div>

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {!tenant ? (
          // `useTenantList` reports three kinds and this panel has to honour
          // all three. Reducing them to a boolean sent "Select a tenant" —
          // an instruction — to a reader whose tenant list was still in
          // flight, and left it on screen permanently when the list had
          // failed outright, with nothing anywhere saying so.
          tenantList.kind === "loading" ? (
            <LoadingState
              label="Loading tenants…"
              testid="tenant-tables-tenants-loading"
            />
          ) : tenantList.kind === "error" ? (
            <EmptyState
              title="Tenants endpoint unavailable"
              body={
                <>
                  This deployment can&apos;t reach{" "}
                  <code className="font-mono text-text-1">/api/tenants</code>:{" "}
                  <span
                    className="font-mono text-text-1"
                    data-testid="tenant-tables-tenants-error-message"
                  >
                    {tenantList.message}
                  </span>
                  . Tables live inside tenants, so none can be listed until the
                  tenant list loads.
                </>
              }
              cta={{ label: "Retry", onClick: tenantList.reload }}
              testid="tenant-tables-tenants-error"
            />
          ) : hasTenants === false ? (
            <EmptyState
              title="No tenants yet"
              body="Click + CREATE TENANT in the top nav to create one. Tables and documents scope to a tenant — once a tenant exists, you can pick it from the selector to see its tables."
              testid="tenant-tables-empty"
            />
          ) : (
            <EmptyState
              title="Select a tenant"
              body="Pick a tenant from the sidebar selector to see its tables."
              testid="tenant-tables-empty"
            />
          )
        ) : tables !== undefined && tables.length === 0 ? (
          <EmptyState
            title="No tables"
            body={`Insert a document via POST /api/tenants/${tenant}/documents or call ctx.db.insert("<table>", ...) from a registered function. Tables appear here as soon as they receive their first write.`}
            testid="tenant-tables-empty"
          />
        ) : (
          // The list table draws its own skeleton rows while `tables` is
          // undefined, under the same header the rows arrive under, so the
          // load moves nothing vertically.
          <TablesListTable tables={tables} />
        )}
      </div>
    </section>
  );
}
