import { useMemo } from "react";

import { shortId } from "../../lib/format";
import type { DocumentJson } from "../../lib/types/table";
import { CopyChip } from "../copy-chip";
import { JsonEditorForm } from "../json-editor-form";
import { Slideover } from "../slideover";

// Drawer for patching an existing document. The initial JSON is the document
// with its reserved system columns stripped, so the editor shows only the
// user-writable fields; `onSubmit` sends the patch.
export function EditDrawer({
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

  return (
    <Slideover
      title={`Edit ${shortId(String(doc._id ?? ""))}`}
      onClose={onClose}
      testid="documents-edit-drawer"
    >
      <div className="flex items-center gap-2 font-mono text-xs text-muted">
        <span>_id</span>
        <CopyChip
          label="document id"
          value={String(doc._id ?? "")}
          testid="documents-edit-id"
        />
      </div>
      <JsonEditorForm
        initialJson={initial}
        label="patch (JSON object — only changed fields)"
        fieldId="edit-json"
        labelClassName="mt-2"
        submitLabel="save"
        submittingLabel="saving…"
        testidPrefix="documents-edit"
        onCancel={onClose}
        onSubmit={onSubmit}
      />
    </Slideover>
  );
}
