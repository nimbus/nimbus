import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { DialogShell, SectionCard } from "./primitives";

export function DangerZoneSection() {
  const [rotateOpen, setRotateOpen] = useState(false);
  const [shutdownOpen, setShutdownOpen] = useState(false);
  return (
    <SectionCard
      title="Session lifecycle"
      testid="settings-danger-zone"
      description="Rotate the local admin token, or shut down the running server. Both actions invalidate the current session."
      tone="danger"
    >
      <div className="flex flex-wrap items-center gap-3">
        <button
          type="button"
          data-testid="settings-rotate-open"
          onClick={() => setRotateOpen(true)}
          className="rounded border border-danger bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] text-danger hover:bg-surface-2"
        >
          Rotate admin token
        </button>
        <button
          type="button"
          data-testid="settings-shutdown-open"
          onClick={() => setShutdownOpen(true)}
          className="rounded border border-danger bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] text-danger hover:bg-surface-2"
        >
          Shut down server
        </button>
        <p className="text-xs text-muted">
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
    </SectionCard>
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
    try {
      const res = await fetch("/api/system/token/rotate", {
        method: "POST",
        credentials: "include",
        headers: {
          Authorization: `Bearer ${token.trim()}`,
        },
      });
      const body = (await res.json()) as {
        generation?: number;
        error?: string;
      };
      if (!res.ok) {
        setError(body.error ?? `Rotation failed (${res.status}).`);
        setSubmitting(false);
        return;
      }
      setResult({ generation: body.generation ?? 0 });
      toast.success("Admin token rotated", {
        description: `New generation ${body.generation}. All other sessions invalidated.`,
      });
    } catch (e) {
      setError(`Rotation failed: ${(e as Error).message}`);
    } finally {
      setSubmitting(false);
    }
  }, [token]);

  return (
    <DialogShell
      title="Rotate admin token"
      onClose={onClose}
      testid="settings-rotate-dialog"
    >
      {result ? (
        <div className="space-y-3" data-testid="settings-rotate-result">
          <p className="text-sm text-default">
            New token issued (generation{" "}
            <span className="font-mono">{result.generation}</span>). Other
            sessions have been invalidated; this browser keeps its session until
            the next protected request.
          </p>
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-app bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] hover:border-strong"
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
            className="flex flex-col gap-1 text-xs text-muted"
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
              className="rounded border border-app bg-surface px-2 py-1 font-mono text-xs text-default focus:border-strong focus:outline-none"
              placeholder="Paste the value of `nimbus token show`"
            />
          </label>
          {error ? (
            <p
              className="text-xs text-danger"
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
              className="rounded border border-danger bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] text-danger hover:bg-surface-2 disabled:cursor-not-allowed disabled:text-muted"
            >
              {submitting ? "Rotating…" : "Rotate"}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="rounded border border-app bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] hover:border-strong"
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
    try {
      const res = await fetch("/api/system/shutdown", {
        method: "POST",
        credentials: "include",
      });
      const body = (await res.json()) as {
        accepted?: boolean;
        error?: string;
      };
      if (!res.ok) {
        setError(body.error ?? `Shutdown failed (${res.status}).`);
        setSubmitting(false);
        return;
      }
      setAccepted(true);
      toast("Shutdown requested", {
        description:
          "Server will close listeners. The disconnect overlay will appear shortly.",
      });
    } catch (e) {
      setError(`Shutdown failed: ${(e as Error).message}`);
    } finally {
      setSubmitting(false);
    }
  }, []);

  return (
    <DialogShell
      title="Shut down server"
      onClose={onClose}
      testid="settings-shutdown-dialog"
    >
      {accepted ? (
        <p
          className="text-sm text-default"
          data-testid="settings-shutdown-accepted"
        >
          Shutdown accepted. The WebSocket will drop and the disconnect overlay
          will take over the UI.
        </p>
      ) : (
        <div className="space-y-3">
          <p className="text-sm text-default">
            This will stop the running <code>nimbus start</code> process. All
            connected clients will disconnect. To restart, run{" "}
            <code>nimbus start</code> again from a terminal.
          </p>
          {error ? (
            <p
              className="text-xs text-danger"
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
              className="rounded border border-danger bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] text-danger hover:bg-surface-2 disabled:cursor-not-allowed disabled:text-muted"
            >
              {submitting ? "Stopping…" : "Confirm shutdown"}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="rounded border border-app bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] hover:border-strong"
            >
              Cancel
            </button>
          </div>
        </div>
      )}
    </DialogShell>
  );
}
