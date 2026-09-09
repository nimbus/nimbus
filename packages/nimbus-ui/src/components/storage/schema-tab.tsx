import { useCallback, useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { schema as schemaApi } from "../../lib/api-mutations";
import type { TableSchemaShape } from "../../lib/types/table";
import { ConfirmDialog } from "../confirm-dialog";

// The Schema tab of a table. Save replaces enforcement through the typed
// schema client; drop removes enforcement and keeps the documents, behind a
// confirmation. Both report failures inline and refetch through `onSaved`.
export function SchemaTab({
  tenant,
  table,
  schema,
  onSaved,
}: {
  tenant: string;
  table: string;
  schema: TableSchemaShape | null;
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
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden rounded-md border border-border-1 bg-bg-panel p-4"
      data-testid="documents-schema-tab"
    >
      <p className="max-w-prose text-sm text-text-3">
        Edit the schema JSON and save to replace enforcement. Drop removes
        enforcement; the table keeps its documents.
      </p>
      <Textarea
        value={json}
        onChange={(event) => setJson(event.target.value)}
        spellCheck={false}
        className="min-h-[280px] flex-1 resize-none font-mono text-xs"
        data-testid="documents-schema-textarea"
        aria-label="Schema JSON"
      />
      {error ? (
        <p
          role="alert"
          className="font-mono text-xs text-error"
          data-testid="documents-schema-error"
        >
          {error}
        </p>
      ) : null}
      <div className="flex items-center justify-end gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => setConfirmDrop(true)}
          disabled={deleting || !schema}
          className="text-error hover:text-error"
          data-testid="documents-schema-drop"
        >
          {deleting ? "Dropping…" : "Drop schema"}
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={() => void save()}
          disabled={saving}
          data-testid="documents-schema-save"
        >
          {saving ? "Saving…" : "Save schema"}
        </Button>
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
    </div>
  );
}
