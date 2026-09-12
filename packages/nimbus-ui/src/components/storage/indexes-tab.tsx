import { Plus, Trash2 } from "lucide-react";
import { type FormEvent, useCallback, useMemo, useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { schema as schemaApi } from "../../lib/api-mutations";
import type { TableSchemaIndex, TableSchemaShape } from "../../lib/types/table";
import { ConfirmDialog } from "../confirm-dialog";
import { DataTable, dataColumns } from "../data-table";
import { StatePill } from "../pill";
import { draftFromSchema, parseSchemaDraft } from "./schema-draft";
import { useIndexStatus } from "./use-index-status";

// The Indexes tab of a table. An index is one entry of the table's schema,
// so create and drop both go through the checked schema apply: the server
// scans the documents against the schema the index rides in, and refuses
// the whole change when one violates it. Storage rebuilds the index inside
// the same commit, so the Status column reads `enabled` as soon as apply
// returns; the pill still polls the schema route so a slower build, if one
// ever lands, is shown rather than assumed.
export function IndexesTab({
  tenant,
  table,
  schema,
  onChanged,
  pollMs,
}: {
  tenant: string;
  table: string;
  schema: TableSchemaShape | null;
  onChanged: () => void;
  pollMs?: number;
}) {
  const indexes = useMemo(() => schema?.indexes ?? [], [schema]);
  const states = useIndexStatus(tenant, table, schema, pollMs);
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dropping, setDropping] = useState<TableSchemaIndex | null>(null);

  // Every change re-sends the whole schema with the index list edited. The
  // draft is rebuilt from the committed schema so server-owned fields (id,
  // state) never round-trip, and a table without a schema gets an empty
  // field list: an index-only schema constrains nothing.
  const applyIndexes = useCallback(
    async (next: TableSchemaIndex[], verb: string) => {
      const draft = parseSchemaDraft(draftFromSchema(schema, table), table);
      if (!draft.ok) {
        setError(draft.error);
        return false;
      }
      setError(null);
      const result = await schemaApi.apply(tenant, table, {
        ...draft.schema,
        indexes: next.map((index) => ({
          name: index.name,
          fields: index.fields,
        })),
      });
      if (!result.ok) {
        setError(result.error);
        return false;
      }
      if (!result.data.applied) {
        setError(
          `Index not ${verb}: ${result.data.violation_count} of ${result.data.scanned} documents violate the table schema. Fix them on the Schema tab, then try again.`,
        );
        return false;
      }
      onChanged();
      return true;
    },
    [schema, tenant, table, onChanged],
  );

  const create = useCallback(
    async (index: TableSchemaIndex) => {
      setBusy("create");
      const ok = await applyIndexes([...indexes, index], "created");
      setBusy(null);
      if (ok) {
        setAdding(false);
        toast.success(`Index ${index.name} created`);
      }
      return ok;
    },
    [applyIndexes, indexes],
  );

  const drop = useCallback(async () => {
    if (!dropping) return;
    const target = dropping;
    setDropping(null);
    setBusy(`drop:${target.name}`);
    const ok = await applyIndexes(
      indexes.filter((index) => index.name !== target.name),
      "dropped",
    );
    setBusy(null);
    if (ok) toast.success(`Index ${target.name} dropped`);
  }, [applyIndexes, dropping, indexes]);

  const columns = useMemo(() => {
    const column = dataColumns<TableSchemaIndex>();
    return [
      column.accessor("name", {
        header: "Name",
        size: 200,
        cell: ({ getValue }) => (
          <span className="font-mono text-xs">{getValue()}</span>
        ),
      }),
      column.accessor((index) => index.fields.join(", "), {
        id: "fields",
        header: "Fields",
        size: 280,
        cell: ({ getValue }) => (
          <span className="font-mono text-xs">{getValue()}</span>
        ),
      }),
      column.accessor("name", {
        id: "state",
        header: "Status",
        size: 120,
        enableSorting: false,
        cell: ({ row }) => (
          <StatePill
            state={
              states.get(row.original.name) ?? row.original.state ?? "unknown"
            }
            data-testid={`documents-index-state-${row.original.name}`}
          />
        ),
      }),
      column.display({
        id: "actions",
        header: "",
        size: 48,
        cell: ({ row }) => (
          <button
            type="button"
            className="rounded-xs p-1 text-text-3 hover:text-error focus-visible:text-error disabled:opacity-50"
            title={`Drop index ${row.original.name}`}
            aria-label={`Drop index ${row.original.name}`}
            disabled={busy !== null}
            onClick={(event) => {
              event.stopPropagation();
              setDropping(row.original);
            }}
            data-testid={`documents-index-drop-${row.original.name}`}
          >
            <Trash2 className="size-3.5" aria-hidden />
          </button>
        ),
      }),
    ];
  }, [states, busy]);

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden"
      data-testid="documents-indexes-tab"
    >
      <div className="flex items-start justify-between gap-3">
        <p className="max-w-prose text-sm text-text-3">
          An index is part of the table schema. Create and drop re-apply the
          schema, so the server checks every document first and refuses the
          change when one violates it. The index is built inside the same
          commit; a new index reads enabled as soon as the change lands.
        </p>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => {
            setAdding((open) => !open);
            setError(null);
          }}
          disabled={busy !== null}
          className="shrink-0"
          data-testid="documents-indexes-add"
        >
          <Plus className="size-3.5" aria-hidden /> New index
        </Button>
      </div>
      {adding ? (
        <NewIndexForm
          existing={indexes.map((index) => index.name)}
          busy={busy === "create"}
          onCancel={() => setAdding(false)}
          onCreate={create}
        />
      ) : null}
      {error ? (
        <p
          role="alert"
          className="rounded-md border border-error/40 bg-error-tint/40 px-3 py-2 text-sm text-text-1"
          data-testid="documents-indexes-error"
        >
          {error}
        </p>
      ) : null}
      {indexes.length === 0 ? (
        <p
          className="rounded-md border border-border-1 bg-bg-panel px-3 py-8 text-center text-sm text-text-3"
          data-testid="documents-indexes-empty"
        >
          No indexes defined. Sorting on a field without an index scans the
          whole table.
        </p>
      ) : (
        <div className="min-h-0 flex-1 overflow-hidden">
          <DataTable
            columns={columns}
            data={indexes}
            getRowId={(index) => index.name}
            ariaLabel="Indexes"
            testid="documents-indexes-table"
            className="h-full"
          />
        </div>
      )}
      <ConfirmDialog
        open={dropping !== null}
        title={`Drop index ${dropping?.name ?? ""}?`}
        description={
          <p>
            Sorting on{" "}
            <span className="font-mono">{dropping?.fields.join(", ")}</span>{" "}
            will scan the whole table until an index covers it again.
          </p>
        }
        confirmLabel="Drop index"
        danger
        busy={busy?.startsWith("drop:") ?? false}
        onCancel={() => setDropping(null)}
        onConfirm={() => void drop()}
        testid="documents-drop-index-dialog"
      />
    </div>
  );
}

