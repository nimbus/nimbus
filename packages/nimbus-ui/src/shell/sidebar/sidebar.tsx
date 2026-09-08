import { Link, useRouterState } from "@tanstack/react-router";
import { useState } from "react";

import { Mascot } from "@/components/mascot";
import { cn } from "@/lib/utils";
import { useUiStore } from "../../store/ui-store";
import { viewFromPathname } from "../nav-entries";
import { useViewportTier, type ViewportTier } from "../use-viewport-tier";
import { SidebarFooter } from "./footer";
import { NavGroups } from "./nav-groups";
import { ScopeRow } from "./scope-row";

export const SIDEBAR_ID = "sidebar";

// Sidebar is the console's one navigation column: brand, scope, rows,
// footer. At 240px it is the full thing; at 64px it is the same thing with
// its words in tooltips. Below 640px `MobileTopBar` puts the same body in a
// sheet and this component is not mounted at all.
export function Sidebar() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const view = viewFromPathname(pathname);
  const { collapsed, toggle, expand, tier } = useSidebarCollapse();
  return (
    <aside
      id={SIDEBAR_ID}
      aria-label="Sidebar"
      data-testid="sidebar"
      data-view={view}
      data-collapsed={collapsed ? "true" : "false"}
      data-tier={tier}
      className={cn(
        "flex h-full shrink-0 flex-col border-r border-border-2 bg-bg-panel transition-[width] duration-150 ease-standard",
        collapsed ? "w-16" : "w-60",
      )}
    >
      <SidebarBody
        collapsed={collapsed}
        inSheet={false}
        onCollapse={toggle}
        onExpand={expand}
      />
    </aside>
  );
}

export function SidebarBody({
  collapsed,
  inSheet,
  onCollapse,
  onExpand,
  onNavigate,
}: {
  collapsed: boolean;
  inSheet: boolean;
  onCollapse: () => void;
  onExpand: () => void;
  onNavigate?: () => void;
}) {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const view = viewFromPathname(pathname);
  return (
    <>
      <Brand collapsed={collapsed} onNavigate={onNavigate} />
      <ScopeRow
        view={view}
        collapsed={collapsed}
        inSheet={inSheet}
        onExpand={onExpand}
      />
      <NavGroups
        view={view}
        collapsed={collapsed}
        inSheet={inSheet}
        onNavigate={onNavigate}
      />
      <SidebarFooter
        collapsed={collapsed}
        inSheet={inSheet}
        sidebarId={SIDEBAR_ID}
        onCollapse={onCollapse}
      />
    </>
  );
}

// The brand row is a link home for the current view, so the mascot does what
// every logo in every app does. Collapsed, only the face remains.
function Brand({
  collapsed,
  onNavigate,
}: {
  collapsed: boolean;
  onNavigate?: () => void;
}) {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const view = viewFromPathname(pathname);
  return (
    <div
      className={cn(
        "flex h-14 shrink-0 items-center",
        collapsed ? "justify-center px-0" : "px-4",
      )}
    >
      <Link
        to={`/${view}`}
        onClick={onNavigate}
        aria-label="Nimbus home"
        data-testid="sidebar-brand"
        className="flex items-center gap-2.5 rounded-sm text-text-1 outline-none"
      >
        <Mascot size={28} decorative />
        {collapsed ? null : (
          <span className="text-base font-semibold tracking-tight">nimbus</span>
        )}
      </Link>
    </div>
  );
}

// Below the desktop tier the sidebar defaults to the rail: at 768px the
// expanded column plus the sub-panel own a third of the viewport. The stored
// preference is only consulted on desktop, and a tablet-width expand is held
// in ephemeral state so it never overwrites that preference. The override
// carries the tier it was made in, so crossing a breakpoint discards it
// during render instead of through a reset effect.
export function useSidebarCollapse(): {
  collapsed: boolean;
  toggle: () => void;
  expand: () => void;
  tier: ViewportTier;
} {
  const tier = useViewportTier();
  const stored = useUiStore((s) => s.sidebarCollapsed);
  const setSidebarCollapsed = useUiStore((s) => s.setSidebarCollapsed);
  const [override, setOverride] = useState<{
    tier: ViewportTier;
    value: boolean;
  } | null>(null);
  const active = override?.tier === tier ? override.value : null;
  const collapsed = active ?? (tier === "desktop" ? stored : true);
  const set = (value: boolean) => {
    if (tier === "desktop") {
      setOverride(null);
      setSidebarCollapsed(value);
      return;
    }
    setOverride({ tier, value });
  };
  return {
    collapsed,
    tier,
    toggle: () => set(!collapsed),
    expand: () => set(false),
  };
}
