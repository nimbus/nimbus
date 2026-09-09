import { useCallback, useState } from "react";
import { toast } from "sonner";

import { ConfirmDialog } from "../../../components/confirm-dialog";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { system } from "../../../lib/api-mutations";
import { PageSection } from "./-primitives";

// The phrase the operator types before the server accepts a shutdown from
// this console. It is the verb, so the dialog reads as the action it takes.
export const SHUTDOWN_PHRASE = "shutdown";

// Both writes reach past this browser: rotation signs every other session
// out, and shutdown drops every client. Each runs through ConfirmDialog with
// a typed proof, and the outcome lands next to the control that drew it, so
// the operator reads the result on the page instead of in a vanished dialog.
export function DangerZoneSection() {
  const [rotateOpen, setRotateOpen] = useState(false);
  const [shutdownOpen, setShutdownOpen] = useState(false);
  const [rotated, setRotated] = useState<{ generation: number } | null>(null);
  const [shutdownAccepted, setShutdownAccepted] = useState(false);
  return (
    <PageSection
      title="Session lifecycle"
      testid="settings-danger-zone"
      description="Rotate the local admin token, or shut down the running server. Both actions invalidate the current session."
      tone="danger"
      framed
    >
      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="outline"
          size="sm"
          className="border-error text-error hover:bg-error-tint"
          data-testid="settings-rotate-open"
          onClick={() => setRotateOpen(true)}
        >
          Rotate admin token
        </Button>
        <Button
          variant="outline"
          size="sm"
          className="border-error text-error hover:bg-error-tint"
          data-testid="settings-shutdown-open"
          onClick={() => setShutdownOpen(true)}
        >
          Shut down server
        </Button>
        <p className="text-xs text-text-3">
          Token rotation requires pasting the current admin bearer. Shutdown
          uses the active session cookie.
        </p>
      </div>
      {rotated ? (
        <p className="text-sm text-text-1" data-testid="settings-rotate-result">
          New token issued (generation{" "}
          <span className="font-mono">{rotated.generation}</span>). Other
          sessions are signed out; this browser keeps its session until the next
          protected request.
        </p>
      ) : null}
      {shutdownAccepted ? (
        <p
          className="text-sm text-text-1"
          data-testid="settings-shutdown-accepted"
        >
          Shutdown accepted. The connection will drop and the disconnect overlay
          will take over the console.
        </p>
      ) : null}
      <RotateTokenDialog
        open={rotateOpen}
        onClose={() => setRotateOpen(false)}
        onRotated={(generation) => {
          setRotated({ generation });
          setRotateOpen(false);
        }}
      />
      <ShutdownDialog
        open={shutdownOpen}
        onClose={() => setShutdownOpen(false)}
        onAccepted={() => {
          setShutdownAccepted(true);
          setShutdownOpen(false);
        }}
      />
    </PageSection>
  );
}

// The proof for rotation is the current bearer itself: the server refuses a
// rotation without it, so the field is the gate and Confirm stays inert
// until it holds something.
function RotateTokenDialog({
  open,
  onClose,
  onRotated,
}: {
  open: boolean;
  onClose: () => void;
  onRotated: (generation: number) => void;
}) {
  const [token, setToken] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | undefined>(undefined);

  const close = useCallback(() => {
    setToken("");
    setError(undefined);
    onClose();
  }, [onClose]);

  const submit = useCallback(async () => {
    const bearer = token.trim();
    if (!bearer) return;
    setSubmitting(true);
    setError(undefined);
    const result = await system.rotateToken(bearer);
    setSubmitting(false);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    const generation = result.data.generation ?? 0;
    toast.success("Admin token rotated", {
      description: `New generation ${generation}. All other sessions invalidated.`,
    });
    setToken("");
    onRotated(generation);
  }, [token, onRotated]);

  return (
    <ConfirmDialog
      open={open}
      title="Rotate admin token"
      description="Issues a new admin bearer and signs every other session out. Read the new token with nimbus token show."
      confirmLabel="Rotate token"
      danger
      busy={submitting}
      confirmDisabled={token.trim() === ""}
      error={error}
      onConfirm={() => void submit()}
      onCancel={close}
      testid="settings-rotate-dialog"
    >
      <label
        htmlFor="settings-rotate-token"
        className="flex flex-col gap-1 text-xs text-text-3"
      >
        <span>Current admin bearer</span>
        <Input
          id="settings-rotate-token"
          type="password"
          value={token}
          autoComplete="off"
          disabled={submitting}
          onChange={(e) => setToken(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && token.trim() !== "" && !submitting) {
              e.preventDefault();
              void submit();
            }
          }}
          data-testid="settings-rotate-token"
          className="font-mono"
          placeholder="Paste the token printed by nimbus token show"
        />
      </label>
    </ConfirmDialog>
  );
}

function ShutdownDialog({
  open,
  onClose,
  onAccepted,
}: {
  open: boolean;
  onClose: () => void;
  onAccepted: () => void;
}) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | undefined>(undefined);

  const close = useCallback(() => {
    setError(undefined);
    onClose();
  }, [onClose]);

  const submit = useCallback(async () => {
    setSubmitting(true);
    setError(undefined);
    const result = await system.shutdown();
    setSubmitting(false);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    toast("Shutdown requested", {
      description:
        "Server will close listeners. The disconnect overlay will appear shortly.",
    });
    onAccepted();
  }, [onAccepted]);

  return (
    <ConfirmDialog
      open={open}
      title="Shut down server"
      description={
        <>
          Stops the running <code>nimbus start</code> process and disconnects
          every client. To start again, run <code>nimbus start</code> from a
          terminal.
        </>
      }
      confirmLabel="Shut down"
      danger
      busy={submitting}
      typedConfirmation={{ phrase: SHUTDOWN_PHRASE }}
      error={error}
      onConfirm={() => void submit()}
      onCancel={close}
      testid="settings-shutdown-dialog"
    />
  );
}
