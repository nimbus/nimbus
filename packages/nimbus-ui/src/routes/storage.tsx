import { createFileRoute, Link } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

import { api } from "../../convex/_generated/api";
import { Breadcrumb } from "../components/breadcrumb";
import { CopyChip } from "../components/copy-chip";
import { cn } from "../lib/cn";

export const Route = createFileRoute("/storage")({
  component: StoragePage,
});

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

type TenantListResponse = {
  tenants?: Array<string | { id?: string; tenantId?: string; name?: string }>;
};

function StoragePage() {
  const tables = useQuery(api.tables.list, {
    tenantId: null,
    limit: 200,
  }) as TableDoc[] | undefined;

  const [serverTenants, setServerTenants] = useState<string[] | null>(null);
  const [serverError, setServerError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [newTenant, setNewTenant] = useState("");
  const [deletingTenant, setDeletingTenant] = useState<string | null>(null);
  const [refreshTick, setRefreshTick] = useState(0);

  const reloadTenants = useCallback(async () => {
    setServerError(null);
    try {
      const response = await fetch("/api/tenants", {
        credentials: "include",
      });
      if (!response.ok) {
        const body = (await response.json().catch(() => null)) as {
          error?: { message?: string };
        } | null;
        throw new Error(
          body?.error?.message ?? `Request failed: ${response.status}`,
        );
      }
      const body = (await response.json()) as TenantListResponse;
      const ids = (body.tenants ?? [])
        .map((t) =>
          typeof t === "string" ? t : (t.tenantId ?? t.id ?? t.name ?? ""),
        )
        .filter(Boolean);
      setServerTenants(ids.sort());
    } catch (err) {
      setServerError(err instanceof Error ? err.message : String(err));
      setServerTenants([]);
    }
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: refreshTick is the manual refetch trigger
  useEffect(() => {
    void reloadTenants();
  }, [reloadTenants, refreshTick]);

  const rows: TenantRow[] = useMemo(() => {
    const byTenant = new Map<string, { count: number; rows: number }>();
    (tables ?? []).forEach((t) => {
      if (!t.tenantId) return;
      const entry = byTenant.get(t.tenantId) ?? { count: 0, rows: 0 };
      entry.count += 1;
      entry.rows += t.rowCount ?? 0;
      byTenant.set(t.tenantId, entry);
    });
    const ids = new Set<string>([
      ...(serverTenants ?? []),
      ...Array.from(byTenant.keys()),
    ]);
    return Array.from(ids)
      .sort()
      .map((id) => ({
        tenantId: id,
        tableCount: byTenant.get(id)?.count ?? 0,
        totalRows: byTenant.get(id)?.rows ?? 0,
      }));
  }, [serverTenants, tables]);

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
        setRefreshTick((t) => t + 1);
      } catch (err) {
        toast.error(
          err instanceof Error ? err.message : "Failed to create tenant",
        );
      } finally {
        setCreating(false);
      }
    },
    [newTenant],
  );

  const handleDelete = useCallback(
    async (id: string) => {
      const tableCount = rows.find((r) => r.tenantId === id)?.tableCount ?? 0;
      const warning =
        tableCount > 0
          ? `Delete tenant "${id}"? This removes ${tableCount} table${tableCount === 1 ? "" : "s"} and all documents.`
          : `Delete tenant "${id}"?`;
      if (!window.confirm(warning)) return;
      setDeletingTenant(id);
      try {
        const response = await fetch(`/api/tenants/${encodeURIComponent(id)}`, {
          method: "DELETE",
          credentials: "include",
        });
        if (!response.ok) {
          const body = (await response.json().catch(() => null)) as {
            error?: { message?: string };
          } | null;
          throw new Error(
            body?.error?.message ?? `Delete failed: ${response.status}`,
          );
        }
        toast.success(`Deleted tenant ${id}`);
        setRefreshTick((t) => t + 1);
      } catch (err) {
        toast.error(
          err instanceof Error ? err.message : "Failed to delete tenant",
        );
      } finally {
        setDeletingTenant(null);
      }
    },
    [rows],
  );

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-storage"
    >
      <header className="flex flex-col gap-2">
        <Breadcrumb
          segments={[{ label: "storage", active: true }]}
          testid="storage-breadcrumb"
        />
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
        {serverError ? (
          <p
            className="font-mono text-xs text-danger"
            data-testid="storage-server-error"
          >
            tenants endpoint: {serverError}
          </p>
        ) : null}
      </header>

      <div className="min-h-0 flex-1 overflow-hidden rounded-md border border-app bg-surface">
        {serverTenants === null && tables === undefined ? (
          <Loading label="Loading tenants…" />
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
                        to="/storage/$tenant"
                        params={{ tenant: row.tenantId }}
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
                        onClick={() => void handleDelete(row.tenantId)}
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

function Loading({ label }: { label: string }) {
  return (
    <div
      className="flex h-full items-center justify-center font-mono text-xs text-muted"
      data-testid="storage-loading"
    >
      {label}
    </div>
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
