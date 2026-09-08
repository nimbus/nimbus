import {
  createFileRoute,
  Link,
  useNavigate,
  useRouter,
} from "@tanstack/react-router";
import { Ellipsis, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { api } from "../../../convex/_generated/api";
import { ConfirmDialog } from "../../components/confirm-dialog";
import { CopyChip } from "../../components/copy-chip";
import {
  DataTable,
  dataColumns,
  type RowAnchor,
} from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { createTenantCommand } from "../../components/onboarding/next-action";
import { PageHeader } from "../../components/page-header";
import {
  RowContextMenu,
  type RowMenuItem,
} from "../../components/storage/row-context-menu";
import { useServerUrl } from "../../hooks/use-server-url";
import { fetchTenants } from "../../hooks/use-tenant-list";
import { tenants as tenantApi } from "../../lib/api-mutations";
import { getNimbusClient } from "../../lib/nimbus-client";
import type { TableDoc } from "../../lib/types/table";
import {
  type SubPanelSpec,
  useContributeSubPanel,
} from "../../shell/sub-panel";
import { CreateTenantDialog } from "./tenants/-create-tenant-dialog";

type TenantsSearch = {
  // `?create=1` opens the create dialog on arrival. The tenant selector
  // sends a developer here when the server has no tenant at all.
  create?: 1;
};

export type TenantRow = {
  tenantId: string;
  tableCount: number;
  totalRows: number;
};

type LoaderResult =
  | { kind: "ok"; tenants: string[]; tables: TableDoc[] }
  | { kind: "error"; message: string };

type MenuState = RowAnchor & { row: TenantRow };

export const Route = createFileRoute("/operator/tenants")({
  validateSearch: (search: Record<string, unknown>): TenantsSearch => ({
    create: search.create === 1 || search.create === "1" ? 1 : undefined,
  }),
  loader: async ({ abortController }): Promise<LoaderResult> => {
    try {
      const tenants = await fetchTenants(abortController.signal);
      if (tenants === null) {
        return {
          kind: "error",
          message: "Tenants endpoint returned a non-OK response.",
        };
      }
      const tables = (await getNimbusClient().query(api.tables.list, {
        tenantId: null,
        limit: 200,
      })) as TableDoc[];
      return { kind: "ok", tenants, tables };
    } catch (err) {
      return {
        kind: "error",
        message: err instanceof Error ? err.message : String(err),
      };
    }
  },
  component: TenantsPage,
});

// tenantRows joins the tenant list with the table inventory. A tenant that
// the list does not name but that owns tables is still a row, so the page
// never hides data the engine holds.
export function tenantRows(
  tenants: ReadonlyArray<string>,
  tables: ReadonlyArray<TableDoc>,
): TenantRow[] {
  const byTenant = new Map<string, { count: number; rows: number }>();
  for (const table of tables) {
    if (!table.tenantId) continue;
    const entry = byTenant.get(table.tenantId) ?? { count: 0, rows: 0 };
    entry.count += 1;
    entry.rows += table.rowCount ?? 0;
    byTenant.set(table.tenantId, entry);
  }
  const ids = new Set<string>([...tenants, ...byTenant.keys()]);
  return Array.from(ids)
    .sort()
    .map((id) => ({
      tenantId: id,
      tableCount: byTenant.get(id)?.count ?? 0,
      totalRows: byTenant.get(id)?.rows ?? 0,
    }));
}

const col = dataColumns<TenantRow>();

function TenantsPage() {
  const data = Route.useLoaderData();
  const { create } = Route.useSearch();
  const router = useRouter();
  const navigate = useNavigate();
  const serverUrl = useServerUrl();
  const tenants = data.kind === "ok" ? data.tenants : [];
  const tables = data.kind === "ok" ? data.tables : [];
  const serverError = data.kind === "error" ? data.message : null;

  const rows = useMemo(() => tenantRows(tenants, tables), [tenants, tables]);

  const reload = useCallback(() => router.invalidate(), [router]);

  // ---- create ------------------------------------------------------------
  const [createOpen, setCreateOpen] = useState(create === 1);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | undefined>();
  useEffect(() => {
    if (create === 1) setCreateOpen(true);
  }, [create]);

  const openCreate = useCallback(() => {
    setCreateError(undefined);
    setCreateOpen(true);
  }, []);

  // Closing also clears `?create=1`, so a refresh does not reopen a dialog
  // the operator already dismissed.
  const closeCreate = useCallback(() => {
    setCreateOpen(false);
    setCreateError(undefined);
    if (create === 1) {
      void navigate({ to: "/operator/tenants", search: {}, replace: true });
    }
  }, [create, navigate]);

  const handleCreate = useCallback(
    async (id: string) => {
      setCreating(true);
      setCreateError(undefined);
      const result = await tenantApi.create(id);
      setCreating(false);
      if (!result.ok) {
        setCreateError(result.error);
        return;
      }
      toast.success(`Created tenant ${id}`);
      closeCreate();
      await reload();
    },
    [closeCreate, reload],
  );

  // ---- open / copy -------------------------------------------------------
  // This page has no detail drawer, so opening a tenant means going to its
  // data rather than revealing a panel beside the table.
  const openTenant = useCallback(
    (tenantId: string) => {
      void navigate({ to: "/developer/storage", search: { as: tenantId } });
    },
    [navigate],
  );

  const copyTenantId = useCallback(async (tenantId: string) => {
    try {
      await navigator.clipboard.writeText(tenantId);
      toast("Copied tenant id", { description: tenantId });
    } catch {
      toast.error("Clipboard is unavailable in this browser context.");
    }
  }, []);

  // ---- delete ------------------------------------------------------------
  const [confirmTenant, setConfirmTenant] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | undefined>();
  const confirmRow = rows.find((row) => row.tenantId === confirmTenant);

  const askDelete = useCallback((tenantId: string) => {
    setDeleteError(undefined);
    setConfirmTenant(tenantId);
  }, []);

  const runDelete = useCallback(
    async (id: string) => {
      setDeleting(true);
      setDeleteError(undefined);
      const result = await tenantApi.remove(id);
      setDeleting(false);
      if (!result.ok) {
        // The refusal stays next to the control that drew it; the dialog
        // is still open with the same tenant named.
        setDeleteError(result.error);
        return;
      }
      toast.success(`Deleted tenant ${id}`);
      setConfirmTenant(null);
      await reload();
    },
    [reload],
  );

  // ---- row menu ----------------------------------------------------------
  const [menu, setMenu] = useState<MenuState | null>(null);
  const menuItems = useCallback(
    (row: TenantRow): RowMenuItem[] => [
      {
        id: "open",
        label: "Open storage",
        onSelect: () => openTenant(row.tenantId),
      },
      {
        id: "copy",
        label: "Copy tenant id",
        hint: row.tenantId,
        onSelect: () => void copyTenantId(row.tenantId),
      },
      {
        id: "delete",
        label: "Delete tenant…",
        danger: true,
        onSelect: () => askDelete(row.tenantId),
      },
    ],
    [askDelete, copyTenantId, openTenant],
  );

  const columns = useMemo(
    () => [
      col.accessor("tenantId", {
        header: "Tenant",
        size: 320,
        cell: (ctx) => {
          const id = ctx.getValue();
          return (
            <span className="flex items-center gap-2">
              <span className="truncate font-mono text-text-1">{id}</span>
              <CopyChip
                label="tenant id"
                value={id}
                hideUntilHover
                testid={`tenants-copy-${id}`}
              >
                copy
              </CopyChip>
            </span>
          );
        },
      }),
      col.accessor("tableCount", {
        header: () => <span className="block text-right">Tables</span>,
        size: 96,
        cell: (ctx) => <NumberCell value={ctx.getValue()} />,
      }),
      col.accessor("totalRows", {
        header: () => <span className="block text-right">Rows</span>,
        size: 112,
        cell: (ctx) => <NumberCell value={ctx.getValue()} />,
      }),
      col.display({
        id: "actions",
        header: () => <span className="sr-only">Actions</span>,
        size: 48,
        enableSorting: false,
        cell: (ctx) => {
          const row = ctx.row.original;
          return (
            <span className="flex justify-end">
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={`Actions for ${row.tenantId}`}
                data-testid={`tenants-row-actions-${row.tenantId}`}
                onClick={(event) => {
                  const rect = event.currentTarget.getBoundingClientRect();
                  setMenu({
                    row,
                    x: rect.left,
                    y: rect.bottom + 2,
                    element: event.currentTarget,
                  });
                }}
              >
                <Ellipsis />
              </Button>
            </span>
          );
        },
      }),
    ],
    [],
  );

  const subPanelSpec = useMemo<SubPanelSpec>(
    () => ({
      kind: "dynamic",
      title: "Tenants",
      search: { placeholder: "Filter tenants", rows: tenants.length },
      children:
        tenants.length === 0 ? (
          <div className="px-3 py-6 text-xs text-text-3">
            <p>No tenants yet.</p>
            <p className="mt-2">Create tenant adds the first one.</p>
          </div>
        ) : (
          <ul className="flex flex-col gap-px px-2 py-2">
            {tenants.map((tenantId) => (
              <li key={tenantId}>
                <Link
                  to="/developer/storage"
                  search={{ as: tenantId }}
                  data-testid={`sub-panel-item-op-${tenantId}`}
                  className="flex h-8 items-center rounded-md px-2 text-sm text-text-3 hover:bg-bg-raised hover:text-text-1"
                >
                  <span className="flex-1 truncate font-mono text-xs">
                    {tenantId}
                  </span>
                </Link>
              </li>
            ))}
          </ul>
        ),
    }),
    [tenants],
  );
  useContributeSubPanel(subPanelSpec);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-tenants"
    >
      <PageHeader
        title="Tenants"
        subtitle={
          <>
            Tenants own tables and documents. The{" "}
            <code className="font-mono text-text-1">_nimbus</code> system tenant
            is operator-only and not listed here.
          </>
        }
        trailing={
          <Button size="sm" onClick={openCreate} data-testid="tenants-create">
            <Plus data-icon="inline-start" />
            Create tenant
          </Button>
        }
      />

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-md border border-border-2 bg-bg-panel">
        {serverError ? (
          <EmptyState
            title="Tenants endpoint unavailable"
            body={
              <>
                This deployment cannot reach{" "}
                <code className="font-mono text-text-1">/api/tenants</code>:{" "}
                <span
                  className="font-mono text-text-1"
                  data-testid="tenants-error"
                >
                  {serverError}
                </span>
                . The server may be offline, or this build does not ship the
                tenants endpoint.
              </>
            }
            cta={{ label: "Retry", onClick: () => void reload() }}
            testid="tenants-error-envelope"
          />
        ) : rows.length === 0 ? (
          <EmptyState
            title="No tenants yet"
            body="A tenant owns tables, documents, files, and services. Create one here or with the API, and the console starts writing data to it."
            cta={{ label: "Create tenant", onClick: openCreate }}
            snippet={createTenantCommand({ serverUrl })}
            testid="tenants-empty"
          />
        ) : (
          <DataTable
            columns={columns}
            data={rows}
            getRowId={(row) => row.tenantId}
            ariaLabel="Tenants"
            onRowActivate={(row) => openTenant(row.tenantId)}
            onRowContextMenu={(row, anchor) => setMenu({ ...anchor, row })}
            rowTestid={(row) => `tenants-row-${row.tenantId}`}
            testid="tenants-table"
            className="min-h-0 flex-1"
          />
        )}
      </div>

      {menu ? (
        <RowContextMenu
          x={menu.x}
          y={menu.y}
          label={`Tenant ${menu.row.tenantId} actions`}
          items={menuItems(menu.row)}
          restoreFocus={menu.element}
          onClose={() => setMenu(null)}
          testid="tenants-row-menu"
        />
      ) : null}

      <CreateTenantDialog
        open={createOpen}
        busy={creating}
        error={createError}
        onSubmit={(id) => void handleCreate(id)}
        onCancel={closeCreate}
      />

      <ConfirmDialog
        open={confirmTenant !== null}
        title={
          confirmTenant ? `Delete tenant "${confirmTenant}"?` : "Delete tenant?"
        }
        description={
          confirmRow && confirmRow.tableCount > 0 ? (
            <>
              This removes{" "}
              <span className="font-mono text-text-1 tabular">
                {confirmRow.tableCount}
              </span>{" "}
              table{confirmRow.tableCount === 1 ? "" : "s"} and every document
              in them.
            </>
          ) : (
            "The tenant has no tables."
          )
        }
        confirmLabel="Delete"
        danger
        busy={deleting}
        typedConfirmation={
          confirmRow && confirmRow.tableCount > 0 && confirmTenant
            ? { phrase: confirmTenant }
            : undefined
        }
        error={deleteError}
        onCancel={() => {
          if (!deleting) setConfirmTenant(null);
        }}
        onConfirm={() => {
          if (confirmTenant) void runDelete(confirmTenant);
        }}
        testid="tenants-delete-dialog"
      />
    </section>
  );
}

function NumberCell({ value }: { value: number }) {
  return (
    <span className="block text-right font-mono text-xs tabular text-text-2">
      {value}
    </span>
  );
}
