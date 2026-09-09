import type { LucideIcon } from "lucide-react";
import {
  Activity,
  Box,
  Boxes,
  Building2,
  Clock,
  Cpu,
  Database,
  Gauge,
  HardDrive,
  MonitorCog,
  Network,
  Server,
  Settings,
} from "lucide-react";

export type NavView = "developer" | "operator";

export type NavEntry = {
  id: string;
  label: string;
  to: string;
  icon: LucideIcon;
  view: NavView;
};

// A group is one labelled run of sidebar rows. A `null` label is a run with
// no heading: the view's home page at the top and Settings at the bottom sit
// outside the groups, separated from them by a rule.
export type NavGroup = {
  label: string | null;
  entries: NavEntry[];
};

// The sidebar carries no counts. A badge that reads "0" next to every empty
// section was noise, and a live count next to each row cost one subscription
// per row for a number the page itself shows on arrival.
function entry(
  view: NavView,
  id: string,
  label: string,
  to: string,
  icon: LucideIcon,
): NavEntry {
  return { id, label, to, icon, view };
}

// Developer rows are grouped by what the tenant does with them: Build is
// what the app is made of, Run is what keeps going after a deploy, Observe
// is how it is debugged. Nothing hides behind a group; the label only says
// which hat a page belongs to.
export const DEVELOPER_NAV_GROUPS: ReadonlyArray<NavGroup> = [
  {
    label: null,
    entries: [entry("developer", "overview", "Overview", "/developer", Gauge)],
  },
  {
    label: "Build",
    entries: [
      entry("developer", "compute", "Compute", "/developer/compute", Cpu),
      entry("developer", "storage", "Storage", "/developer/storage", Database),
      entry("developer", "files", "Files", "/developer/files", HardDrive),
    ],
  },
  {
    label: "Run",
    entries: [
      entry("developer", "services", "Services", "/developer/services", Boxes),
      entry("developer", "sandboxes", "Sandboxes", "/developer/sandboxes", Box),
      entry(
        "developer",
        "schedules",
        "Schedules",
        "/developer/schedules",
        Clock,
      ),
    ],
  },
  {
    label: "Observe",
    entries: [
      entry(
        "developer",
        "observability",
        "Observability",
        "/developer/observability",
        Activity,
      ),
    ],
  },
  {
    label: null,
    entries: [
      entry(
        "developer",
        "settings",
        "Settings",
        "/developer/settings",
        Settings,
      ),
    ],
  },
];

// Operator rows are grouped by what the operator runs: Fleet is the hardware
// and the placements on it, Access is who may use the server, Observe is the
// cross-tenant debugging surface.
export const OPERATOR_NAV_GROUPS: ReadonlyArray<NavGroup> = [
  {
    label: null,
    entries: [entry("operator", "nodes", "Nodes", "/operator", Server)],
  },
  {
    label: "Fleet",
    entries: [
      entry(
        "operator",
        "machines",
        "Machines",
        "/operator/machines",
        MonitorCog,
      ),
      entry("operator", "network", "Network", "/operator/network", Network),
      entry("operator", "services", "Services", "/operator/services", Boxes),
    ],
  },
  {
    label: "Access",
    entries: [
      entry("operator", "tenants", "Tenants", "/operator/tenants", Building2),
    ],
  },
  {
    label: "Observe",
    entries: [
      entry(
        "operator",
        "observability",
        "Observability",
        "/operator/observability",
        Activity,
      ),
    ],
  },
  {
    label: null,
    entries: [
      entry("operator", "settings", "Settings", "/operator/settings", Settings),
    ],
  },
];

export const DEVELOPER_NAV_ENTRIES: NavEntry[] = DEVELOPER_NAV_GROUPS.flatMap(
  (group) => group.entries,
);

export const OPERATOR_NAV_ENTRIES: NavEntry[] = OPERATOR_NAV_GROUPS.flatMap(
  (group) => group.entries,
);

export function navGroupsForView(view: NavView): ReadonlyArray<NavGroup> {
  return view === "developer" ? DEVELOPER_NAV_GROUPS : OPERATOR_NAV_GROUPS;
}

export function navEntriesForView(view: NavView): NavEntry[] {
  return view === "developer" ? DEVELOPER_NAV_ENTRIES : OPERATOR_NAV_ENTRIES;
}

export function viewFromPathname(pathname: string): NavView {
  return pathname.startsWith("/operator") ? "operator" : "developer";
}