const INPUT_CLASS =
  "h-[26px] rounded-xs border border-border-2 bg-bg-panel px-2 font-mono text-xs text-text-1 placeholder:text-text-3 focus-visible:border-accent-edge";

function NewIndexForm({
  existing,
  busy,
  onCancel,
  onCreate,
}: {
  existing: string[];
  busy: boolean;
  onCancel: () => void;
  onCreate: (index: TableSchemaIndex) => Promise<boolean>;
}) {
  const [name, setName] = useState("");
  const [fields, setFields] = useState("");
  const [error, setError] = useState<string | null>(null);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const trimmed = name.trim();
    const list = fields
      .split(",")
      .map((field) => field.trim())
      .filter((field) => field !== "");
    if (trimmed === "") {
      setError("Name the index.");
      return;
    }
    if (existing.includes(trimmed)) {
      setError(`An index named ${trimmed} already exists.`);
      return;
    }
    if (list.length === 0) {
      setError("List at least one field.");
      return;
    }
    setError(null);
    void onCreate({ name: trimmed, fields: list });
  };

  return (
    <form
      className="flex flex-wrap items-center gap-2 rounded-md border border-border-1 bg-bg-panel px-3 py-2"
      onSubmit={submit}
      data-testid="documents-index-form"
    >
      <label className="inline-flex items-center gap-1.5 text-xs font-medium text-text-3">
        name
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="by_author"
          className={`${INPUT_CLASS} w-40`}
          data-testid="documents-index-name"
        />
      </label>
      <label className="inline-flex items-center gap-1.5 text-xs font-medium text-text-3">
        fields
        <input
          value={fields}
          onChange={(event) => setFields(event.target.value)}
          // The leading field is the one a sort can use without a scan.
          placeholder="author, seq"
          className={`${INPUT_CLASS} w-56`}
          data-testid="documents-index-fields"
        />
      </label>
      <Button
        type="submit"
        size="sm"
        disabled={busy}
        data-testid="documents-index-create"
      >
        {busy ? "Creating…" : "Create index"}
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={onCancel}
        disabled={busy}
        data-testid="documents-index-cancel"
      >
        Cancel
      </Button>
      {error ? (
        <span
          role="alert"
          className="basis-full text-xs text-error"
          data-testid="documents-index-form-error"
        >
          {error}
        </span>
      ) : null}
    </form>
  );
}
