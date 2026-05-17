import { useNimbus, useNimbusConnectionState, useQuery } from "nimbus/react";

import { api } from "../../convex/_generated/api";
import { CopyChip } from "../components/copy-chip";
import { Kbd } from "../components/kbd";
import { type ConnState, StateDot } from "../components/state-dot";
import { UpgradePopover } from "../components/upgrade-popover";
import { useStalenessContext } from "../hooks/use-staleness";
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

  const staleness = useStalenessContext();
  const baseValue = `${version}${buildHash ? `+${buildHash.slice(0, 7)}` : ""}`;

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
      <VersionSlot
        baseValue={baseValue}
        currentVersion={version}
        staleness={staleness}
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

function VersionSlot({
  baseValue,
  currentVersion,
  staleness,
}: {
  baseValue: string;
  currentVersion: string;
  staleness: ReturnType<typeof useStalenessContext>;
}) {
  const { snapshot, openPopover, closePopover, startUpgrade, copyCommand } =
    staleness;
  const { state, info, targetLatest } = snapshot;

  if (state === "hidden" || !info) {
    return (
      <CopyChip label="version" value={baseValue} testid="status-version" />
    );
  }

  if (state === "upgrading") {
    return (
      <span
        role="status"
        aria-live="polite"
        data-testid="status-version-upgrading"
        className="inline-flex items-center gap-1.5"
      >
        <UpgradeDot tone="starting" />
        <span className="text-default">
          Updating to {targetLatest ?? info.latest}…
        </span>
      </span>
    );
  }

  if (state === "upgraded") {
    return (
      <span
        role="status"
        aria-live="polite"
        data-testid="status-version-upgraded"
        className="inline-flex items-center gap-1.5"
      >
        <UpgradeDot tone="success" />
        <span className="text-default">{baseValue}</span>
      </span>
    );
  }

  // available or confirming — both render the actionable row; popover state
  // determines whether the popup is mounted next to it.
  const open = state === "confirming";
  return (
    <span
      role="status"
      aria-live="polite"
      data-testid="status-version-available"
      className="inline-flex items-center"
    >
      <UpgradePopover
        open={open}
        onOpenChange={(next) => {
          if (next) openPopover();
          else closePopover();
        }}
        info={info}
        isLocal={staleness.isLocal}
        hasDesktopBridge={staleness.hasDesktopBridge}
        onUpdate={startUpgrade}
        onCopyCommand={copyCommand}
        trigger={
          <>
            <UpgradeDot tone="accent" />
            <span className="text-default">v{currentVersion}</span>
            <span className="text-muted">·</span>
            <span className="text-default">update to {info.latest} →</span>
          </>
        }
      />
    </span>
  );
}

function UpgradeDot({ tone }: { tone: "accent" | "starting" | "success" }) {
  const color =
    tone === "accent"
      ? "var(--color-brand)"
      : tone === "starting"
        ? "var(--color-starting)"
        : "var(--color-success)";
  return (
    <span
      aria-hidden
      className={`inline-block size-2 rounded-full ${tone === "starting" ? "animate-pulse" : ""}`}
      style={{ background: color }}
    />
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
