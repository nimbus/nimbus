import { type ReactNode, useId, useRef, useState } from "react";

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
import { Input } from "@/components/ui/input";

// ConfirmDialog is the one shape for a write the operator cannot take back.
// The title names the verb, the body restates the object and what the write
// reaches, and the footer keeps Cancel first and focused, so Enter on an
// unread dialog does nothing. While the write is in flight the dialog stays
// open, every dismissal is refused, and both buttons stay in the tab order
// with `aria-disabled` so focus does not fall off the page. A refused write
// lands in the error strip next to the control that drew it.
//
// A write that reaches past this browser (a server shutdown, a token that
// signs every session out) adds a typed confirmation: the dialog asks for a
// phrase and keeps Confirm inert until the phrase matches, so a stray click
// cannot land the write. `confirmDisabled` is the same gate for a dialog
// whose proof is its own field, such as a bearer the server must check.
export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  cancelLabel = "Cancel",
  danger = false,
  busy = false,
  confirmDisabled = false,
  typedConfirmation,
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
  confirmDisabled?: boolean;
  // The phrase the operator types before Confirm becomes live.
  typedConfirmation?: { phrase: string };
  error?: string;
  onConfirm: () => void;
  onCancel: () => void;
  testid?: string;
  children?: ReactNode;
}) {
  const cancelRef = useRef<HTMLButtonElement>(null);
  const [typed, setTyped] = useState("");
  const typedId = useId();
  const typedMatches =
    typedConfirmation === undefined ||
    typed.trim() === typedConfirmation.phrase;
  const inert = busy || confirmDisabled || !typedMatches;
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
        {(children || danger || typedConfirmation) && (
          <DialogBody className="flex flex-col gap-3 text-sm text-text-2">
            {children}
            {typedConfirmation && (
              <label
                htmlFor={typedId}
                className="flex flex-col gap-1 text-xs text-text-3"
              >
                <span>
                  Type{" "}
                  <code className="text-text-1">
                    {typedConfirmation.phrase}
                  </code>{" "}
                  to confirm
                </span>
                <Input
                  id={typedId}
                  value={typed}
                  autoComplete="off"
                  spellCheck={false}
                  disabled={busy}
                  onChange={(e) => setTyped(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !inert) {
                      e.preventDefault();
                      onConfirm();
                    }
                  }}
                  data-testid={`${testid}-typed`}
                  className="font-mono"
                />
              </label>
            )}
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
            aria-disabled={inert}
            onClick={() => {
              if (!inert) onConfirm();
            }}
          >
            {busy ? "Working…" : confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
