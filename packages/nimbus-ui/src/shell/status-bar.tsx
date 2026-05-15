import { useNimbus, useNimbusConnectionState, useQuery } from "nimbus/react";

import { api } from "../../convex/_generated/api";
import { CopyChip } from "../components/copy-chip";
import { Kbd } from "../components/kbd";
import { type ConnState, StateDot } from "../components/state-dot";
import { metaGlyph } from "../lib/platform";

type SystemStatus = {
  version?: string | null;
  buildHash?: string | null;
  activeTenant?: string | null;
} | null;

export function StatusBar() {
  const conn = useNimbusConnectionState();
  const status = useQuery(api.system.status, {}) as SystemStatus | undefined;
  const client = useNimbus();
  const serverUrl = client.url ?? deriveOrigin();
  const connState: ConnState = !conn.isWebSocketConnected
    ? conn.hasEverConnected
      ? "reconnecting"
      : "offline"
    : "connected";

  const connLabel =
    connState === "connected"
      ? "Connected"
      : connState === "reconnecting"
        ? "Reconnecting"
        : "Offline";

  const version = status?.version ?? "—";
  const buildHash = status?.buildHash ?? "";
  const tenant = status?.activeTenant ?? "_nimbus";

  return (
    <footer
      role="contentinfo"
      aria-label="Status bar"
      className="flex h-7 items-center gap-3 border-t border-app bg-surface px-3 text-xs font-mono text-muted"
    >
      <span
        className="inline-flex items-center gap-1.5"
        data-testid="status-connection"
      >
        <StateDot state={connState} />
        <span>{connLabel}</span>
      </span>
      <Divider />
      <CopyChip
        label="server URL"
        value={serverUrl}
        testid="status-server-url"
      />
      <Divider />
      <CopyChip
        label="version"
        value={`${version}${buildHash ? `+${buildHash.slice(0, 7)}` : ""}`}
        testid="status-version"
      />
      <Divider />
      <CopyChip label="tenant" value={tenant} testid="status-tenant" />
      {conn.hasInflightRequests ? (
        <>
          <Divider />
          <span data-testid="status-inflight" className="tabular">
            {conn.inflightMutations + conn.inflightActions} inflight
          </span>
        </>
      ) : null}
      <span className="ml-auto inline-flex items-center gap-3">
        <span className="inline-flex items-center gap-1">
          <Kbd>{metaGlyph}</Kbd>
          <Kbd>K</Kbd>
          <span className="text-muted">palette</span>
        </span>
        <span className="inline-flex items-center gap-1">
          <Kbd>{metaGlyph}</Kbd>
          <Kbd>\</Kbd>
          <span className="text-muted">system tenant lens</span>
        </span>
      </span>
    </footer>
  );
}

function Divider() {
  return (
    <span aria-hidden className="text-muted/40">
      ·
    </span>
  );
}

function deriveOrigin(): string {
  if (typeof window === "undefined") return "—";
  return window.location.origin;
}
