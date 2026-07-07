import { useCallback, useState } from "react";

import { cn } from "../lib/cn";

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
        className={cn(
          "font-mono text-[10px] uppercase tracking-wide text-muted",
          labelClassName,
        )}
      >
        {label}
      </label>
      <textarea
        id={fieldId}
        value={json}
        onChange={(e) => setJson(e.target.value)}
        spellCheck={false}
        className="min-h-[240px] flex-1 resize-none rounded border border-app bg-surface-2 p-2 font-mono text-xs text-default focus-visible:border-strong"
        data-testid={`${testidPrefix}-textarea`}
      />
      {error ? (
        <p
          className="font-mono text-xs text-danger"
          data-testid={`${testidPrefix}-error`}
        >
          {error}
        </p>
      ) : null}
      <div className="mt-2 flex items-center justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
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
          data-testid={`${testidPrefix}-submit`}
        >
          {submitting ? submittingLabel : submitLabel}
        </button>
      </div>
    </>
  );
}
