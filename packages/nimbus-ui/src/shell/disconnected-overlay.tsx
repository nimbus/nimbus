import { useNimbusConnectionState } from "@nimbus/nimbus/react";
import { useEffect } from "react";

import { StateDot } from "../components/state-dot";

const DEFAULT_SESSION_PROBE_INTERVAL_MS = 1_000;
const DEFAULT_SESSION_PROBE_TIMEOUT_MS = 3_000;

export type SessionProbeResult =
  | "authorized"
  | "reauthenticate"
  | "unreachable";

export interface DisconnectedOverlayProps {
  readonly probeSession?: () => Promise<SessionProbeResult>;
  readonly onSessionExpired?: () => void;
  readonly probeIntervalMs?: number;
}

export async function probeUiSession(
  fetchImpl: typeof fetch = globalThis.fetch,
  timeoutMs = DEFAULT_SESSION_PROBE_TIMEOUT_MS,
): Promise<SessionProbeResult> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetchImpl("/ui/", {
      method: "GET",
      credentials: "same-origin",
      redirect: "manual",
      cache: "no-store",
      signal: controller.signal,
    });
    if (
      response.status === 401 ||
      response.status === 403 ||
      (response.status >= 300 && response.status < 400) ||
      response.type === "opaqueredirect"
    ) {
      return "reauthenticate";
    }
    return "authorized";
  } catch {
    return "unreachable";
  } finally {
    clearTimeout(timeout);
  }
}

function navigateToAuth(): void {
  window.location.assign("/ui/auth");
}

export function DisconnectedOverlay({
  probeSession = probeUiSession,
  onSessionExpired = navigateToAuth,
  probeIntervalMs = DEFAULT_SESSION_PROBE_INTERVAL_MS,
}: DisconnectedOverlayProps = {}) {
  const conn = useNimbusConnectionState();

  useEffect(() => {
    if (conn.isWebSocketConnected || !conn.hasEverConnected) return;

    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const schedule = () => {
      timer = setTimeout(() => {
        void runProbe();
      }, probeIntervalMs);
    };
    const runProbe = async () => {
      const result = await probeSession();
      if (cancelled) return;
      if (result === "reauthenticate") {
        onSessionExpired();
        return;
      }
      schedule();
    };
    schedule();

    return () => {
      cancelled = true;
      if (timer !== null) clearTimeout(timer);
    };
  }, [
    conn.hasEverConnected,
    conn.isWebSocketConnected,
    onSessionExpired,
    probeIntervalMs,
    probeSession,
  ]);

  if (conn.isWebSocketConnected) return null;
  if (!conn.hasEverConnected) {
    // Initial load — wait for the first connection attempt to complete.
    return null;
  }
  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="disconnected-overlay"
      className="pointer-events-none fixed left-1/2 top-3 z-30 flex -translate-x-1/2 items-center gap-2 rounded-full border bg-surface px-3 py-1 text-xs font-mono shadow border-app text-default"
    >
      <StateDot state="reconnecting" />
      <span>Reconnecting · stale data shown, mutations disabled</span>
    </div>
  );
}
