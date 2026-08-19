import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute } from "@tanstack/react-router";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";

import { api } from "../../../convex/_generated/api";
import { Breadcrumb } from "../../components/breadcrumb";
import { ConfirmDialog } from "../../components/confirm-dialog";
import { EmptyState } from "../../components/empty-state";
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
import type {
  DocumentJson,
  PageResponse,
  TableDoc,
} from "../../lib/types/table";
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
      cursors: parseCursors(search.cursors),
    };
  },
  component: TableDocumentsPage,
});

type TableSearch = {
  panel?: "schema" | "indexes";
  sort?: string;
  dir?: "asc" | "desc";
  filters?: DocumentFilter[];
  /** One cursor per page after the first — the pager's position. */
  cursors?: string[];
};

// The whole stack travels, not just the current cursor: PREV walks back down
// it, so a URL carrying only the current page would deep-link a view whose
// PREV button is dead on reload.
function parseCursors(raw: unknown): string[] | undefined {
  if (!Array.isArray(raw)) return undefined;
  const out = raw.filter(
    (value): value is string => typeof value === "string" && value !== "",
  );
  return out.length > 0 ? out : undefined;
}

const NO_FILTERS: DocumentFilter[] = [];
const NO_CURSORS: string[] = [];
/** Stands in while the first page of a table is still in flight. */
const EMPTY_PAGE: PageResponse = {
  data: [],
  next_cursor: null,
  has_more: false,
};
/** IDs listed by name in the bulk-delete confirmation before it elides. */
const CONFIRM_ID_PREVIEW = 5;
/** Failures listed by name in the partial-delete toast before it elides. */
const FAILED_ID_PREVIEW = 3;
/** How long the partial-delete toast stays up, in ms. */
const FAILED_DELETE_TOAST_MS = 12_000;

type DeleteFailure = { id: string; error: string };

