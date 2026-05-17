import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

import { api } from "../../convex/_generated/api";
import { Breadcrumb } from "../components/breadcrumb";
import { ConfirmDialog } from "../components/confirm-dialog";
import { CopyChip } from "../components/copy-chip";
import { cn } from "../lib/cn";
import { shortId } from "../lib/format";

export const Route = createFileRoute("/storage_/$tenant_/$table")({
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

type TableDoc = {
  _id: string;
  tenantId?: string;
  name?: string;
  schema?: SchemaShape | null;
  rowCount?: number;
};

type SchemaField = {
  name: string;
  field_type?: string;
  required?: boolean;
};

type SchemaShape = {
  table?: string;
  fields?: SchemaField[];
  indexes?: Array<{
    name: string;
    fields: string[];
    unique?: boolean;
    type?: string;
  }>;
};

type DocumentJson = Record<string, unknown> & {
  _id?: string;
  _creationTime?: number;
  _updateTime?: number;
};

type PageResponse = {
  data: DocumentJson[];
  next_cursor: string | null;
  has_more: boolean;
};

const PAGE_SIZE = 25;

function TableDocumentsPage() {
  const { tenant, table } = Route.useParams();
  const search = Route.useSearch();
  const navigate = Route.useNavigate();

  const tableMeta = useQuery(api.tables.byName, {
    tenantId: tenant,
    name: table,
  }) as TableDoc | null | undefined;

  const [page, setPage] = useState<PageResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [pageError, setPageError] = useState<string | null>(null);
  const [cursorStack, setCursorStack] = useState<Array<string | null>>([null]);
  const [refreshTick, setRefreshTick] = useState(0);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [showInsert, setShowInsert] = useState(false);
  const [editing, setEditing] = useState<DocumentJson | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string[] | null>(null);
  const [deletingDocs, setDeletingDocs] = useState(false);

  const currentCursor = cursorStack[cursorStack.length - 1] ?? null;

  const loadPage = useCallback(
    async (cursor: string | null) => {
      setLoading(true);
      setPageError(null);
      try {
        const response = await fetch(
          `/api/tenants/${encodeURIComponent(tenant)}/query/paginated`,
          {
            method: "POST",
            credentials: "include",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              query: {
                table,
                filters: [],
                order: null,
                limit: null,
              },
              page_size: PAGE_SIZE,
              after: cursor,
            }),
          },
        );
        if (!response.ok) {
          const body = (await response.json().catch(() => null)) as {
            error?: { message?: string };
          } | null;
          throw new Error(
            body?.error?.message ?? `Query failed: ${response.status}`,
          );
        }
        const body = (await response.json()) as {
          data: DocumentJson[];
          next_cursor: string | null;
          has_more: boolean;
        };
        setPage({
          data: body.data,
          next_cursor: body.next_cursor,
          has_more: body.has_more,
        });
      } catch (err) {
        setPageError(err instanceof Error ? err.message : String(err));
        setPage(null);
      } finally {
        setLoading(false);
      }
    },
    [tenant, table],
  );

  // biome-ignore lint/correctness/useExhaustiveDependencies: refreshTick is the manual refetch trigger
  useEffect(() => {
    void loadPage(currentCursor);
  }, [loadPage, currentCursor, refreshTick]);

  const reset = useCallback(() => {
    setCursorStack([null]);
    setSelected(new Set());
    setRefreshTick((t) => t + 1);
  }, []);

  const onNext = useCallback(() => {
    if (page?.next_cursor) {
      setCursorStack((stack) => [...stack, page.next_cursor]);
      setSelected(new Set());
    }
  }, [page]);

  const onPrev = useCallback(() => {
    setCursorStack((stack) => (stack.length > 1 ? stack.slice(0, -1) : stack));
    setSelected(new Set());
  }, []);

  const handleInsert = useCallback(
    async (json: string) => {
      let parsed: unknown;
      try {
        parsed = JSON.parse(json);
      } catch (err) {
        throw new Error(`Invalid JSON: ${(err as Error).message}`);
      }
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        throw new Error("Document must be a JSON object");
      }
      const response = await fetch(
        `/api/tenants/${encodeURIComponent(tenant)}/documents`,
        {
          method: "POST",
          credentials: "include",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ table, fields: parsed }),
        },
      );
      if (!response.ok) {
        const body = (await response.json().catch(() => null)) as {
          error?: { message?: string };
        } | null;
        throw new Error(
          body?.error?.message ?? `Insert failed: ${response.status}`,
        );
      }
      toast.success("Document inserted");
      reset();
    },
    [tenant, table, reset],
  );

  const handleUpdate = useCallback(
    async (id: string, json: string) => {
      let parsed: unknown;
      try {
        parsed = JSON.parse(json);
      } catch (err) {
        throw new Error(`Invalid JSON: ${(err as Error).message}`);
      }
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        throw new Error("Patch must be a JSON object");
      }
      const response = await fetch(
        `/api/tenants/${encodeURIComponent(tenant)}/documents/${encodeURIComponent(table)}/${encodeURIComponent(id)}`,
        {
          method: "PATCH",
          credentials: "include",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ patch: parsed }),
        },
      );
      if (!response.ok) {
        const body = (await response.json().catch(() => null)) as {
          error?: { message?: string };
        } | null;
        throw new Error(
          body?.error?.message ?? `Update failed: ${response.status}`,
        );
      }
      toast.success("Document updated");
      setEditing(null);
      setRefreshTick((t) => t + 1);
    },
    [tenant, table],
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
        const response = await fetch(
          `/api/tenants/${encodeURIComponent(tenant)}/documents/${encodeURIComponent(table)}/${encodeURIComponent(id)}`,
          { method: "DELETE", credentials: "include" },
        );
        if (!response.ok) failed += 1;
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
      setRefreshTick((t) => t + 1);
    },
    [tenant, table],
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
        search: { panel: search.panel === panel ? undefined : panel },
      });
    },
    [navigate, search.panel],
  );

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-table-documents"
    >
      <header className="flex flex-col gap-2">
        <Breadcrumb
          segments={[
            { label: "storage", href: "/storage" },
            {
              label: tenant,
              href: "/storage/$tenant",
              copyValue: tenant,
              copyLabel: "tenant id",
            },
            {
              label: table,
              copyValue: table,
              copyLabel: "table",
              active: true,
            },
          ]}
          testid="documents-breadcrumb"
        />
        <div className="flex items-baseline justify-between">
          <div>
            <h1 className="text-default" style={{ fontSize: "var(--text-xl)" }}>
              <span className="font-mono">{table}</span>
            </h1>
            <p className="text-sm text-muted">
              {tableMeta?.schema
                ? "Schema enforced. Inserts validated before write."
                : "Schemaless table — any document shape is accepted."}
            </p>
          </div>
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
        </div>
      </header>

      <div className="flex min-h-0 flex-1 gap-4 overflow-hidden">
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-md border border-app bg-surface">
          {loading && !page ? (
            <Loading label="Loading documents…" />
          ) : pageError ? (
            <PageError
              message={pageError}
              onRetry={() => setRefreshTick((t) => t + 1)}
            />
          ) : !page || page.data.length === 0 ? (
            <Empty
              title="No documents"
              detail={`Insert a document using the toolbar or POST /api/tenants/${tenant}/documents with body { table: "${table}", fields: {...} }.`}
            />
          ) : (
            <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
              <div className="flex-1 overflow-auto">
                <table
                  className="w-full border-collapse text-sm"
                  data-testid="documents-table"
                >
                  <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
                    <tr>
                      <th className="w-8 px-2 py-2">
                        <input
                          type="checkbox"
                          aria-label="Select all on page"
                          checked={
                            page.data.length > 0 &&
                            page.data.every((doc) =>
                              selected.has(String(doc._id ?? "")),
                            )
                          }
                          onChange={(e) => {
                            if (e.target.checked) {
                              setSelected(
                                new Set(
                                  page.data
                                    .map((d) => String(d._id ?? ""))
                                    .filter(Boolean),
                                ),
                              );
                            } else {
                              setSelected(new Set());
                            }
                          }}
                          data-testid="documents-select-all"
                        />
                      </th>
                      {columns.map((col) => (
                        <th
                          key={col}
                          className="px-3 py-2 text-left font-semibold"
                        >
                          {col}
                        </th>
                      ))}
                      <th className="px-3 py-2 text-right font-semibold">
                        actions
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {page.data.map((doc) => {
                      const id = String(doc._id ?? "");
                      return (
                        <tr
                          key={id}
                          className="border-t border-app hover:bg-surface-2"
                          data-testid={`documents-row-${id}`}
                        >
                          <td className="w-8 px-2 py-2 align-top">
                            <input
                              type="checkbox"
                              aria-label={`Select document ${shortId(id)}`}
                              checked={selected.has(id)}
                              onChange={(e) => {
                                const next = new Set(selected);
                                if (e.target.checked) next.add(id);
                                else next.delete(id);
                                setSelected(next);
                              }}
                              data-testid={`documents-select-${id}`}
                            />
                          </td>
                          {columns.map((col) => (
                            <td
                              key={col}
                              className="px-3 py-2 align-top font-mono text-xs text-default"
                            >
                              <CellValue value={doc[col]} field={col} id={id} />
                            </td>
                          ))}
                          <td className="px-3 py-2 text-right align-top">
                            <button
                              type="button"
                              onClick={() => setEditing(doc)}
                              className="mr-2 rounded border border-app px-2 py-0.5 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
                              data-testid={`documents-edit-${id}`}
                            >
                              edit
                            </button>
                            <button
                              type="button"
                              onClick={() => void handleDelete([id])}
                              className="rounded border border-app px-2 py-0.5 font-mono text-[11px] uppercase tracking-wide text-danger hover:bg-surface"
                              data-testid={`documents-delete-${id}`}
                            >
                              delete
                            </button>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
              <div
                className="flex items-center justify-between border-t border-app bg-surface-2 px-3 py-2 font-mono text-[11px] text-muted"
                data-testid="documents-pagination"
              >
                <span>
                  page {cursorStack.length} · {page.data.length} row
                  {page.data.length === 1 ? "" : "s"}
                </span>
                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    onClick={onPrev}
                    disabled={cursorStack.length <= 1}
                    className={cn(
                      "rounded border border-app px-2 py-0.5 uppercase tracking-wide",
                      cursorStack.length <= 1
                        ? "text-muted"
                        : "text-default hover:bg-surface",
                    )}
                    data-testid="documents-prev-page"
                  >
                    prev
                  </button>
                  <button
                    type="button"
                    onClick={onNext}
                    disabled={!page.has_more}
                    className={cn(
                      "rounded border border-app px-2 py-0.5 uppercase tracking-wide",
                      !page.has_more
                        ? "text-muted"
                        : "text-default hover:bg-surface",
                    )}
                    data-testid="documents-next-page"
                  >
                    next
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>

        {search.panel === "schema" ? (
          <SchemaPanel
            tenant={tenant}
            table={table}
            schema={tableMeta?.schema ?? null}
            onClose={() => togglePanel(undefined)}
            onSaved={() => setRefreshTick((t) => t + 1)}
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

function CellValue({
  value,
  field,
  id,
}: {
  value: unknown;
  field: string;
  id: string;
}) {
  if (field === "_id" && id) {
    return (
      <CopyChip
        label="document id"
        value={id}
        testid={`documents-cell-id-${id}`}
      >
        {shortId(id)}
      </CopyChip>
    );
  }
  if (value === undefined || value === null) {
    return <span className="text-muted">—</span>;
  }
  if (typeof value === "string") {
    return (
      <span className="truncate" title={value}>
        {value}
      </span>
    );
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return <span>{String(value)}</span>;
  }
  return (
    <span className="truncate text-muted" title={JSON.stringify(value)}>
      {JSON.stringify(value).slice(0, 60)}
    </span>
  );
}

function InsertDrawer({
  onClose,
  onSubmit,
}: {
  onClose: () => void;
  onSubmit: (json: string) => Promise<void>;
}) {
  const [json, setJson] = useState("{\n  \n}");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = useCallback(async () => {
    setError(null);
    setSubmitting(true);
    try {
      await onSubmit(json);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  }, [json, onSubmit, onClose]);

  return (
    <Drawer
      title="Insert document"
      onClose={onClose}
      testid="documents-insert-drawer"
    >
      <label
        htmlFor="insert-json"
        className="font-mono text-[10px] uppercase tracking-wide text-muted"
      >
        document fields (JSON object)
      </label>
      <textarea
        id="insert-json"
        value={json}
        onChange={(e) => setJson(e.target.value)}
        spellCheck={false}
        className="min-h-[240px] flex-1 resize-none rounded border border-app bg-surface-2 p-2 font-mono text-xs text-default focus-visible:border-strong"
        data-testid="documents-insert-textarea"
      />
      {error ? (
        <p
          className="font-mono text-xs text-danger"
          data-testid="documents-insert-error"
        >
          {error}
        </p>
      ) : null}
      <div className="mt-2 flex items-center justify-end gap-2">
        <button
          type="button"
          onClick={onClose}
          className="rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
        >
          cancel
        </button>
        <button
          type="button"
          onClick={() => void submit()}
          disabled={submitting}
          className={cn(
            "rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide",
            submitting ? "text-muted" : "text-default hover:bg-surface",
          )}
          data-testid="documents-insert-submit"
        >
          {submitting ? "inserting…" : "insert"}
        </button>
      </div>
    </Drawer>
  );
}

function EditDrawer({
  doc,
  onClose,
  onSubmit,
}: {
  doc: DocumentJson;
  onClose: () => void;
  onSubmit: (json: string) => Promise<void>;
}) {
  const initial = useMemo(() => {
    const copy: Record<string, unknown> = { ...doc };
    delete copy._id;
    delete copy._creationTime;
    delete copy._updateTime;
    return JSON.stringify(copy, null, 2);
  }, [doc]);
  const [json, setJson] = useState(initial);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = useCallback(async () => {
    setError(null);
    setSubmitting(true);
    try {
      await onSubmit(json);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  }, [json, onSubmit]);

  return (
    <Drawer
      title={`Edit ${shortId(String(doc._id ?? ""))}`}
      onClose={onClose}
      testid="documents-edit-drawer"
    >
      <div className="flex items-center gap-2 font-mono text-[11px] text-muted">
        <span>_id</span>
        <CopyChip
          label="document id"
          value={String(doc._id ?? "")}
          testid="documents-edit-id"
        />
      </div>
      <label
        htmlFor="edit-json"
        className="mt-2 font-mono text-[10px] uppercase tracking-wide text-muted"
      >
        patch (JSON object — only changed fields)
      </label>
      <textarea
        id="edit-json"
        value={json}
        onChange={(e) => setJson(e.target.value)}
        spellCheck={false}
        className="min-h-[240px] flex-1 resize-none rounded border border-app bg-surface-2 p-2 font-mono text-xs text-default focus-visible:border-strong"
        data-testid="documents-edit-textarea"
      />
      {error ? (
        <p
          className="font-mono text-xs text-danger"
          data-testid="documents-edit-error"
        >
          {error}
        </p>
      ) : null}
      <div className="mt-2 flex items-center justify-end gap-2">
        <button
          type="button"
          onClick={onClose}
          className="rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
        >
          cancel
        </button>
        <button
          type="button"
          onClick={() => void submit()}
          disabled={submitting}
          className={cn(
            "rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide",
            submitting ? "text-muted" : "text-default hover:bg-surface",
          )}
          data-testid="documents-edit-submit"
        >
          {submitting ? "saving…" : "save"}
        </button>
      </div>
    </Drawer>
  );
}

function SchemaPanel({
  tenant,
  table,
  schema,
  onClose,
  onSaved,
}: {
  tenant: string;
  table: string;
  schema: SchemaShape | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [json, setJson] = useState(() =>
    schema ? JSON.stringify(schema, null, 2) : "{\n  \n}",
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [confirmDrop, setConfirmDrop] = useState(false);

  const save = useCallback(async () => {
    setError(null);
    setSaving(true);
    try {
      let parsed: unknown;
      try {
        parsed = JSON.parse(json);
      } catch (err) {
        throw new Error(`Invalid JSON: ${(err as Error).message}`);
      }
      const response = await fetch(
        `/api/tenants/${encodeURIComponent(tenant)}/schema/${encodeURIComponent(table)}`,
        {
          method: "PUT",
          credentials: "include",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(parsed),
        },
      );
      if (!response.ok) {
        const body = (await response.json().catch(() => null)) as {
          error?: { message?: string };
        } | null;
        throw new Error(
          body?.error?.message ?? `Save failed: ${response.status}`,
        );
      }
      toast.success("Schema saved");
      onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }, [json, tenant, table, onSaved]);

  const runDrop = useCallback(async () => {
    setConfirmDrop(false);
    setError(null);
    setDeleting(true);
    try {
      const response = await fetch(
        `/api/tenants/${encodeURIComponent(tenant)}/schema/${encodeURIComponent(table)}`,
        { method: "DELETE", credentials: "include" },
      );
      if (!response.ok) {
        const body = (await response.json().catch(() => null)) as {
          error?: { message?: string };
        } | null;
        throw new Error(
          body?.error?.message ?? `Drop failed: ${response.status}`,
        );
      }
      toast.success("Schema dropped");
      onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setDeleting(false);
    }
  }, [tenant, table, onSaved]);

  return (
    <aside
      className="flex w-[420px] shrink-0 flex-col overflow-hidden rounded-md border border-app bg-surface"
      data-testid="documents-schema-panel"
    >
      <PanelHeader title="Schema" onClose={onClose} />
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto p-3">
        <p className="font-mono text-[11px] text-muted">
          Replace the schema by editing this JSON and saving. Drop removes
          enforcement (the table still keeps its documents).
        </p>
        <textarea
          value={json}
          onChange={(e) => setJson(e.target.value)}
          spellCheck={false}
          className="min-h-[280px] flex-1 resize-none rounded border border-app bg-surface-2 p-2 font-mono text-xs text-default focus-visible:border-strong"
          data-testid="documents-schema-textarea"
          aria-label="Schema JSON"
        />
        {error ? (
          <p
            className="font-mono text-xs text-danger"
            data-testid="documents-schema-error"
          >
            {error}
          </p>
        ) : null}
        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={() => setConfirmDrop(true)}
            disabled={deleting || !schema}
            className={cn(
              "rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide",
              deleting || !schema
                ? "text-muted"
                : "text-danger hover:bg-surface-2",
            )}
            data-testid="documents-schema-drop"
          >
            {deleting ? "dropping…" : "drop"}
          </button>
          <button
            type="button"
            onClick={() => void save()}
            disabled={saving}
            className={cn(
              "rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide",
              saving ? "text-muted" : "text-default hover:bg-surface-2",
            )}
            data-testid="documents-schema-save"
          >
            {saving ? "saving…" : "save"}
          </button>
        </div>
      </div>
      <ConfirmDialog
        open={confirmDrop}
        title={`Drop schema for ${table}?`}
        description={
          <p>
            The table will accept any document shape. Existing documents are
            kept; only enforcement is removed.
          </p>
        }
        confirmLabel="Drop schema"
        danger
        busy={deleting}
        onCancel={() => setConfirmDrop(false)}
        onConfirm={() => void runDrop()}
        testid="documents-drop-schema-dialog"
      />
    </aside>
  );
}

function IndexPanel({
  schema,
  onClose,
}: {
  schema: SchemaShape | null;
  onClose: () => void;
}) {
  const indexes = schema?.indexes ?? [];
  return (
    <aside
      className="flex w-[420px] shrink-0 flex-col overflow-hidden rounded-md border border-app bg-surface"
      data-testid="documents-indexes-panel"
    >
      <PanelHeader title="Indexes" onClose={onClose} />
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto p-3">
        <p className="font-mono text-[11px] text-muted">
          Read-only view derived from the table schema. Index REST endpoints
          (create/drop) ship after the native index API lands.
        </p>
        {indexes.length === 0 ? (
          <p
            className="font-mono text-xs text-muted"
            data-testid="documents-indexes-empty"
          >
            No indexes defined.
          </p>
        ) : (
          <table
            className="w-full border-collapse text-xs"
            data-testid="documents-indexes-table"
          >
            <thead className="text-[10px] uppercase tracking-wide text-muted">
              <tr>
                <th className="px-2 py-1 text-left">Name</th>
                <th className="px-2 py-1 text-left">Fields</th>
                <th className="px-2 py-1 text-left">Unique</th>
              </tr>
            </thead>
            <tbody>
              {indexes.map((idx) => (
                <tr key={idx.name} className="border-t border-app">
                  <td className="px-2 py-1 font-mono text-default">
                    {idx.name}
                  </td>
                  <td className="px-2 py-1 font-mono text-default">
                    {idx.fields.join(", ")}
                  </td>
                  <td className="px-2 py-1 font-mono text-muted">
                    {idx.unique ? "yes" : "no"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </aside>
  );
}

function Drawer({
  title,
  onClose,
  testid,
  children,
}: {
  title: string;
  onClose: () => void;
  testid: string;
  children: React.ReactNode;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-30 flex justify-end">
      <button
        type="button"
        aria-label={`Dismiss ${title}`}
        onClick={onClose}
        className="absolute inset-0 bg-black/40"
        data-testid={`${testid}-overlay`}
      />
      <div
        role="dialog"
        aria-label={title}
        className="relative flex h-full w-[480px] flex-col gap-2 border-l border-app bg-bg p-4 shadow-xl"
        data-testid={testid}
      >
        <PanelHeader title={title} onClose={onClose} />
        {children}
      </div>
    </div>
  );
}

function PanelHeader({
  title,
  onClose,
}: {
  title: string;
  onClose: () => void;
}) {
  return (
    <div className="flex items-center justify-between border-b border-app px-3 py-2">
      <h2 className="font-mono text-xs uppercase tracking-[0.14em] text-muted">
        {title}
      </h2>
      <button
        type="button"
        onClick={onClose}
        aria-label={`Close ${title}`}
        className="rounded border border-app px-2 py-0.5 font-mono text-[11px] uppercase tracking-wide text-muted hover:bg-surface hover:text-default"
      >
        close
      </button>
    </div>
  );
}

function Loading({ label }: { label: string }) {
  return (
    <div className="flex h-full items-center justify-center font-mono text-xs text-muted">
      {label}
    </div>
  );
}

function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div
      className="flex h-full flex-col items-center justify-center gap-2 px-6 py-10 text-center"
      data-testid="documents-empty"
    >
      <p className="font-mono text-sm text-default">{title}</p>
      <p className="max-w-md text-xs text-muted">{detail}</p>
    </div>
  );
}

function PageError({
  message,
  onRetry,
}: {
  message: string;
  onRetry: () => void;
}) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 py-10 text-center">
      <p
        className="font-mono text-sm text-danger"
        data-testid="documents-error"
      >
        {message}
      </p>
      <button
        type="button"
        onClick={onRetry}
        className="rounded border border-app px-2 py-1 font-mono text-[11px] uppercase tracking-wide text-default hover:bg-surface"
        data-testid="documents-retry"
      >
        retry
      </button>
    </div>
  );
}
