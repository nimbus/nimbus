import { useEffect, useRef } from "react";

import { cn } from "../lib/cn";

export type ConfirmDialogProps = {
  open: boolean;
  title: string;
  description?: React.ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  danger?: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  testid?: string;
};

export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  cancelLabel = "Cancel",
  danger,
  busy,
  onConfirm,
  onCancel,
  testid = "confirm-dialog",
}: ConfirmDialogProps) {
  const confirmRef = useRef<HTMLButtonElement>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) return;
    previouslyFocusedRef.current =
      (document.activeElement as HTMLElement | null) ?? null;
    confirmRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onCancel();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      previouslyFocusedRef.current?.focus?.();
    };
  }, [open, onCancel]);

  if (!open) return null;

  const confirmTone = danger
    ? "border-danger text-danger hover:bg-surface-2"
    : "border-app text-default hover:bg-surface-2";

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 px-4"
      data-testid={`${testid}-backdrop`}
    >
      <button
        type="button"
        aria-label="Close dialog"
        onClick={onCancel}
        className="absolute inset-0 cursor-default"
      />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        data-testid={testid}
        className="relative z-10 w-full max-w-md rounded-md border border-app bg-surface p-4 shadow-lg"
      >
        <header className="mb-3 flex items-baseline justify-between">
          <h2 className="text-sm text-default">{title}</h2>
          <button
            type="button"
            onClick={onCancel}
            aria-label="Dismiss"
            className="font-mono text-xs text-muted hover:text-default"
          >
            ✕
          </button>
        </header>
        {description ? (
          <div
            className="mb-4 text-sm text-default"
            data-testid={`${testid}-description`}
          >
            {description}
          </div>
        ) : null}
        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            disabled={busy}
            data-testid={`${testid}-cancel`}
            className="rounded border border-app bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] text-default hover:border-strong disabled:cursor-not-allowed disabled:text-muted"
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            ref={confirmRef}
            onClick={onConfirm}
            disabled={busy}
            data-testid={`${testid}-confirm`}
            className={cn(
              "rounded border bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] disabled:cursor-not-allowed disabled:text-muted",
              confirmTone,
            )}
          >
            {busy ? "Working…" : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
