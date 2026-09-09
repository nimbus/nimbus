import { Radio } from "@base-ui/react/radio";
import { RadioGroup } from "@base-ui/react/radio-group";
import { useNimbus } from "@nimbus/nimbus/react";
import { Building2, Code2, Server, ServerCog } from "lucide-react";

import { CopyChip } from "@/components/copy-chip";
import { cn } from "@/lib/utils";
import { useUiStore } from "../../store/ui-store";
import type { NavView } from "../nav-entries";
import { TenantSelector } from "../tenant-selector";
import { useViewSwitch, VIEW_SEGMENTS, ViewSwitcher } from "../view-switcher";
import { RailTooltip, rowClass } from "./rail";

const VIEW_ICONS = {
  developer: Code2,
  operator: ServerCog,
} as const;

// ScopeRow is the line under the brand that says what the rows below are
// about: which view, and within the developer view, which tenant. The
// operator view has no tenant; its scope is the server, so the row names
// the server instead.
export function ScopeRow({
  view,
  collapsed,
  inSheet,
  onExpand,
}: {
  view: NavView;
  collapsed: boolean;
  inSheet: boolean;
  onExpand: () => void;
}) {
  if (collapsed) {
    return (
      <div
        data-testid="sidebar-scope"
        className="flex flex-col items-center gap-1 px-3 pb-2"
      >
        <RailViewSwitcher />
        <RailScopeButton view={view} onExpand={onExpand} />
      </div>
    );
  }
  return (
    <div
      data-testid="sidebar-scope"
      className={cn("flex flex-col gap-2 px-4 pb-3", inSheet && "gap-3")}
    >
      <ViewSwitcher />
      {view === "developer" ? (
        <TenantSelector mode={{ kind: "developer" }} />
      ) : (
        <ServerIdentity />
      )}
    </div>
  );
}

// The rail's view switcher is the same radio group as the expanded one with
// the labels moved into tooltips, so arrow keys and aria-checked behave the
// same in both widths.
function RailViewSwitcher() {
  const { activeView, switchTo } = useViewSwitch();
  return (
    <RadioGroup
      aria-label="Console view"
      value={activeView}
      onValueChange={(next) => switchTo(next as NavView)}
      data-testid="view-switcher"
      className="flex flex-col items-center gap-0.5 rounded-md border border-border-2 bg-bg-panel p-0.5"
    >
      {VIEW_SEGMENTS.map((segment) => {
        const Icon = VIEW_ICONS[segment.value];
        return (
          <RailTooltip key={segment.value} label={segment.label}>
            <Radio.Root
              value={segment.value}
              aria-label={segment.label}
              data-testid={`view-switcher-${segment.value}`}
              className="inline-flex size-8 items-center justify-center rounded-sm text-text-3 outline-none transition-colors duration-150 ease-standard hover:text-text-1 data-checked:bg-bg-raised data-checked:text-text-1"
            >
              <Icon size={16} aria-hidden />
            </Radio.Root>
          </RailTooltip>
        );
      })}
    </RadioGroup>
  );
}

// In the rail the tenant and the server become one button that opens the
// sidebar: a 64px column cannot hold a listbox, and expanding is a smaller
// surprise than a popover that pushes the page.
function RailScopeButton({
  view,
  onExpand,
}: {
  view: NavView;
  onExpand: () => void;
}) {
  const activeTenant = useUiStore((s) => s.activeTenant);
  const host = useServerHost();
  const label =
    view === "developer"
      ? `Tenant · ${activeTenant ?? "none selected"}`
      : `Server · ${host}`;
  const Icon = view === "developer" ? Building2 : Server;
  return (
    <RailTooltip label={label}>
      <button
        type="button"
        onClick={onExpand}
        aria-label={`${label}. Expand sidebar to change`}
        data-testid="sidebar-scope-button"
        className={rowClass({ collapsed: true, className: "h-8" })}
      >
        <Icon size={16} aria-hidden />
      </button>
    </RailTooltip>
  );
}

function ServerIdentity() {
  const host = useServerHost();
  const url = useServerUrl();
  return (
    <div
      className="flex h-7 items-center gap-2 rounded-md border border-border-2 bg-bg-panel px-2 text-xs"
      data-testid="sidebar-server"
    >
      <span className="font-medium text-text-3">Server</span>
      <CopyChip
        label="server URL"
        value={url}
        testid="sidebar-server-url"
        className="min-w-0 flex-1 truncate text-left font-mono text-text-1"
      >
        {host}
      </CopyChip>
    </div>
  );
}

function useServerUrl(): string {
  const client = useNimbus();
  if (client.url) return client.url;
  if (typeof window === "undefined") return "—";
  return window.location.origin;
}

// The hostname alone is what identifies a server at a glance; the scheme
// and port are in the copied value and in the title.
function useServerHost(): string {
  const url = useServerUrl();
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
