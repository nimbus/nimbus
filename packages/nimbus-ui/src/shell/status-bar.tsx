import {
  useNimbus,
  useNimbusConnectionState,
  useQuery,
} from "@nimbus/nimbus/react";

import { api } from "../../convex/_generated/api";
import { CopyChip } from "../components/copy-chip";
import { Kbd } from "../components/kbd";
import { type StateKind, statePalette } from "../components/state-chip";
import { type ConnState, StateDot } from "../components/state-dot";
import { UpgradePopover } from "../components/upgrade-popover";
import { useStalenessContext } from "../hooks/use-staleness";
import { metaGlyph } from "../lib/platform";
import { useViewportTier } from "./use-viewport-tier";

type SystemStatus = {
  version?: string | null;
  buildHash?: string | null;
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

  const staleness = useStalenessContext();
  const baseValue = `${version}${buildHash ? `+${buildHash.slice(0, 7)}` : ""}`;
  const tier = useViewportTier();

  return (
    <footer
      role="contentinfo"
      aria-label="Status bar"
      // The band is a hard 28px, so the row has to refuse to grow into a
      // second line. Without `whitespace-nowrap` the multi-word hints wrapped
      // to three lines and the text spilled over the page above the bar and
      // past the viewport below it; without `overflow-hidden` that spill
      // reached the document and put a horizontal scrollbar on the whole
      // shell. DESIGN.md: "The bar never wraps. Truncate aggressively; rely
      // on title attributes for full values."
      className="flex h-[var(--statusbar-height)] items-center justify-between gap-3 overflow-hidden whitespace-nowrap border-t border-app bg-surface px-3 text-xs font-mono text-muted"
    >
      {/* Left: keyboard hints (least important — Chrome's link-hover URL
          preview covers this corner, so the connection/url/tenant info lives
          on the right where it stays readable). Dropped entirely at the mobile
          tier: a touch viewport has no ⌘ key to press, and at 390px three
          hints crowd out the connection state and server URL the bar exists
          to show. */}
      {tier === "mobile" ? null : (
        // min-w-0 is what lets a flex item shrink past its min-content width,
        // which for a multi-word label is a whole word. The hints are the
        // group that gives way, so the connection state and URL keep their
        // room.
        <span
          className="inline-flex min-w-0 items-center gap-3 overflow-hidden"
          data-testid="status-hints"
        >
          <span className="inline-flex items-center gap-1">
            <Kbd>{metaGlyph}</Kbd>
            <Kbd>\</Kbd>
            <span className="text-muted">system tenant lens</span>
          </span>
          <span className="inline-flex items-center gap-1">
            <Kbd>{metaGlyph}</Kbd>
            <Kbd>K</Kbd>
            <span className="text-muted">palette</span>
          </span>
          <span className="inline-flex items-center gap-1">
            <Kbd>/</Kbd>
            <span className="text-muted">filter</span>
          </span>
        </span>
      )}
      {/* Right: connection status, server URL, version (the info that matters,
          kept clear of the bottom-left link-hover URL preview). `shrink-0`
          because this group must not be the one that gives way; the server URL
          inside it already truncates to 28ch and carries the full value in its
          title. */}
      <span className="inline-flex shrink-0 items-center gap-3">
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
        <VersionSlot
          baseValue={baseValue}
          currentVersion={version}
          staleness={staleness}
        />
        {conn.hasInflightRequests ? (
          <>
            <Divider />
            <span data-testid="status-inflight" className="tabular">
              {conn.inflightMutations + conn.inflightActions} inflight
            </span>
          </>
        ) : null}
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

  // The steady-state version lives in the top nav (`nimbus v…`); this footer
  // slot only appears when there is an actionable upgrade.
  if (state === "hidden" || !info) {
    return null;
  }

  if (state === "upgrading") {
    return (
      <>
        <Divider />
        <span
          role="status"
          aria-live="polite"
          data-testid="status-version-upgrading"
          className="inline-flex items-center gap-1.5"
        >
          <UpgradeDot tone="upgrading" />
          <span className="text-default">
            Updating to {targetLatest ?? info.latest}…
          </span>
        </span>
      </>
    );
  }

  if (state === "upgraded") {
    return (
      <>
        <Divider />
        <span
          role="status"
          aria-live="polite"
          data-testid="status-version-upgraded"
          className="inline-flex items-center gap-1.5"
        >
          <UpgradeDot tone="upgraded" />
          <span className="text-default">{baseValue}</span>
        </span>
      </>
    );
  }

  // available or confirming — both render the actionable row; popover state
  // determines whether the popup is mounted next to it.
  const open = state === "confirming";
  return (
    <>
      <Divider />
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
              <UpgradeDot tone="available" />
              <span className="text-default">v{currentVersion}</span>
              <span className="text-muted">·</span>
              <span className="text-default">update to {info.latest} →</span>
            </>
          }
        />
      </span>
    </>
  );
}

// Upgrade tones are states, so the colour comes from the shared `statePalette`
// in components/state-chip.tsx. A private tone->colour table here was the third
// copy of that binding in the console, after StateChip and StateDot; three
// tables owning one vocabulary is how they drift apart.
//
// The prop names the upgrade state rather than a colour. A tone called
// "accent" that painted `--brand` is precisely how the drift started:
// `--brand` is identity and has no entry in the state table, while an upgrade
// that is offered but not applied is genuinely `pending`.
//
// No tone pulses. DESIGN.md grants the pulse to `Running` alone, and a
// permanent pulse in the always-on-screen footer is the opposite of calm.
const UPGRADE_TONES = {
  available: "pending",
  upgrading: "starting",
  upgraded: "ready",
} as const satisfies Record<string, StateKind>;

export type UpgradeTone = keyof typeof UPGRADE_TONES;

function UpgradeDot({ tone }: { tone: UpgradeTone }) {
  const kind = UPGRADE_TONES[tone];
  return (
    <span
      aria-hidden
      data-state={kind}
      className="inline-block size-2 rounded-full"
      style={{ background: `var(${statePalette[kind].token})` }}
    />
  );
}

// Exported for the drift-lock test: it asserts every tone resolves to a real
// entry in the shared table, so a renamed or deleted StateKind fails here
// instead of silently falling back to an unknown-state glyph.
export const UPGRADE_TONE_KINDS = UPGRADE_TONES;

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
