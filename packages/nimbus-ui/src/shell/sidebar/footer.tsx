import { useNimbusConnectionState, useQuery } from "@nimbus/nimbus/react";
import { Moon, PanelLeftClose, PanelLeftOpen, Sun } from "lucide-react";

import { api } from "../../../convex/_generated/api";
import { type ConnState, StateDot } from "../../components/state-dot";
import { useUiStore } from "../../store/ui-store";
import { RailTooltip, rowClass } from "./rail";

type SystemStatus = {
  version?: string | null;
} | null;

const CONN_LABELS: Record<ConnState, string> = {
  connected: "Connected",
  reconnecting: "Reconnecting",
  offline: "Offline",
};

// SidebarFooter holds the three things that are true of the whole console
// and not of any page: whether it is connected and to which version, which
// theme it is in, and how wide the sidebar is. The sheet omits the collapse
// control because a sheet has no rail to collapse to.
export function SidebarFooter({
  collapsed,
  inSheet,
  sidebarId,
  onCollapse,
}: {
  collapsed: boolean;
  inSheet: boolean;
  sidebarId: string;
  onCollapse: () => void;
}) {
  return (
    <div
      data-testid="sidebar-footer"
      className={
        collapsed
          ? "flex flex-col items-center gap-0.5 border-t border-border-1 px-3 py-2"
          : "flex flex-col gap-0.5 border-t border-border-1 p-2"
      }
    >
      <ConnectionStatus collapsed={collapsed} />
      <ThemeToggle collapsed={collapsed} inSheet={inSheet} />
      {inSheet ? null : (
        <CollapseButton
          collapsed={collapsed}
          sidebarId={sidebarId}
          onCollapse={onCollapse}
        />
      )}
    </div>
  );
}

function useConnection(): { state: ConnState; label: string } {
  const conn = useNimbusConnectionState();
  const state: ConnState = !conn.isWebSocketConnected
    ? conn.hasEverConnected
      ? "reconnecting"
      : "offline"
    : "connected";
  return { state, label: CONN_LABELS[state] };
}

function ConnectionStatus({ collapsed }: { collapsed: boolean }) {
  const { state, label } = useConnection();
  const status = useQuery(api.system.status, {}) as SystemStatus | undefined;
  const version = status?.version ? `v${status.version}` : null;
  const text = version ? `${label} · ${version}` : label;
  const row = (
    <div
      role="status"
      data-testid="sidebar-status"
      data-state={state}
      aria-label={text}
      className={rowClass({
        collapsed,
        className: "h-8 cursor-default hover:bg-transparent",
      })}
    >
      <span className="flex size-4 shrink-0 items-center justify-center">
        <StateDot state={state} />
      </span>
      {collapsed ? null : (
        <span className="truncate text-xs">
          {label}
          {version ? (
            <span className="font-mono text-text-4"> · {version}</span>
          ) : null}
        </span>
      )}
    </div>
  );
  return collapsed ? <RailTooltip label={text}>{row}</RailTooltip> : row;
}

// The toggle names the theme the console is in, not the one a click would
// give: "Dark theme" reads as a fact about the screen, and the tooltip and
// aria-label carry the action. It flips between light and dark only; the
// Settings page keeps the "follow the system" option.
export function ThemeToggle({
  collapsed,
  inSheet,
}: {
  collapsed: boolean;
  inSheet: boolean;
}) {
  const theme = useUiStore((s) => s.theme);
  const setThemeMode = useUiStore((s) => s.setThemeMode);
  const next = theme === "dark" ? "light" : "dark";
  const current = theme === "dark" ? "Dark theme" : "Light theme";
  const action = `Switch to ${next} theme`;
  const Icon = theme === "dark" ? Moon : Sun;
  const button = (
    <button
      type="button"
      onClick={() => setThemeMode(next)}
      aria-label={`${current}. ${action}`}
      data-testid="sidebar-theme-toggle"
      data-theme={theme}
      className={rowClass({
        collapsed,
        inSheet,
        className: inSheet ? "" : "h-8",
      })}
    >
      <Icon size={16} aria-hidden className="shrink-0" />
      {collapsed ? null : <span className="truncate text-xs">{current}</span>}
    </button>
  );
  return collapsed ? (
    <RailTooltip label={action}>{button}</RailTooltip>
  ) : (
    button
  );
}

function CollapseButton({
  collapsed,
  sidebarId,
  onCollapse,
}: {
  collapsed: boolean;
  sidebarId: string;
  onCollapse: () => void;
}) {
  const label = collapsed ? "Expand sidebar" : "Collapse sidebar";
  const Icon = collapsed ? PanelLeftOpen : PanelLeftClose;
  const button = (
    <button
      type="button"
      onClick={onCollapse}
      aria-expanded={!collapsed}
      aria-controls={sidebarId}
      aria-label={label}
      data-testid="sidebar-toggle"
      className={rowClass({ collapsed, className: "h-8" })}
    >
      <Icon size={16} aria-hidden className="shrink-0" />
      {collapsed ? null : <span className="truncate text-xs">Collapse</span>}
    </button>
  );
  return collapsed ? <RailTooltip label={label}>{button}</RailTooltip> : button;
}
