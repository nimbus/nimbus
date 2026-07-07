import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute } from "@tanstack/react-router";
import { useCallback, useMemo, useState } from "react";
import { toast } from "sonner";

import { api } from "../../../convex/_generated/api";
import { Breadcrumb } from "../../components/breadcrumb";
import { ConfirmDialog } from "../../components/confirm-dialog";
import { EmptyState } from "../../components/empty-state";
import { LoadingState } from "../../components/loading-state";
import { PageHeader } from "../../components/page-header";
import { DocumentsTable } from "../../components/storage/documents-table";
import { EditDrawer } from "../../components/storage/edit-drawer";
import { IndexPanel } from "../../components/storage/index-panel";
import { InsertDrawer } from "../../components/storage/insert-drawer";
import { PageError } from "../../components/storage/page-error";
import { SchemaPanel } from "../../components/storage/schema-panel";
import { useTableDocuments } from "../../hooks/use-table-documents";
import { documents } from "../../lib/api-mutations";
import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import type { DocumentJson, TableDoc } from "../../lib/types/table";
import { useUiStore } from "../../store/ui-store";

export const Route = createFileRoute("/developer/storage_/$table")({
  validateSearch: (search: Record<string, unknown>): TableSearch => ({
    panel:
      search.panel === "schema" || search.panel === "indexes"
        ? search.panel
        : undefined,
  }),
  component: TableDocumentsPage,
});

type TableSearch = {
  panel?: "schema" | "indexes";
};

// Parse a drawer's JSON draft into a document object, throwing a readable error
// (surfaced by the drawer's form) for anything that is not a JSON object.
function parseObjectJson(json: string, noun: string): Record<string, unknown> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch (err) {
    throw new Error(`Invalid JSON: ${(err as Error).message}`);
  }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error(`${noun} must be a JSON object`);
  }
  return parsed as Record<string, unknown>;
}

