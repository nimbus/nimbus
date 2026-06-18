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
  MonitorCog,
  Network,
  Server,
  Settings,
} from "lucide-react";

import { queryEntry, type QueryEntry } from "@nimbus/nimbus/browser";

import { api } from "../../convex/_generated/api";

// Storage type for heterogeneous nav-count entries. Each construction site
// (via `queryEntry(api.X, args)`) is type-checked against api.X's TArgs;
// TArgs is widened to `any` only at the array level so a single NavEntry
// shape can host counts with different arg shapes.
export type NavCountEntry = QueryEntry<any, readonly unknown[]>;

// Non-query nav-count sources: tenants come from the HTTP tenant list and
// "nodes" is the single local host — neither is a convex array query.
export type NavCountKind = "tenants" | "nodes";

export type NavView = "developer" | "operator";

export type NavEntry = {
  id: string;
  label: string;
  to: string;
  icon: LucideIcon;
  view: NavView;
  count: NavCountEntry | null;
  countKind?: NavCountKind;
};

export const DEVELOPER_NAV_ENTRIES: NavEntry[] = [
  {
    id: "overview",
    label: "Overview",
    to: "/developer",
    icon: Gauge,
    view: "developer",
    count: null,
  },
  {
    id: "compute",
    label: "Compute",
    to: "/developer/compute",
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
    to: "/developer/services",
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
    to: "/developer/schedules",
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
    to: "/developer/storage",
    icon: Database,
    view: "developer",
    count: queryEntry(api.tables.list, { tenantId: null, limit: 200 }),
  },
  {
    id: "files",
    label: "Files",
    to: "/developer/files",
    icon: HardDrive,
    view: "developer",
    count: null,
  },
  {
    id: "observability",
    label: "Observability",
    to: "/developer/observability",
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
    to: "/developer/settings",
    icon: Settings,
    view: "developer",
    count: null,
  },
];

export const OPERATOR_NAV_ENTRIES: NavEntry[] = [
  {
    id: "nodes",
    label: "Nodes",
    to: "/operator",
    icon: Server,
    view: "operator",
    count: null,
    countKind: "nodes",
  },
  {
    id: "tenants",
    label: "Tenants",
    to: "/operator/tenants",
    icon: Building2,
    view: "operator",
    count: null,
    countKind: "tenants",
  },
  {
    id: "machines",
    label: "Machines",
    to: "/operator/machines",
    icon: MonitorCog,
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
    to: "/operator/network",
    icon: Network,
    view: "operator",
    count: queryEntry(api.routes.list, { adapter: null, limit: 200 }),
  },
  {
    id: "services",
    label: "Services",
    to: "/operator/services",
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
    to: "/operator/observability",
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
    to: "/operator/settings",
    icon: Settings,
    view: "operator",
    count: null,
  },
];

export function navEntriesForView(view: NavView): NavEntry[] {
  return view === "developer" ? DEVELOPER_NAV_ENTRIES : OPERATOR_NAV_ENTRIES;
}

export function viewFromPathname(pathname: string): NavView {
  return pathname.startsWith("/operator") ? "operator" : "developer";
}
