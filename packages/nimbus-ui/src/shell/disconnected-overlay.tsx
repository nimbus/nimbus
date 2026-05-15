import { useNimbusConnectionState } from "nimbus/react";

import { StateDot } from "../components/state-dot";

export function DisconnectedOverlay() {
  const conn = useNimbusConnectionState();
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
      <span>
        Reconnecting · stale data shown, mutations disabled
      </span>
    </div>
  );
}
