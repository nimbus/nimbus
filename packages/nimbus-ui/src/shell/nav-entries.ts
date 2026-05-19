import type { LucideIcon } from "lucide-react";
import {
  Activity,
  Boxes,
  Building2,
  Clock,
  Cpu,
  Database,
  Gauge,
  HardDrive,
  Network,
  Server,
  Settings,
} from "lucide-react";

import { queryEntry, type QueryEntry } from "nimbus/browser";

import { api } from "../../convex/_generated/api";

// Storage type for heterogeneous nav-count entries. Each construction site
// (via `queryEntry(api.X, args)`) is type-checked against api.X's TArgs;
// TArgs is widened to `any` only at the array level so a single NavEntry
// shape can host counts with different arg shapes.
export type NavCountEntry = QueryEntry<any, readonly unknown[]>;

export type NavView = "developer" | "operator";

export type NavEntry = {
  id: string;
  label: string;
  to: string;
  icon: LucideIcon;
  view: NavView;
  count: NavCountEntry | null;
};

export const DEVELOPER_NAV_ENTRIES: NavEntry[] = [
  {
    id: "overview",
    label: "Overview",
    to: "/app",
    icon: Gauge,
    view: "developer",
    count: null,
  },
  {
    id: "compute",
    label: "Compute",
    to: "/app/compute",
    icon: Cpu,
    view: "developer",
    count: queryEntry(api.functions.list, {
      bundleId: null,
      kind: null,
      limit: 200,
    }),
  },
  {
    id: "services",
    label: "Services",
    to: "/app/services",
    icon: Boxes,
    view: "developer",
    count: queryEntry(api.services.list, {
      tenantId: null,
      machineId: null,
      state: null,
      limit: 200,
    }),
  },
  {
    id: "schedules",
    label: "Schedules",
    to: "/app/schedules",
    icon: Clock,
    view: "developer",
    count: queryEntry(api.scheduled_jobs.list, {
      tenantId: null,
      status: null,
      limit: 200,
    }),
  },
  {
    id: "storage",
    label: "Storage",
    to: "/app/storage",
    icon: Database,
    view: "developer",
    count: queryEntry(api.tables.list, { tenantId: null, limit: 200 }),
  },
  {
    id: "files",
    label: "Files",
    to: "/app/files",
    icon: HardDrive,
    view: "developer",
    count: null,
  },
  {
    id: "observability",
    label: "Observability",
    to: "/app/observability",
    icon: Activity,
    view: "developer",
    count: queryEntry(api.runs.recent, {
      bundleId: null,
      functionPath: null,
      status: null,
      limit: 200,
    }),
  },
  {
    id: "settings",
    label: "Settings",
    to: "/app/settings",
    icon: Settings,
    view: "developer",
    count: null,
  },
];

export const OPERATOR_NAV_ENTRIES: NavEntry[] = [
  {
    id: "system",
    label: "System",
    to: "/admin",
    icon: Gauge,
    view: "operator",
    count: null,
  },
  {
    id: "tenants",
    label: "Tenants",
    to: "/admin/tenants",
    icon: Building2,
    view: "operator",
    count: null,
  },
  {
    id: "machines",
    label: "Machines",
    to: "/admin/machines",
    icon: Server,
    view: "operator",
    count: queryEntry(api.machines.list, {
      state: null,
      provider: null,
      limit: 200,
    }),
  },
  {
    id: "network",
    label: "Network",
    to: "/admin/network",
    icon: Network,
    view: "operator",
    count: queryEntry(api.routes.list, { adapter: null, limit: 200 }),
  },
  {
    id: "services",
    label: "Services",
    to: "/admin/services",
    icon: Boxes,
    view: "operator",
    count: queryEntry(api.services.list, {
      tenantId: null,
      machineId: null,
      state: null,
      limit: 200,
    }),
  },
  {
    id: "observability",
    label: "Observability",
    to: "/admin/observability",
    icon: Activity,
    view: "operator",
    count: queryEntry(api.runs.recent, {
      bundleId: null,
      functionPath: null,
      status: null,
      limit: 200,
    }),
  },
  {
    id: "settings",
    label: "Settings",
    to: "/admin/settings",
    icon: Settings,
    view: "operator",
    count: null,
  },
];

export function navEntriesForView(view: NavView): NavEntry[] {
  return view === "developer" ? DEVELOPER_NAV_ENTRIES : OPERATOR_NAV_ENTRIES;
}

export function viewFromPathname(pathname: string): NavView {
  return pathname.startsWith("/admin") ? "operator" : "developer";
}
