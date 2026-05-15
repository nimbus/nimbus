import type { LucideIcon } from "lucide-react";
import {
  Activity,
  Cpu,
  Database,
  Gauge,
  Network,
  Server,
  Settings,
} from "lucide-react";

import { api } from "../../convex/_generated/api";
import type { JsonValue, QueryReference } from "./types";

type CountQuery = QueryReference<unknown, JsonValue[] | null | undefined>;

export type NavEntry = {
  id: string;
  label: string;
  to: string;
  icon: LucideIcon;
  countQuery: CountQuery | null;
  countArgs: Record<string, unknown> | null;
};

export const NAV_ENTRIES: NavEntry[] = [
  {
    id: "overview",
    label: "Overview",
    to: "/",
    icon: Gauge,
    countQuery: null,
    countArgs: null,
  },
  {
    id: "compute",
    label: "Compute",
    to: "/compute",
    icon: Cpu,
    countQuery: api.functions.list as unknown as CountQuery,
    countArgs: { bundleId: null, kind: null, limit: 200 },
  },
  {
    id: "storage",
    label: "Storage",
    to: "/storage",
    icon: Database,
    countQuery: api.tables.list as unknown as CountQuery,
    countArgs: { tenantId: null, limit: 200 },
  },
  {
    id: "network",
    label: "Network",
    to: "/network",
    icon: Network,
    countQuery: api.routes.list as unknown as CountQuery,
    countArgs: { adapter: null, limit: 200 },
  },
  {
    id: "machines",
    label: "Machines",
    to: "/machines",
    icon: Server,
    countQuery: api.machines.list as unknown as CountQuery,
    countArgs: { state: null, provider: null, limit: 200 },
  },
  {
    id: "observability",
    label: "Observability",
    to: "/observability",
    icon: Activity,
    countQuery: api.runs.recent as unknown as CountQuery,
    countArgs: { bundleId: null, functionPath: null, status: null, limit: 200 },
  },
  {
    id: "settings",
    label: "Settings",
    to: "/settings",
    icon: Settings,
    countQuery: null,
    countArgs: null,
  },
];
