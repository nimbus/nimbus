import { type StateKind, statePalette } from "../../components/state-dot";
import { UpgradePopover } from "../../components/upgrade-popover";
import { useStalenessContext } from "../../hooks/use-staleness";
import { RailTooltip, rowClass } from "./rail";

// The upgrade row is the one sidebar row that is not always there. It
// appears under the connection line when the server is behind the latest
// release, follows the upgrade while it runs, and holds the "updated" line
// for a moment after. When the console is current there is nothing to say,
// and the row says nothing.
//
// Three tones, all read from the shared state palette rather than a private
// table, so an upgrade paints the same amber as any other transition in the
// console: pending (an update is waiting on the operator), starting (the
// upgrade is running), ready (it finished).
const UPGRADE_TONES = {
  available: "pending",
  upgrading: "starting",
  upgraded: "ready",
} satisfies Record<string, StateKind>;

export type UpgradeTone = keyof typeof UPGRADE_TONES;

// Exported for the drift-lock test: every tone must name a live palette
// entry, so a renamed StateKind fails a test instead of falling through to
// the unknown-state glyph.
export const UPGRADE_TONE_KINDS = UPGRADE_TONES;

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

export function UpgradeRow({ collapsed }: { collapsed: boolean }) {
  const {
    snapshot,
    isLocal,
    hasDesktopBridge,
    openPopover,
    closePopover,
    startUpgrade,
    copyCommand,
  } = useStalenessContext();
  const { state, info, targetLatest } = snapshot;
  if (state === "hidden" || !info) return null;

  if (state === "upgrading") {
    return (
      <StatusRow
        collapsed={collapsed}
        tone="upgrading"
        testid="sidebar-upgrade-upgrading"
        text={`Updating to ${targetLatest ?? info.latest}…`}
      />
    );
  }
  if (state === "upgraded") {
    return (
      <StatusRow
        collapsed={collapsed}
        tone="upgraded"
        testid="sidebar-upgrade-upgraded"
        text={`Updated to ${info.current}`}
      />
    );
  }

  const label = `Update to ${info.latest}`;
  const trigger = (
    <span data-testid="sidebar-upgrade-available" className="contents">
      <span className="flex size-4 shrink-0 items-center justify-center">
        <UpgradeDot tone="available" />
      </span>
      {collapsed ? null : <span className="truncate text-xs">{label}</span>}
    </span>
  );
  const popover = (
    <UpgradePopover
      open={state === "confirming"}
      onOpenChange={(open) => (open ? openPopover() : closePopover())}
      info={info}
      isLocal={isLocal}
      hasDesktopBridge={hasDesktopBridge}
      onUpdate={startUpgrade}
      onCopyCommand={copyCommand}
      trigger={trigger}
      triggerLabel={label}
      triggerClassName={rowClass({ collapsed, className: "h-8" })}
    />
  );
  // The rail tooltip wraps a plain box rather than the trigger itself: the
  // popover owns its trigger button, and the connection row above uses the
  // same wrapper, so the two rail rows name themselves the same way.
  return collapsed ? (
    <RailTooltip label={label}>
      <div className="flex">{popover}</div>
    </RailTooltip>
  ) : (
    popover
  );
}

function StatusRow({
  collapsed,
  tone,
  testid,
  text,
}: {
  collapsed: boolean;
  tone: UpgradeTone;
  testid: string;
  text: string;
}) {
  const row = (
    <div
      role="status"
      data-testid={testid}
      aria-label={text}
      className={rowClass({
        collapsed,
        className: "h-8 cursor-default hover:bg-transparent",
      })}
    >
      <span className="flex size-4 shrink-0 items-center justify-center">
        <UpgradeDot tone={tone} />
      </span>
      {collapsed ? null : <span className="truncate text-xs">{text}</span>}
    </div>
  );
  return collapsed ? <RailTooltip label={text}>{row}</RailTooltip> : row;
}
