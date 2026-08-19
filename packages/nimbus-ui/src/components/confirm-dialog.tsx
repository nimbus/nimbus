import { useRef } from "react";

import { useModalFocus } from "../hooks/use-modal-focus";
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
  busy = false,
  onConfirm,
  onCancel,
  testid = "confirm-dialog",
}: ConfirmDialogProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);

  // `busy` means the confirmed work is already running, and the caller's loop
  // does not stop when the dialog closes — the storage bulk delete keeps
  // deleting every remaining document. Escape, the backdrop and the ✕ used to
  // dismiss anyway while both action buttons were frozen, which reads as
  // "cancelled": the modal is gone, nothing on the page says work is in
  // flight, and a "Deleted 25 documents" toast arrives seconds later. Every
  // informal exit runs through this guard, so while the work is in flight the
  // dialog stays and says what is happening.
  const dismiss = () => {
    if (busy) return;
    onCancel();
  };

  // Focus opens on Cancel, never on the confirm button. Every call site is
  // destructive — delete tenant, delete machine, drop schema, bulk delete
  // documents — so an operator who answers a dialog on reflex with Enter, or
  // who hits it before a screen reader has read the description, would commit
  // the deletion with one keystroke and no undo. There is no non-destructive
  // caller and no tone prop to switch on, so this is unconditional rather than
  // a choice the caller can get wrong.
  useModalFocus({
    open,
    panelRef,
    initialFocusRef: cancelRef,
    onEscape: dismiss,
  });

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
        onClick={dismiss}
        disabled={busy}
        className="absolute inset-0 cursor-default"
      />
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        data-testid={testid}
        // tabIndex -1 so the panel can hold focus if it ever has no focusable
        // child; outline-none so that lands without ringing the whole dialog.
        tabIndex={-1}
        className="relative z-10 w-full max-w-md rounded-md border border-app bg-surface p-4 shadow-lg outline-none"
      >
        <header className="mb-3 flex items-baseline justify-between">
          <h2 className="text-sm text-default">{title}</h2>
          <button
            type="button"
            onClick={dismiss}
            aria-disabled={busy}
            aria-label="Dismiss"
            className="font-mono text-xs text-muted hover:text-default aria-disabled:cursor-not-allowed"
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
        {/* `aria-disabled`, not `disabled`. `busy` flips while the dialog is
            open and focus is sitting on one of these two buttons, and a
            `disabled` attribute on the focused element hands focus to <body>:
            the operator was left with a "Working…" dialog on screen, nothing
            focused, and Tab restarting from the top of the page behind it.
            These keep their tab stop and announce the state instead; the
            handlers are what refuse the action. */}
        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            ref={cancelRef}
            onClick={dismiss}
            aria-disabled={busy}
            data-testid={`${testid}-cancel`}
            className="rounded border border-app bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] text-default hover:border-strong aria-disabled:cursor-not-allowed aria-disabled:text-muted"
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            onClick={() => {
              if (busy) return;
              onConfirm();
            }}
            aria-disabled={busy}
            data-testid={`${testid}-confirm`}
            className={cn(
              "rounded border bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] aria-disabled:cursor-not-allowed aria-disabled:text-muted",
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
