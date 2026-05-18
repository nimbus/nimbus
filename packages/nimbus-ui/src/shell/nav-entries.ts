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

import { api } from "../../convex/_generated/api";
import type { JsonValue, QueryReference } from "./types";

type CountQuery = QueryReference<unknown, JsonValue[] | null | undefined>;

export type NavView = "developer" | "operator";

export type NavEntry = {
  id: string;
  label: string;
  to: string;
  icon: LucideIcon;
  view: NavView;
  countQuery: CountQuery | null;
  countArgs: Record<string, unknown> | null;
};

export const DEVELOPER_NAV_ENTRIES: NavEntry[] = [
  {
    id: "overview",
    label: "Overview",
    to: "/app",
    icon: Gauge,
    view: "developer",
    countQuery: null,
    countArgs: null,
  },
  {
    id: "compute",
    label: "Compute",
    to: "/app/compute",
    icon: Cpu,
    view: "developer",
    countQuery: api.functions.list as unknown as CountQuery,
    countArgs: { bundleId: null, kind: null, limit: 200 },
  },
  {
    id: "services",
    label: "Services",
    to: "/app/services",
    icon: Boxes,
    view: "developer",
    countQuery: api.services.list as unknown as CountQuery,
    countArgs: { tenantId: null, machineId: null, state: null, limit: 200 },
  },
  {
    id: "schedules",
    label: "Schedules",
    to: "/app/schedules",
    icon: Clock,
    view: "developer",
    countQuery: api.scheduled_jobs.list as unknown as CountQuery,
    countArgs: { tenantId: null, status: null, limit: 200 },
  },
  {
    id: "storage",
    label: "Storage",
    to: "/app/storage",
    icon: Database,
    view: "developer",
    countQuery: api.tables.list as unknown as CountQuery,
    countArgs: { tenantId: null, limit: 200 },
  },
  {
    id: "files",
    label: "Files",
    to: "/app/files",
    icon: HardDrive,
    view: "developer",
    countQuery: null,
    countArgs: null,
  },
  {
    id: "observability",
    label: "Observability",
    to: "/app/observability",
    icon: Activity,
    view: "developer",
    countQuery: api.runs.recent as unknown as CountQuery,
    countArgs: { bundleId: null, functionPath: null, status: null, limit: 200 },
  },
  {
    id: "settings",
    label: "Settings",
    to: "/app/settings",
    icon: Settings,
    view: "developer",
    countQuery: null,
    countArgs: null,
  },
];

export const OPERATOR_NAV_ENTRIES: NavEntry[] = [
  {
    id: "system",
    label: "System",
    to: "/admin",
    icon: Gauge,
    view: "operator",
    countQuery: null,
    countArgs: null,
  },
  {
    id: "tenants",
    label: "Tenants",
    to: "/admin/tenants",
    icon: Building2,
    view: "operator",
    countQuery: null,
    countArgs: null,
  },
  {
    id: "machines",
    label: "Machines",
    to: "/admin/machines",
    icon: Server,
    view: "operator",
    countQuery: api.machines.list as unknown as CountQuery,
    countArgs: { state: null, provider: null, limit: 200 },
  },
  {
    id: "network",
    label: "Network",
    to: "/admin/network",
    icon: Network,
    view: "operator",
    countQuery: api.routes.list as unknown as CountQuery,
    countArgs: { adapter: null, limit: 200 },
  },
  {
    id: "services",
    label: "Services",
    to: "/admin/services",
    icon: Boxes,
    view: "operator",
    countQuery: api.services.list as unknown as CountQuery,
    countArgs: { tenantId: null, machineId: null, state: null, limit: 200 },
  },
  {
    id: "observability",
    label: "Observability",
    to: "/admin/observability",
    icon: Activity,
    view: "operator",
    countQuery: api.runs.recent as unknown as CountQuery,
    countArgs: { bundleId: null, functionPath: null, status: null, limit: 200 },
  },
  {
    id: "settings",
    label: "Settings",
    to: "/admin/settings",
    icon: Settings,
    view: "operator",
    countQuery: null,
    countArgs: null,
  },
];

export function navEntriesForView(view: NavView): NavEntry[] {
  return view === "developer" ? DEVELOPER_NAV_ENTRIES : OPERATOR_NAV_ENTRIES;
}

export function viewFromPathname(pathname: string): NavView {
  return pathname.startsWith("/admin") ? "operator" : "developer";
}
