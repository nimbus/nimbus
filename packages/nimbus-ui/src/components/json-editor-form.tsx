import { useCallback, useState } from "react";

import { cn } from "@/lib/utils";

// A labelled JSON textarea with a cancel/submit footer that owns the draft
// text, the submitting flag, and the error surface. Callers pass only the
// submit handler (plus labels/testids); parse-and-persist and close-on-success
// stay in the caller's `onSubmit`, so a thrown error lands in the form's error
// line rather than closing the drawer.
export function JsonEditorForm({
  initialJson,
  label,
  fieldId,
  labelClassName,
  submitLabel,
  submittingLabel,
  testidPrefix,
  onSubmit,
  onCancel,
}: {
  initialJson: string;
  label: string;
  fieldId: string;
  labelClassName?: string;
  submitLabel: string;
  submittingLabel: string;
  testidPrefix: string;
  onSubmit: (json: string) => Promise<void>;
  onCancel: () => void;
}) {
  const [json, setJson] = useState(initialJson);
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
    <>
      <label
        htmlFor={fieldId}
        className={cn("text-xs font-medium text-text-3", labelClassName)}
      >
        {label}
      </label>
      <textarea
        id={fieldId}
        value={json}
        onChange={(e) => setJson(e.target.value)}
        spellCheck={false}
        className="min-h-[240px] flex-1 resize-none rounded-xs border border-border-2 bg-bg-raised p-2 font-mono text-xs text-text-1 focus-visible:border-accent"
        data-testid={`${testidPrefix}-textarea`}
      />
      {error ? (
        <p
          className="font-mono text-xs text-error"
          data-testid={`${testidPrefix}-error`}
        >
          {error}
        </p>
      ) : null}
      <div className="mt-2 flex items-center justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="rounded-xs border border-border-2 px-2 py-1 text-xs font-medium text-text-3 hover:bg-bg-panel hover:text-text-1"
        >
          cancel
        </button>
        <button
          type="button"
          onClick={() => void submit()}
          disabled={submitting}
          className={cn(
            "rounded-xs border border-border-2 px-2 py-1 text-xs font-medium",
            submitting ? "text-text-3" : "text-text-1 hover:bg-bg-panel",
          )}
          data-testid={`${testidPrefix}-submit`}
        >
          {submitting ? submittingLabel : submitLabel}
        </button>
      </div>
    </>
  );
}
