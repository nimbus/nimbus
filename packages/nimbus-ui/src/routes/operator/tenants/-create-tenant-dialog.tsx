import { type FormEvent, useId, useRef, useState } from "react";

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

// CreateTenantDialog is the one way the console creates a tenant. The
// server owns the id rule (nimbus-core: ASCII letters, digits, `_` and `-`,
// at most 128 characters, a leading `_` reserved), so the dialog states the
// rule once under the field and lets the server refuse an id it does not
// accept. The refusal lands in the error strip and the dialog stays open
// with the typed id in place, so the operator can fix it rather than start
// over. While the write is in flight the dialog cannot be dismissed and
// both buttons stay in the tab order with `aria-disabled`.
export function CreateTenantDialog({
  open,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  open: boolean;
  busy: boolean;
  error?: string;
  onSubmit: (id: string) => void;
  onCancel: () => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !busy) onCancel();
      }}
    >
      <DialogContent
        data-testid="tenants-create-dialog"
        initialFocus={inputRef}
        showCloseButton={!busy}
      >
        {/* The form mounts with the dialog, so a reopened dialog starts
            with an empty field and no stale refusal. */}
        <CreateTenantForm
          inputRef={inputRef}
          busy={busy}
          error={error}
          onSubmit={onSubmit}
          onCancel={onCancel}
        />
      </DialogContent>
    </Dialog>
  );
}

function CreateTenantForm({
  inputRef,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  inputRef: React.RefObject<HTMLInputElement | null>;
  busy: boolean;
  error?: string;
  onSubmit: (id: string) => void;
  onCancel: () => void;
}) {
  const [id, setId] = useState("");
  const inputId = useId();
  const hintId = useId();
  const trimmed = id.trim();
  const inert = busy || trimmed.length === 0;
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!inert) onSubmit(trimmed);
  };
  return (
    <form
      onSubmit={submit}
      className="contents"
      data-testid="tenants-create-form"
    >
      <DialogHeader>
        <DialogTitle>Create tenant</DialogTitle>
        <DialogDescription>
          A tenant owns its own tables, documents, and services.
        </DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-1.5">
        <label htmlFor={inputId} className="text-xs font-medium text-text-3">
          Tenant id
        </label>
        <Input
          ref={inputRef}
          id={inputId}
          value={id}
          onChange={(event) => setId(event.target.value)}
          autoComplete="off"
          spellCheck={false}
          disabled={busy}
          placeholder="acme"
          aria-describedby={hintId}
          aria-invalid={error ? true : undefined}
          className="font-mono"
          data-testid="tenants-create-input"
        />
        <p id={hintId} className="text-xs text-text-3">
          ASCII letters, digits, <code className="text-text-2">_</code> and{" "}
          <code className="text-text-2">-</code>, up to 128 characters. A
          leading <code className="text-text-2">_</code> is reserved for the
          system tenant.
        </p>
      </DialogBody>
      <DialogError>{error}</DialogError>
      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-disabled={busy}
          onClick={() => {
            if (!busy) onCancel();
          }}
          data-testid="tenants-create-cancel"
        >
          Cancel
        </Button>
        <Button
          type="submit"
          size="sm"
          aria-disabled={inert}
          data-testid="tenants-create-submit"
        >
          {busy ? "Creating…" : "Create"}
        </Button>
      </DialogFooter>
    </form>
  );
}
