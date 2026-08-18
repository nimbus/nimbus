import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

import { api } from "../../../convex/_generated/api";
import { Breadcrumb } from "../../components/breadcrumb";
import { ConfirmDialog } from "../../components/confirm-dialog";
import { EmptyState } from "../../components/empty-state";
import { LoadingState } from "../../components/loading-state";
import { PageHeader } from "../../components/page-header";
import { BulkToolbar } from "../../components/storage/bulk-toolbar";
import { ColumnChooser } from "../../components/storage/column-chooser";
import { DocumentsTable } from "../../components/storage/documents-table";
import { EditDrawer } from "../../components/storage/edit-drawer";
import { IndexPanel } from "../../components/storage/index-panel";
import { InsertDrawer } from "../../components/storage/insert-drawer";
import { PageError } from "../../components/storage/page-error";
import { QueryBar } from "../../components/storage/query-bar";
import { SchemaPanel } from "../../components/storage/schema-panel";
import {
  type DocumentFilter,
  type DocumentOrder,
  indexBackedFields,
  parseFilters,
  parseOrder,
} from "../../components/storage/table-query";
import { useTablesSubDrawer } from "../../components/storage/tables-sub-drawer";
import {
  resolveColumns,
  useColumnPrefs,
  useDiscoveredFields,
} from "../../components/storage/use-column-prefs";
import { useDocumentPage } from "../../components/storage/use-document-page";
import { documents } from "../../lib/api-mutations";
import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import type { DocumentJson, TableDoc } from "../../lib/types/table";
import { useUiStore } from "../../store/ui-store";

export const Route = createFileRoute("/developer/storage_/$table")({
  // URL is state (DESIGN.md §Cross-Screen Rules): the filter set and the sort
  // order live here so a filtered view survives a refresh and can be shared.
  validateSearch: (search: Record<string, unknown>): TableSearch => {
    const filters = parseFilters(search.filters);
    return {
      panel:
        search.panel === "schema" || search.panel === "indexes"
          ? search.panel
          : undefined,
      sort:
        typeof search.sort === "string" && search.sort !== ""
          ? search.sort
          : undefined,
      dir:
        search.dir === "desc"
          ? "desc"
          : search.dir === "asc"
            ? "asc"
            : undefined,
      filters: filters.length > 0 ? filters : undefined,
    };
  },
  component: TableDocumentsPage,
});

type TableSearch = {
  panel?: "schema" | "indexes";
  sort?: string;
  dir?: "asc" | "desc";
  filters?: DocumentFilter[];
};

