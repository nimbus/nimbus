import { type ReactNode, useRef } from "react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogBody,
  DialogContent,
  DialogDescription,
  DialogError,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

// ConfirmDialog is the one shape for a write the operator cannot take back.
// The title names the verb, the body restates the object and what the write
// reaches, and the footer keeps Cancel first and focused, so Enter on an
// unread dialog does nothing. While the write is in flight the dialog stays
// open, every dismissal is refused, and both buttons stay in the tab order
// with `aria-disabled` so focus does not fall off the page. A refused write
// lands in the error strip next to the control that drew it.
export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  cancelLabel = "Cancel",
  danger = false,
  busy = false,
  error,
  onConfirm,
  onCancel,
  testid = "confirm-dialog",
  children,
}: {
  open: boolean;
  title: string;
  description?: ReactNode;
  confirmLabel: string;
  cancelLabel?: string;
  danger?: boolean;
  busy?: boolean;
  error?: string;
  onConfirm: () => void;
  onCancel: () => void;
  testid?: string;
  children?: ReactNode;
}) {
  const cancelRef = useRef<HTMLButtonElement>(null);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !busy) onCancel();
      }}
    >
      <DialogContent
        data-testid={testid}
        initialFocus={cancelRef}
        showCloseButton={!busy}
      >
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description && (
            <DialogDescription data-testid={`${testid}-description`}>
              {description}
            </DialogDescription>
          )}
        </DialogHeader>
        {(children || danger) && (
          <DialogBody className="flex flex-col gap-2 text-sm text-text-2">
            {children}
            {danger && <p className="text-xs text-text-3">There is no undo.</p>}
          </DialogBody>
        )}
        <DialogError>{error}</DialogError>
        <DialogFooter>
          <Button
            ref={cancelRef}
            type="button"
            variant="outline"
            size="sm"
            data-testid={`${testid}-cancel`}
            aria-disabled={busy}
            onClick={() => {
              if (!busy) onCancel();
            }}
          >
            {cancelLabel}
          </Button>
          <Button
            type="button"
            variant={danger ? "destructive" : "default"}
            size="sm"
            data-testid={`${testid}-confirm`}
            aria-disabled={busy}
            onClick={() => {
              if (!busy) onConfirm();
            }}
          >
            {busy ? "Working…" : confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