// The mirror of the confirm dialog's id list: that one names what is about to
// be destroyed, this one names what survived and why.
function DeleteFailures({
  failures,
}: {
  failures: ReadonlyArray<DeleteFailure>;
}) {
  return (
    <ul className="font-mono text-xs" data-testid="documents-delete-failures">
      {failures.slice(0, FAILED_ID_PREVIEW).map((failure) => (
        <li key={failure.id} className="break-all">
          {failure.id}: {failure.error}
        </li>
      ))}
      {failures.length > FAILED_ID_PREVIEW ? (
        <li>and {failures.length - FAILED_ID_PREVIEW} more</li>
      ) : null}
    </ul>
  );
}

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

  const cursors = search.cursors ?? NO_CURSORS;
  const setCursors = useCallback(
    (next: string[]) => {
      void navigate({
        search: (prev: TableSearch) => ({
          ...prev,
          cursors: next.length > 0 ? next : undefined,
        }),
      });
    },
    [navigate],
  );
  const pager = useMemo(() => ({ cursors, setCursors }), [cursors, setCursors]);

  const {
    page,
    loading,
    pageError,
    pageNumber,
    refresh,
    onNext,
    onPrev,
    reset,
  } = useDocumentPage(tenant, table, { filters, order }, pager);
  const tablePage = page ?? EMPTY_PAGE;

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [showInsert, setShowInsert] = useState(false);
  const [editing, setEditing] = useState<DocumentJson | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string[] | null>(null);
  const [deletingDocs, setDeletingDocs] = useState(false);
  // A bulk delete is a loop of independent requests, not one call, and the
  // dialog reporting it belongs to this route. Navigating away mid-delete
  // (sidebar, palette, browser back) used to leave the loop destroying
  // documents with nothing on screen saying so: the state setters went
  // nowhere, the dialog left with the route, and only the final toast
  // arrived, seconds later and out of context. The loop reads this flag
  // between documents.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
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
      const failures: DeleteFailure[] = [];
      let attempted = 0;
      for (const id of ids) {
        // Stop at a document boundary rather than mid-request: at most the
        // one delete already in flight outlives the route, and the rest of
        // the set is left where the operator can still see it.
        if (!mounted.current) break;
        attempted += 1;
        const result = await documents.remove(tenant, table, id);
        if (!result.ok) failures.push({ id, error: result.error });
      }
      const stopped = attempted < ids.length;
      setDeletingDocs(false);
      setConfirmDelete(null);
      if (stopped) {
        // The table is gone, so this toast is the only account of a
        // half-finished destructive operation. It says how far the loop got
        // and that the remainder was never touched, because "Deleted 1/3"
        // would read as two failures that never happened.
        toast.error(
          `Stopped after deleting ${attempted - failures.length} of ${ids.length} documents`,
          {
            description: (
              <div className="space-y-1" data-testid="documents-delete-stopped">
                <p>
                  Left {table} before the rest were deleted;{" "}
                  {ids.length - attempted} document
                  {ids.length - attempted === 1 ? " was" : "s were"} not
                  touched.
                </p>
                {failures.length > 0 ? (
                  <DeleteFailures failures={failures} />
                ) : null}
              </div>
            ),
            duration: FAILED_DELETE_TOAST_MS,
          },
        );
      } else if (failures.length === 0) {
        toast.success(
          `Deleted ${ids.length} document${ids.length === 1 ? "" : "s"}`,
        );
        setSelected(new Set());
      } else {
        // A bare count ("Deleted 3/5") names neither the documents that
        // survived nor the reason, and the reasons differ per document: a
        // permission denial reads nothing like a validation trigger. The
        // server message is already extracted for us in `ApiResult.error`, so
        // drop it into the toast next to the id it belongs to instead of
        // counting the failure and discarding it.
        toast.error(
          `Deleted ${ids.length - failures.length}/${ids.length} documents`,
          {
            description: <DeleteFailures failures={failures} />,
            // Long enough to read several "<id>: <reason>" lines; the default
            // four seconds is not, and this is the only place the reason is
            // ever shown.
            duration: FAILED_DELETE_TOAST_MS,
          },
        );
        // Leave exactly the failures selected: the surviving rows come back on
        // refresh, and re-picking them out of a refreshed table is the step
        // the operator should not have to repeat before retrying.
        setSelected(new Set(failures.map((failure) => failure.id)));
      }
      if (mounted.current) refresh();
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

  // A cursor is decoded against the query that produced it, so a filter or
  // sort change resets the position in the *same* navigation — one URL write,
  // never a render where the new query and the old cursor coexist.
  const applyFilters = useCallback(
    (next: DocumentFilter[]) => {
      patchSearch({
        filters: next.length > 0 ? next : undefined,
        cursors: undefined,
      });
      setSelected(new Set());
    },
    [patchSearch],
  );

  const applyOrder = useCallback(
    (next: DocumentOrder | null) => {
      patchSearch({
        sort: next?.field,
        dir: next?.direction,
        cursors: undefined,
      });
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
        {/* `flex-1` is `flex: 1 1 0%`, and `overflow-hidden` resolves this
            column's automatic minimum width to zero, so with the schema or
            index inspector open it yielded every pixel to its 420px sibling
            and collapsed to its own two borders at narrow widths. 20rem is
            what one readable row costs: the 38px selection gutter the `_id`
            column is pinned against, the 13-character `_id` cell itself
            (~118px at text-xs mono plus `px-3`), and one data cell wide
            enough to read. The pinned action column is deliberately not in
            that budget -- the grid scrolls horizontally, so it stays
            reachable. */}
        <div
          className="flex min-h-0 min-w-[20rem] flex-1 flex-col overflow-hidden rounded-md border border-app bg-surface"
          data-testid="documents-table-column"
        >
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
          ) : pageError ? (
            <PageError message={pageError} onRetry={refresh} />
          ) : loading || tablePage.data.length > 0 ? (
            // A page in flight keeps the table mounted and paints skeleton
            // rows: the header, the column widths and the pager stay put, and
            // no row of the previous table survives under the new one.
            <DocumentsTable
              page={tablePage}
              loading={loading}
              columns={columns}
              selected={selected}
              pageNumber={pageNumber}
              order={order}
              indexBacked={indexBacked}
              onSort={requestSort}
              onToggleAll={(checked) =>
                setSelected(
                  checked
                    ? new Set(
                        tablePage.data
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
          ) : filters.length > 0 ? (
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
