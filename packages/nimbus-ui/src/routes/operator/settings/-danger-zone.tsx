import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { system } from "../../../lib/api-mutations";
import { DialogShell, PageSection } from "./-primitives";

export function DangerZoneSection() {
  const [rotateOpen, setRotateOpen] = useState(false);
  const [shutdownOpen, setShutdownOpen] = useState(false);
  return (
    <PageSection
      title="Session lifecycle"
      testid="settings-danger-zone"
      description="Rotate the local admin token, or shut down the running server. Both actions invalidate the current session."
      tone="danger"
      framed
    >
      <div className="flex flex-wrap items-center gap-3">
        <button
          type="button"
          data-testid="settings-rotate-open"
          onClick={() => setRotateOpen(true)}
          className="rounded-xs border border-error bg-bg-panel px-3 py-1.5 text-xs font-medium text-error hover:bg-bg-raised"
        >
          Rotate admin token
        </button>
        <button
          type="button"
          data-testid="settings-shutdown-open"
          onClick={() => setShutdownOpen(true)}
          className="rounded-xs border border-error bg-bg-panel px-3 py-1.5 text-xs font-medium text-error hover:bg-bg-raised"
        >
          Shut down server
        </button>
        <p className="text-xs text-text-3">
          Token rotation requires pasting the current admin bearer. Shutdown
          uses the active session cookie.
        </p>
      </div>
      {rotateOpen ? (
        <RotateTokenDialog onClose={() => setRotateOpen(false)} />
      ) : null}
      {shutdownOpen ? (
        <ShutdownDialog onClose={() => setShutdownOpen(false)} />
      ) : null}
    </PageSection>
  );
}

function RotateTokenDialog({ onClose }: { onClose: () => void }) {
  const [token, setToken] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<{ generation: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const tokenInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    tokenInputRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const submit = useCallback(async () => {
    if (!token.trim()) {
      setError("Paste the current admin bearer token to confirm rotation.");
      return;
    }
    setSubmitting(true);
    setError(null);
    const result = await system.rotateToken(token.trim());
    if (!result.ok) {
      setError(result.error);
      setSubmitting(false);
      return;
    }
    setResult({ generation: result.data.generation ?? 0 });
    toast.success("Admin token rotated", {
      description: `New generation ${result.data.generation}. All other sessions invalidated.`,
    });
    setSubmitting(false);
  }, [token]);

  return (
    <DialogShell
      title="Rotate admin token"
      onClose={onClose}
      testid="settings-rotate-dialog"
    >
      {result ? (
        <div className="space-y-3" data-testid="settings-rotate-result">
          <p className="text-sm text-text-1">
            New token issued (generation{" "}
            <span className="font-mono">{result.generation}</span>). Other
            sessions have been invalidated; this browser keeps its session until
            the next protected request.
          </p>
          <button
            type="button"
            onClick={onClose}
            className="rounded-xs border border-border-2 bg-bg-panel px-3 py-1.5 text-xs font-medium hover:border-border-3"
          >
            Close
          </button>
        </div>
      ) : (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
          className="space-y-3"
        >
          <label
            htmlFor="settings-rotate-token"
            className="flex flex-col gap-1 text-xs text-text-3"
          >
            <span>Current admin bearer</span>
            <input
              ref={tokenInputRef}
              id="settings-rotate-token"
              type="password"
              value={token}
              autoComplete="off"
              onChange={(e) => setToken(e.target.value)}
              data-testid="settings-rotate-token"
              // No `focus:outline-none`: `--border` -> `--border-strong` is
              // a 1.35:1 -> 1.74:1 shift on this white field in warm light,
              // which is not a focus indicator on its own. The tint stays as
              // emphasis; the console-wide `:focus-visible` outline it used to
              // cancel is what marks focus.
              className="rounded-xs border border-border-2 bg-bg-panel px-2 py-1 font-mono text-xs text-text-1 focus-visible:border-accent"
              placeholder="Paste the token printed by nimbus token show"
            />
          </label>
          {error ? (
            <p
              className="text-xs text-error"
              data-testid="settings-rotate-error"
            >
              {error}
            </p>
          ) : null}
          <div className="flex items-center gap-2">
            <button
              type="submit"
              data-testid="settings-rotate-submit"
              disabled={submitting}
              className="rounded-xs border border-error bg-bg-panel px-3 py-1.5 text-xs font-medium text-error hover:bg-bg-raised disabled:cursor-not-allowed disabled:text-text-3"
            >
              {submitting ? "Rotating…" : "Rotate"}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="rounded-xs border border-border-2 bg-bg-panel px-3 py-1.5 text-xs font-medium hover:border-border-3"
            >
              Cancel
            </button>
          </div>
        </form>
      )}
    </DialogShell>
  );
}

function ShutdownDialog({ onClose }: { onClose: () => void }) {
  const [submitting, setSubmitting] = useState(false);
  const [accepted, setAccepted] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const submit = useCallback(async () => {
    setSubmitting(true);
    setError(null);
    const result = await system.shutdown();
    if (!result.ok) {
      setError(result.error);
      setSubmitting(false);
      return;
    }
    setAccepted(true);
    toast("Shutdown requested", {
      description:
        "Server will close listeners. The disconnect overlay will appear shortly.",
    });
    setSubmitting(false);
  }, []);

  return (
    <DialogShell
      title="Shut down server"
      onClose={onClose}
      testid="settings-shutdown-dialog"
    >
      {accepted ? (
        <p
          className="text-sm text-text-1"
          data-testid="settings-shutdown-accepted"
        >
          Shutdown accepted. The WebSocket will drop and the disconnect overlay
          will take over the UI.
        </p>
      ) : (
        <div className="space-y-3">
          <p className="text-sm text-text-1">
            This will stop the running <code>nimbus start</code> process. All
            connected clients will disconnect. To restart, run{" "}
            <code>nimbus start</code> again from a terminal.
          </p>
          {error ? (
            <p
              className="text-xs text-error"
              data-testid="settings-shutdown-error"
            >
              {error}
            </p>
          ) : null}
          <div className="flex items-center gap-2">
            <button
              type="button"
              data-testid="settings-shutdown-submit"
              onClick={() => void submit()}
              disabled={submitting}
              className="rounded-xs border border-error bg-bg-panel px-3 py-1.5 text-xs font-medium text-error hover:bg-bg-raised disabled:cursor-not-allowed disabled:text-text-3"
            >
              {submitting ? "Stopping…" : "Confirm shutdown"}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="rounded-xs border border-border-2 bg-bg-panel px-3 py-1.5 text-xs font-medium hover:border-border-3"
            >
              Cancel
            </button>
          </div>
        </div>
      )}
    </DialogShell>
  );
}