const NO_FILTERS: DocumentFilter[] = [];
/** IDs listed by name in the bulk-delete confirmation before it elides. */
const CONFIRM_ID_PREVIEW = 5;

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

  // Table-to-table switching is the most repeated action in a data browser, so
  // the detail route contributes the same Tables drawer the list route does.
  const tables = useQuery(
    api.tables.list,
    tenant ? { tenantId: tenant, limit: 200 } : "skip",
  ) as TableDoc[] | undefined;
  useTablesSubDrawer({
    tenant: tenant || null,
    tables,
    hasTenants: undefined,
  });

  const filters = search.filters ?? NO_FILTERS;
  const order = useMemo(
    () => parseOrder(search.sort, search.dir),
    [search.sort, search.dir],
  );

  const {
    page,
    loading,
    pageError,
    cursorStack,
    refresh,
    onNext,
    onPrev,
    reset,
  } = useDocumentPage(tenant, table, { filters, order });

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [showInsert, setShowInsert] = useState(false);
  const [editing, setEditing] = useState<DocumentJson | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string[] | null>(null);
  const [deletingDocs, setDeletingDocs] = useState(false);
  const [pendingScanSort, setPendingScanSort] = useState<string | null>(null);

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

  // ESC is the universal way out of an armed destructive state, and the armed
  // state here is a bulk delete. The listener self-suppresses instead of
  // relying on shadowing: `Slideover`, `ConfirmDialog`, and the shell keyboard
  // contract all listen on `window` without stopping propagation, so without
  // these guards one Escape would clear the selection *and* close a drawer.
  useEffect(() => {
    if (selected.size === 0) return;
    if (showInsert || editing || confirmDelete) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const { paletteOpen, lensOpen, actionMenuOpen } = useUiStore.getState();
      if (paletteOpen || lensOpen || actionMenuOpen) return;
      if (event.defaultPrevented) return;
      setSelected(new Set());
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selected.size, showInsert, editing, confirmDelete]);

  const schemaFields = useMemo(
    () =>
      (tableMeta?.schema?.fields ?? [])
        .map((f) => f.name)
        .filter((name): name is string => Boolean(name)),
    [tableMeta],
  );
  const discovered = useDiscoveredFields(tenant, table, page?.data);
  // With a schema the field list is authoritative. Without one it is only the
  // fields seen so far — never a hard-capped slice of the current page, which
  // is how an operator concludes a field does not exist.
  const availableFields = schemaFields.length > 0 ? schemaFields : discovered;

  const {
    prefs,
    setHidden,
    moveColumn,
    reset: resetColumns,
  } = useColumnPrefs(tenant, table);
  const columns = useMemo(
    () => resolveColumns(availableFields, prefs),
    [availableFields, prefs],
  );
  const indexBacked = useMemo(
    () => indexBackedFields(tableMeta?.schema ?? null),
    [tableMeta],
  );

  // Every search write goes through the updater form: replacing the whole
  // search object would silently drop the other params (a filter change would
  // close the schema panel, a panel toggle would drop the sort).
  const patchSearch = useCallback(
    (patch: Partial<TableSearch>) => {
      void navigate({ search: (prev: TableSearch) => ({ ...prev, ...patch }) });
    },
    [navigate],
  );

  const applyFilters = useCallback(
    (next: DocumentFilter[]) => {
      patchSearch({ filters: next.length > 0 ? next : undefined });
      setSelected(new Set());
    },
    [patchSearch],
  );

  const applyOrder = useCallback(
    (next: DocumentOrder | null) => {
      patchSearch({ sort: next?.field, dir: next?.direction });
      setPendingScanSort(null);
      setSelected(new Set());
    },
    [patchSearch],
  );

  const requestSort = useCallback(
    (field: string) => {
      // Re-clicking the active column only flips direction — the scan cost was
      // already accepted when that sort was applied.
      if (order?.field === field) {
        applyOrder({
          field,
          direction: order.direction === "asc" ? "desc" : "asc",
        });
        return;
      }
      if (!indexBacked.has(field)) {
        setPendingScanSort(field);
        return;
      }
      applyOrder({ field, direction: "asc" });
    },
    [order, indexBacked, applyOrder],
  );

  const togglePanel = useCallback(
    (panel: "schema" | "indexes" | undefined) => {
      patchSearch({ panel: search.panel === panel ? undefined : panel });
    },
    [patchSearch, search.panel],
  );

  const filterFields = useMemo(
    () => ["_id", ...availableFields],
    [availableFields],
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
                  "rounded border border-app px-2 py-1 font-mono text-xs uppercase tracking-wide hover:bg-surface",
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
                  "rounded border border-app px-2 py-1 font-mono text-xs uppercase tracking-wide hover:bg-surface",
                  search.panel === "indexes"
                    ? "bg-surface text-default"
                    : "text-muted hover:text-default",
                )}
                data-testid="documents-toggle-indexes"
              >
                indexes
              </button>
              {/* Bulk delete lives in the selection toolbar above the rows it
                  acts on, not in a page-level button — two competing delete
                  affordances is how an operator deletes the wrong set. */}
              <button
                type="button"
                onClick={() => setShowInsert(true)}
                className="rounded border border-app px-2 py-1 font-mono text-xs uppercase tracking-wide text-default hover:bg-surface"
                data-testid="documents-open-insert"
              >
                insert
              </button>
            </div>
          }
        />
      </div>

      <div className="flex min-h-0 flex-1 gap-4 overflow-hidden">
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-md border border-app bg-surface">
          <QueryBar
            fields={filterFields}
            filters={filters}
            order={order}
            indexBacked={indexBacked}
            pendingScanSort={pendingScanSort}
            onFiltersChange={applyFilters}
            onOrderChange={applyOrder}
            onConfirmScanSort={() => {
              if (pendingScanSort) {
                applyOrder({ field: pendingScanSort, direction: "asc" });
              }
            }}
            onCancelScanSort={() => setPendingScanSort(null)}
            trailing={
              <ColumnChooser
                available={availableFields}
                visible={columns}
                fromSchema={schemaFields.length > 0}
                // The chooser reports visibility; the store records hiding.
                onToggle={(field, visible) => setHidden(field, !visible)}
                onMove={(field, delta) => moveColumn(columns, field, delta)}
                onReset={resetColumns}
              />
            }
          />
          {selected.size > 0 ? (
            <BulkToolbar
              count={selected.size}
              onDelete={() => handleDelete(Array.from(selected))}
              onClear={() => setSelected(new Set())}
            />
          ) : null}
          {!tenant ? (
            <EmptyState
              title="Select a tenant"
              body="Documents scope to a tenant. Pick one from the top-nav selector to browse this table."
              testid="documents-empty"
            />
          ) : loading && !page ? (
            <LoadingState label="Loading documents…" />
          ) : pageError ? (
            <PageError message={pageError} onRetry={refresh} />
          ) : !page || page.data.length === 0 ? (
            filters.length > 0 ? (
              <EmptyState
                title="No documents match the filter"
                body="Remove a filter chip above to widen the query, or insert a document that satisfies it."
                testid="documents-empty"
              />
            ) : (
              <EmptyState
                title="No documents"
                body={`Insert a document using the toolbar or POST /api/tenants/${tenant}/documents with body { table: "${table}", fields: {...} }.`}
                testid="documents-empty"
              />
            )
          ) : (
            <DocumentsTable
              page={page}
              columns={columns}
              selected={selected}
              cursorStack={cursorStack}
              order={order}
              indexBacked={indexBacked}
              onSort={requestSort}
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
            {/* A bulk confirm that names no document tells the operator
                nothing about what is about to be destroyed. */}
            {confirmDelete && confirmDelete.length > 0 ? (
              <ul
                className="font-mono text-xs text-muted"
                data-testid="documents-delete-ids"
              >
                {confirmDelete.slice(0, CONFIRM_ID_PREVIEW).map((id) => (
                  <li key={id} className="truncate">
                    {id}
                  </li>
                ))}
                {confirmDelete.length > CONFIRM_ID_PREVIEW ? (
                  <li>and {confirmDelete.length - CONFIRM_ID_PREVIEW} more</li>
                ) : null}
              </ul>
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
