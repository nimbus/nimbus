import { createFileRoute, Link, useRouter } from "@tanstack/react-router";
import { useCallback, useMemo, useState } from "react";
import { toast } from "sonner";

import { api } from "../../../convex/_generated/api";
import { ConfirmDialog } from "../../components/confirm-dialog";
import { CopyChip } from "../../components/copy-chip";
import { EmptyState } from "../../components/empty-state";
import { cn } from "../../lib/cn";
import { getNimbusClient } from "../../lib/nimbus-client";
import { fetchTenants } from "../../shell/tenants-fetch";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";

type TenantsSearch = {
  create?: 1;
};

type TableDoc = {
  _id: string;
  tenantId?: string;
  name?: string;
  rowCount?: number;
  lastWriteAt?: number;
};

type TenantRow = {
  tenantId: string;
  tableCount: number;
  totalRows: number;
};

type LoaderResult =
  | { kind: "ok"; tenants: string[]; tables: TableDoc[] }
  | { kind: "error"; message: string };

export const Route = createFileRoute("/admin/tenants")({
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

function TenantsPage() {
  const data = Route.useLoaderData();
  const router = useRouter();
  const tenants = data.kind === "ok" ? data.tenants : [];
  const tables = data.kind === "ok" ? data.tables : [];
  const serverError = data.kind === "error" ? data.message : null;

  const [creating, setCreating] = useState(false);
  const [newTenant, setNewTenant] = useState("");
  const [deletingTenant, setDeletingTenant] = useState<string | null>(null);
  const [confirmTenant, setConfirmTenant] = useState<string | null>(null);

  const rows: TenantRow[] = useMemo(() => {
    const byTenant = new Map<string, { count: number; rows: number }>();
    for (const t of tables) {
      if (!t.tenantId) continue;
      const entry = byTenant.get(t.tenantId) ?? { count: 0, rows: 0 };
      entry.count += 1;
      entry.rows += t.rowCount ?? 0;
      byTenant.set(t.tenantId, entry);
    }
    const ids = new Set<string>([...tenants, ...byTenant.keys()]);
    return Array.from(ids)
      .sort()
      .map((id) => ({
        tenantId: id,
        tableCount: byTenant.get(id)?.count ?? 0,
        totalRows: byTenant.get(id)?.rows ?? 0,
      }));
  }, [tenants, tables]);

  const reload = useCallback(() => {
    void router.invalidate();
  }, [router]);

  const handleCreate = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      const id = newTenant.trim();
      if (!id) return;
      setCreating(true);
      try {
        const response = await fetch("/api/tenants", {
          method: "POST",
          credentials: "include",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ id }),
        });
        if (!response.ok) {
          const body = (await response.json().catch(() => null)) as {
            error?: { message?: string };
          } | null;
          throw new Error(
            body?.error?.message ?? `Create failed: ${response.status}`,
          );
        }
        toast.success(`Created tenant ${id}`);
        setNewTenant("");
        reload();
      } catch (err) {
        toast.error(
          err instanceof Error ? err.message : "Failed to create tenant",
        );
      } finally {
        setCreating(false);
      }
    },
    [newTenant, reload],
  );

  const confirmTenantRow = rows.find((r) => r.tenantId === confirmTenant);

  const runDelete = useCallback(
    async (id: string) => {
      setDeletingTenant(id);
      setConfirmTenant(null);
      try {
        const response = await fetch(
          `/api/tenants/${encodeURIComponent(id)}`,
          {
            method: "DELETE",
            credentials: "include",
          },
        );
        if (!response.ok) {
          const body = (await response.json().catch(() => null)) as {
            error?: { message?: string };
          } | null;
          throw new Error(
            body?.error?.message ?? `Delete failed: ${response.status}`,
          );
        }
        toast.success(`Deleted tenant ${id}`);
        reload();
      } catch (err) {
        toast.error(
          err instanceof Error ? err.message : "Failed to delete tenant",
        );
      } finally {
        setDeletingTenant(null);
      }
    },
    [reload],
  );

  const subDrawerSpec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Tenants",
      search: { placeholder: "Filter tenants" },
      children:
        tenants.length === 0 ? (
          <div className="px-3 py-6 text-xs text-muted">
            <p>No tenants yet.</p>
            <p className="mt-2">Use Create tenant above to add one.</p>
          </div>
        ) : (
          <ul className="flex flex-col gap-px px-2 py-2">
            {tenants.map((tenantId) => (
              <li key={tenantId}>
                <a
                  href={`/admin/tenants?selected=${tenantId}`}
                  data-testid={`sub-drawer-item-op-${tenantId}`}
                  className="flex h-8 items-center rounded-md px-2 text-sm text-muted hover:bg-surface-2 hover:text-default"
                >
                  <span className="flex-1 truncate font-mono text-xs">
                    {tenantId}
                  </span>
                </a>
              </li>
            ))}
          </ul>
        ),
    }),
    [tenants],
  );
  useContributeSubDrawer(subDrawerSpec);

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-storage"
    >
      <header className="flex flex-col gap-2">
        <div className="flex items-baseline justify-between">
          <div>
            <h1 className="text-default" style={{ fontSize: "var(--text-xl)" }}>
              Tenants
            </h1>
            <p className="text-sm text-muted">
              Tenants own tables and documents. The{" "}
              <code className="font-mono text-default">_nimbus</code> system
              tenant is operator-only and not listed here.
            </p>
          </div>
          <form
            onSubmit={handleCreate}
            className="flex items-center gap-2"
            data-testid="storage-create-form"
          >
            <label htmlFor="storage-create-id" className="sr-only">
              New tenant id
            </label>
            <input
              id="storage-create-id"
              type="text"
              value={newTenant}
              onChange={(e) => setNewTenant(e.target.value)}
              placeholder="tenant-id"
              className="rounded border border-app bg-surface px-2 py-1 font-mono text-xs text-default placeholder:text-muted focus-visible:border-strong"
              data-testid="storage-create-input"
              disabled={creating}
            />
            <button
              type="submit"
              disabled={creating || !newTenant.trim()}
              className={cn(
                "rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide",
                creating || !newTenant.trim()
                  ? "text-muted"
                  : "text-default hover:bg-surface",
              )}
              data-testid="storage-create-submit"
            >
              {creating ? "creating…" : "create tenant"}
            </button>
          </form>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        {serverError ? (
          <EmptyState
            title="Tenants endpoint unavailable"
            body={
              <>
                This deployment can&apos;t reach{" "}
                <code className="font-mono text-default">/api/tenants</code>:{" "}
                <span
                  className="font-mono text-default"
                  data-testid="storage-server-error"
                >
                  {serverError}
                </span>
                . The server may be offline or this build doesn&apos;t ship the
                tenants endpoint.
              </>
            }
            cta={{
              label: "Retry",
              onClick: reload,
            }}
            testid="storage-server-error-envelope"
          />
        ) : rows.length === 0 ? (
          <Empty
            title="No tenants"
            detail="Use the form above or POST /api/tenants to create your first tenant. Tables and documents live inside tenants."
          />
        ) : (
          <div className="overflow-auto">
            <table
              className="w-full border-collapse text-sm"
              data-testid="storage-tenants-table"
            >
              <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
                <tr>
                  <Th>Tenant</Th>
                  <Th align="right">Tables</Th>
                  <Th align="right">Rows</Th>
                  <Th align="right">Actions</Th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => (
                  <tr
                    key={row.tenantId}
                    className="border-t border-app hover:bg-surface-2"
                    data-testid={`storage-tenant-row-${row.tenantId}`}
                  >
                    <Td>
                      <Link
                        to="/app/storage"
                        search={{ as: row.tenantId }}
                        className="font-mono text-default hover:underline"
                        data-testid={`storage-tenant-link-${row.tenantId}`}
                      >
                        {row.tenantId}
                      </Link>
                      <span className="ml-2 align-middle">
                        <CopyChip
                          label="tenant id"
                          value={row.tenantId}
                          hideUntilHover
                          testid={`storage-tenant-copy-${row.tenantId}`}
                        >
                          copy
                        </CopyChip>
                      </span>
                    </Td>
                    <Td align="right" mono>
                      {row.tableCount}
                    </Td>
                    <Td align="right" mono>
                      {row.totalRows}
                    </Td>
                    <Td align="right">
                      <button
                        type="button"
                        onClick={() => setConfirmTenant(row.tenantId)}
                        disabled={deletingTenant === row.tenantId}
                        className={cn(
                          "rounded border border-app px-2 py-0.5 font-mono text-[11px] uppercase tracking-wide",
                          deletingTenant === row.tenantId
                            ? "text-muted"
                            : "text-danger hover:bg-surface-2",
                        )}
                        data-testid={`storage-tenant-delete-${row.tenantId}`}
                      >
                        {deletingTenant === row.tenantId
                          ? "deleting…"
                          : "delete"}
                      </button>
                    </Td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <ConfirmDialog
        open={confirmTenant !== null}
        title={
          confirmTenant ? `Delete tenant "${confirmTenant}"?` : "Delete tenant?"
        }
        description={
          confirmTenantRow && confirmTenantRow.tableCount > 0 ? (
            <p>
              This removes{" "}
              <span className="font-mono text-default tabular">
                {confirmTenantRow.tableCount}
              </span>{" "}
              table{confirmTenantRow.tableCount === 1 ? "" : "s"} and all
              documents. This action cannot be undone.
            </p>
          ) : (
            <p>The tenant has no tables. This action cannot be undone.</p>
          )
        }
        confirmLabel="Delete"
        danger
        busy={deletingTenant !== null}
        onCancel={() => setConfirmTenant(null)}
        onConfirm={() => {
          if (confirmTenant) void runDelete(confirmTenant);
        }}
        testid="storage-delete-tenant-dialog"
      />
    </section>
  );
}

function Th({
  children,
  align = "left",
}: {
  children: React.ReactNode;
  align?: "left" | "right";
}) {
  return (
    <th
      className={cn(
        "px-3 py-2 font-semibold",
        align === "right" ? "text-right" : "text-left",
      )}
    >
      {children}
    </th>
  );
}

function Td({
  children,
  align = "left",
  mono,
}: {
  children: React.ReactNode;
  align?: "left" | "right";
  mono?: boolean;
}) {
  return (
    <td
      className={cn(
        "px-3 py-2 text-default",
        align === "right" ? "text-right" : "text-left",
        mono && "font-mono tabular",
      )}
    >
      {children}
    </td>
  );
}

function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div
      className="flex h-full flex-col items-center justify-center gap-2 px-6 py-10 text-center"
      data-testid="storage-empty"
    >
      <p className="font-mono text-sm text-default">{title}</p>
      <p className="max-w-md text-xs text-muted">{detail}</p>
    </div>
  );
}
