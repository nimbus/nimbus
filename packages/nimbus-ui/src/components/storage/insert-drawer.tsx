import { JsonEditorForm } from "../json-editor-form";
import { Slideover } from "../slideover";

// Right-anchored drawer for inserting a document. `onSubmit` parses and
// persists (throwing on failure so the form surfaces the error); this closes
// the drawer only once it resolves.
export function InsertDrawer({
  onClose,
  onSubmit,
}: {
  onClose: () => void;
  onSubmit: (json: string) => Promise<void>;
}) {
  return (
    <Slideover
      title="Insert document"
      onClose={onClose}
      testid="documents-insert-drawer"
    >
      <JsonEditorForm
        initialJson={"{\n  \n}"}
        label="document fields (JSON object)"
        fieldId="insert-json"
        submitLabel="insert"
        submittingLabel="inserting…"
        testidPrefix="documents-insert"
        onCancel={onClose}
        onSubmit={async (json) => {
          await onSubmit(json);
          onClose();
        }}
      />
    </Slideover>
  );
}