function TableDocumentsPage() {
  const { table } = Route.useParams();
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
  const tenant = useUiStore((s) => s.activeTenant) ?? "";

  const tableMeta = useQuery(
    api.tables.byName,
    tenant ? { tenantId: tenant, name: table } : "skip",
  ) as TableDoc | null | undefined;

  const { page, loading, pageError, cursorStack, refresh, onNext, onPrev, reset } =
    useTableDocuments(tenant, table);

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [showInsert, setShowInsert] = useState(false);
  const [editing, setEditing] = useState<DocumentJson | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string[] | null>(null);
  const [deletingDocs, setDeletingDocs] = useState(false);

  const goNext = useCallback(() => {
    onNext();
    setSelected(new Set());
  }, [onNext]);

  const goPrev = useCallback(() => {
    onPrev();
    setSelected(new Set());
  }, [onPrev]);

  const handleInsert = useCallback(
    async (json: string) => {
      const fields = parseObjectJson(json, "Document");
      const result = await documents.insert(tenant, table, fields);
      if (!result.ok) throw new Error(result.error);
      toast.success("Document inserted");
      setSelected(new Set());
      reset();
    },
    [tenant, table, reset],
  );

  const handleUpdate = useCallback(
    async (id: string, json: string) => {
      const patch = parseObjectJson(json, "Patch");
      const result = await documents.update(tenant, table, id, patch);
      if (!result.ok) throw new Error(result.error);
      toast.success("Document updated");
      setEditing(null);
      refresh();
    },
    [tenant, table, refresh],
  );

  const handleDelete = useCallback((ids: string[]) => {
    if (ids.length === 0) return;
    setConfirmDelete(ids);
  }, []);

  const runDelete = useCallback(
    async (ids: string[]) => {
      setDeletingDocs(true);
      let failed = 0;
      for (const id of ids) {
        const result = await documents.remove(tenant, table, id);
        if (!result.ok) failed += 1;
      }
      setDeletingDocs(false);
      setConfirmDelete(null);
      if (failed === 0) {
        toast.success(
          `Deleted ${ids.length} document${ids.length === 1 ? "" : "s"}`,
        );
      } else {
        toast.error(`Deleted ${ids.length - failed}/${ids.length} documents`);
      }
      setSelected(new Set());
      refresh();
    },
    [tenant, table, refresh],
  );

  const columns = useMemo(() => {
    const fromSchema = (tableMeta?.schema?.fields ?? [])
      .map((f) => f.name)
      .filter((name): name is string => Boolean(name));
    if (fromSchema.length > 0) {
      return ["_id", ...fromSchema];
    }
    const fromData = new Set<string>();
    (page?.data ?? []).forEach((doc) => {
      for (const key of Object.keys(doc)) {
        if (key.startsWith("_")) continue;
        fromData.add(key);
      }
    });
    return ["_id", ...Array.from(fromData).slice(0, 8)];
  }, [tableMeta, page]);

  const togglePanel = useCallback(
    (panel: "schema" | "indexes" | undefined) => {
      void navigate({
        search: {
          panel: search.panel === panel ? undefined : panel,
        },
      });
    },
    [navigate, search.panel],
  );

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-table-documents"
    >
      <div className="flex flex-col gap-2">
        <Breadcrumb
          segments={[
            { label: "storage", href: "/developer/storage" },
            ...(tenant
              ? [
                  {
                    label: tenant,
                    href: "/developer/storage",
                    copyValue: tenant,
                    copyLabel: "tenant id",
                  },
                ]
              : []),
            {
              label: table,
              copyValue: table,
              copyLabel: "table",
              active: true,
            },
          ]}
          testid="documents-breadcrumb"
        />
        <PageHeader
          title={table}
          subtitle={
            tableMeta?.schema
              ? "Schema enforced. Inserts validated before write."
              : "Schemaless table — any document shape is accepted."
          }
          trailing={
            <div
              className="flex items-center gap-2"
              data-testid="documents-toolbar"
            >
              <button
                type="button"
                onClick={() => togglePanel("schema")}
                className={cn(
                  "rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide hover:bg-surface",
                  search.panel === "schema"
                    ? "bg-surface text-default"
                    : "text-muted hover:text-default",
                )}
                data-testid="documents-toggle-schema"
              >
                schema
              </button>
              <button
                type="button"
                onClick={() => togglePanel("indexes")}
                className={cn(
                  "rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide hover:bg-surface",
                  search.panel === "indexes"
                    ? "bg-surface text-default"
                    : "text-muted hover:text-default",
                )}
                data-testid="documents-toggle-indexes"
              >
                indexes
              </button>
              <button
                type="button"
                onClick={() => setShowInsert(true)}
                className="rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide text-default hover:bg-surface"
                data-testid="documents-open-insert"
              >
                insert
              </button>
              <button
                type="button"
                onClick={() => void handleDelete(Array.from(selected))}
                disabled={selected.size === 0}
                className={cn(
                  "rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide",
                  selected.size === 0
                    ? "text-muted"
                    : "text-danger hover:bg-surface",
                )}
                data-testid="documents-bulk-delete"
              >
                delete{selected.size > 0 ? ` (${selected.size})` : ""}
              </button>
            </div>
          }
        />
      </div>

      <div className="flex min-h-0 flex-1 gap-4 overflow-hidden">
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-md border border-app bg-surface">
          {loading && !page ? (
            <LoadingState label="Loading documents…" />
          ) : pageError ? (
            <PageError message={pageError} onRetry={refresh} />
          ) : !page || page.data.length === 0 ? (
            <EmptyState
              title="No documents"
              body={`Insert a document using the toolbar or POST /api/tenants/${tenant}/documents with body { table: "${table}", fields: {...} }.`}
              testid="documents-empty"
            />
          ) : (
            <DocumentsTable
              page={page}
              columns={columns}
              selected={selected}
              cursorStack={cursorStack}
              onToggleAll={(checked) =>
                setSelected(
                  checked
                    ? new Set(
                        page.data
                          .map((d) => String(d._id ?? ""))
                          .filter(Boolean),
                      )
                    : new Set(),
                )
              }
              onToggleOne={(id, checked) =>
                setSelected((prev) => {
                  const next = new Set(prev);
                  if (checked) next.add(id);
                  else next.delete(id);
                  return next;
                })
              }
              onEdit={setEditing}
              onDelete={handleDelete}
              onPrev={goPrev}
              onNext={goNext}
            />
          )}
        </div>

        {search.panel === "schema" ? (
          <SchemaPanel
            tenant={tenant}
            table={table}
            schema={tableMeta?.schema ?? null}
            onClose={() => togglePanel(undefined)}
            onSaved={refresh}
          />
        ) : null}
        {search.panel === "indexes" ? (
          <IndexPanel
            schema={tableMeta?.schema ?? null}
            onClose={() => togglePanel(undefined)}
          />
        ) : null}
      </div>

      {showInsert ? (
        <InsertDrawer
          onClose={() => setShowInsert(false)}
          onSubmit={handleInsert}
        />
      ) : null}
      {editing ? (
        <EditDrawer
          doc={editing}
          onClose={() => setEditing(null)}
          onSubmit={(json) => handleUpdate(String(editing._id ?? ""), json)}
        />
      ) : null}

      <ConfirmDialog
        open={confirmDelete !== null}
        title={
          confirmDelete && confirmDelete.length === 1
            ? `Delete document ${shortId(confirmDelete[0])}?`
            : `Delete ${confirmDelete?.length ?? 0} documents?`
        }
        description={
          <div className="space-y-2">
            <p>
              Removes{" "}
              <span className="font-mono text-default tabular">
                {confirmDelete?.length ?? 0}
              </span>{" "}
              document
              {confirmDelete && confirmDelete.length === 1 ? "" : "s"} from{" "}
              <span className="font-mono text-default">{table}</span>. This
              action cannot be undone.
            </p>
            {confirmDelete && confirmDelete.length === 1 ? (
              <p className="font-mono text-xs text-muted">{confirmDelete[0]}</p>
            ) : null}
          </div>
        }
        confirmLabel="Delete"
        danger
        busy={deletingDocs}
        onCancel={() => setConfirmDelete(null)}
        onConfirm={() => {
          if (confirmDelete) void runDelete(confirmDelete);
        }}
        testid="documents-delete-dialog"
      />
    </section>
  );
}
