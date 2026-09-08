import { useCallback, useState } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import {
  type SchemaApplyReport,
  schema as schemaApi,
} from "../../lib/api-mutations";
import type { TableSchemaShape } from "../../lib/types/table";
import { ConfirmDialog } from "../confirm-dialog";
import { CopyChip } from "../copy-chip";
import { draftFromSchema, parseSchemaDraft } from "./schema-draft";

// The Schema tab of a table. Apply scans every document before it stores
// the draft and refuses on the first violation, so enforcement never lands
// on a table that already breaks it; Check runs the same scan without
// storing. Drop removes enforcement and keeps the documents, behind a
// confirmation. Every outcome is reported inline, and a change refetches
// through `onSaved`.
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
  const [json, setJson] = useState(() => draftFromSchema(schema, table));
  const [busy, setBusy] = useState<"check" | "apply" | "drop" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<SchemaApplyReport | null>(null);
  const [confirmDrop, setConfirmDrop] = useState(false);

  const submit = useCallback(
    async (mode: "check" | "apply") => {
      setError(null);
      setReport(null);
      const draft = parseSchemaDraft(json, table);
      if (!draft.ok) {
        setError(draft.error);
        return;
      }
      setBusy(mode);
      const result = await schemaApi.apply(tenant, table, draft.schema, {
        dryRun: mode === "check",
      });
      setBusy(null);
      if (!result.ok) {
        setError(result.error);
        return;
      }
      setReport(result.data);
      if (result.data.applied) {
        toast.success("Schema applied");
        onSaved();
      }
    },
    [json, tenant, table, onSaved],
  );

  const runDrop = useCallback(async () => {
    setConfirmDrop(false);
    setError(null);
    setReport(null);
    setBusy("drop");
    const result = await schemaApi.drop(tenant, table);
    setBusy(null);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    toast.success("Schema dropped");
    onSaved();
  }, [tenant, table, onSaved]);

  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden rounded-md border border-border-1 bg-bg-panel p-4"
      data-testid="documents-schema-tab"
    >
      <p className="max-w-prose text-sm text-text-3">
        Edit the schema and apply it. Apply reads every document first and
        refuses the draft when one violates it, so enforcement never lands on a
        table that already breaks it. Check runs the same scan without storing
        anything. Drop removes enforcement; the table keeps its documents.
      </p>
      <Textarea
        value={json}
        onChange={(event) => setJson(event.target.value)}
        spellCheck={false}
        className="min-h-[240px] flex-1 resize-none font-mono text-xs"
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
      {report ? <ApplyReport report={report} /> : null}
      <div className="flex items-center justify-end gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => setConfirmDrop(true)}
          disabled={busy !== null || !schema}
          className="mr-auto text-error hover:text-error"
          data-testid="documents-schema-drop"
        >
          {busy === "drop" ? "Dropping…" : "Drop schema"}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void submit("check")}
          disabled={busy !== null}
          data-testid="documents-schema-check"
        >
          {busy === "check" ? "Checking…" : "Check"}
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={() => void submit("apply")}
          disabled={busy !== null}
          data-testid="documents-schema-apply"
        >
          {busy === "apply" ? "Applying…" : "Apply schema"}
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
        busy={busy === "drop"}
        onCancel={() => setConfirmDrop(false)}
        onConfirm={() => void runDrop()}
        testid="documents-drop-schema-dialog"
      />
    </div>
  );
}

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

// The server's report, as the three outcomes it can carry: applied, a check
// that passed, and a refusal with the documents that caused it. The refusal
// lists ids because "12 violations" gives an operator nothing to fix.
export function ApplyReport({ report }: { report: SchemaApplyReport }) {
  const refused = report.violation_count > 0;
  const listed = report.violations.length;
  return (
    <div
      role="status"
      className={cn(
        "flex shrink-0 flex-col gap-2 rounded-md border px-3 py-2 text-sm",
        refused
          ? "border-error/40 bg-error-tint/40"
          : "border-success/40 bg-success-tint/40",
      )}
      data-testid="documents-schema-report"
      data-outcome={
        refused ? "refused" : report.applied ? "applied" : "checked"
      }
    >
      {refused ? (
        <p className="text-text-1">
          <span className="font-medium text-error">Not applied.</span>{" "}
          {plural(report.violation_count, "document")} of{" "}
          {report.scanned.toLocaleString()} scanned violate the draft. Fix or
          delete them, then apply again.
        </p>
      ) : report.applied ? (
        <p className="text-text-1">
          <span className="font-medium text-success">Applied.</span> All{" "}
          {plural(report.scanned, "document")} satisfy the schema; new writes
          are validated against it.
        </p>
      ) : (
        <p className="text-text-1">
          <span className="font-medium text-success">Check passed.</span> All{" "}
          {plural(report.scanned, "document")} satisfy the draft. Nothing was
          stored.
        </p>
      )}
      {refused ? (
        <ul
          className="flex max-h-48 flex-col gap-1 overflow-auto font-mono text-xs"
          data-testid="documents-schema-violations"
        >
          {report.violations.map((violation) => (
            <li
              key={violation.id}
              className="flex flex-wrap items-center gap-x-2"
              data-testid={`documents-schema-violation-${violation.id}`}
            >
              <CopyChip label="document id" value={violation.id}>
                {violation.id}
              </CopyChip>
              <span className="text-text-3">{violation.message}</span>
            </li>
          ))}
          {report.violation_count > listed ? (
            <li className="text-text-3">
              and {(report.violation_count - listed).toLocaleString()} more
            </li>
          ) : null}
        </ul>
      ) : null}
    </div>
  );
}
