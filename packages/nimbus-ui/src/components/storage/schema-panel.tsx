import { useCallback, useState } from "react";
import { toast } from "sonner";

import { schema as schemaApi } from "../../lib/api-mutations";
import { cn } from "../../lib/cn";
import type { TableSchemaShape } from "../../lib/types/table";
import { ConfirmDialog } from "../confirm-dialog";
import { PanelHeader } from "../slideover";

// Side panel for editing a table's schema. Save replaces enforcement via the
// typed schema client; drop removes enforcement (keeping documents) behind a
// confirmation. Both surface failures inline and refetch through `onSaved`.
export function SchemaPanel({
  tenant,
  table,
  schema,
  onClose,
  onSaved,
}: {
  tenant: string;
  table: string;
  schema: TableSchemaShape | null;
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
    let parsed: unknown;
    try {
      parsed = JSON.parse(json);
    } catch (err) {
      setError(`Invalid JSON: ${(err as Error).message}`);
      return;
    }
    setSaving(true);
    const result = await schemaApi.put(tenant, table, parsed);
    if (!result.ok) {
      setError(result.error);
      setSaving(false);
      return;
    }
    toast.success("Schema saved");
    onSaved();
    setSaving(false);
  }, [json, tenant, table, onSaved]);

  const runDrop = useCallback(async () => {
    setConfirmDrop(false);
    setError(null);
    setDeleting(true);
    const result = await schemaApi.drop(tenant, table);
    if (!result.ok) {
      setError(result.error);
      setDeleting(false);
      return;
    }
    toast.success("Schema dropped");
    onSaved();
    setDeleting(false);
  }, [tenant, table, onSaved]);

  return (
    <aside
      // 420px is the preferred width, not a floor. With `shrink-0` it was both,
      // so in a window under ~564px the panel kept all 420px and the row's
      // `overflow-hidden` cut off its right edge — the close button included —
      // leaving no way to dismiss it. `min-w-0` lets the panel go below its
      // min-content width so it stays whole and closeable at any width; the
      // documents table beside it still yields its space first, because its
      // `flex-1` basis of 0 absorbs no shrink.
      className="flex w-[420px] min-w-0 flex-col overflow-hidden rounded-md border border-app bg-surface"
      data-testid="documents-schema-panel"
    >
      <PanelHeader title="Schema" onClose={onClose} />
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto p-3">
        <p className="font-mono text-xs text-muted">
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
              "rounded border border-app px-2 py-1 font-mono text-xs uppercase tracking-wide",
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
              "rounded border border-app px-2 py-1 font-mono text-xs uppercase tracking-wide",
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
