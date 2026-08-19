import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { Play, Radio, ScrollText, TriangleAlert } from "lucide-react";
import { useMemo } from "react";

import { api } from "../../../convex/_generated/api";
import { Td, Th } from "../../components/data-table";
import { EmptyState } from "../../components/empty-state";
import { PageHeader } from "../../components/page-header";
import { ScrollRegion } from "../../components/scroll-region";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { shortId } from "../../lib/format";
import {
  type StaticSubDrawerSpec,
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";
import {
  parseTenantScope,
  serializeTenantScope,
  type TenantScope,
} from "../../shell/tenant-scope";

type AdminObservabilitySearch = {
  tab?: AdminObservabilityTab;
  tenant?: string;
};

export const Route = createFileRoute("/operator/observability")({
  component: AdminObservabilityPage,
  validateSearch: (
    search: Record<string, unknown>,
  ): AdminObservabilitySearch => ({
    // Resolve the default here, not at render: the sub-drawer is now the only
    // sub-view switch, and it marks an item active by matching its `search`
    // against the location's. A bare /operator/observability would otherwise
    // show Logs without showing Logs as selected.
    tab: parseTab(search.tab) ?? "logs",
    tenant: typeof search.tenant === "string" ? search.tenant : undefined,
  }),
});

function parseTab(value: unknown): AdminObservabilityTab | undefined {
  return value === "logs" || value === "runs" ? value : undefined;
}

// The sub-drawer is the only Logs/Runs/Events/Errors switch on this surface.
// It renders the "coming soon" chip for any item marked `disabled`, so the
// state explains itself without a second, duplicate tab strip -- and without
// each caller spelling the marker into its own label.
const ADMIN_OBSERVABILITY_ITEMS = [
  {
    id: "logs",
    label: "Logs",
    to: "/operator/observability",
    search: { tab: "logs" },
    disabled: false,
    icon: ScrollText,
  },
  {
    id: "runs",
    label: "Runs",
    to: "/operator/observability",
    search: { tab: "runs" },
    disabled: false,
    icon: Play,
  },
  {
    id: "events",
    label: "Events",
    to: "/operator/observability",
    search: { tab: "events" },
    disabled: true,
    icon: Radio,
  },
  {
    id: "errors",
    label: "Errors",
    to: "/operator/observability",
    search: { tab: "errors" },
    disabled: true,
    icon: TriangleAlert,
  },
] as const;

export const ADMIN_OBSERVABILITY_SUB_DRAWER = {
  kind: "static",
  title: "Observability",
  items: ADMIN_OBSERVABILITY_ITEMS,
} as const satisfies StaticSubDrawerSpec<"logs" | "runs" | "events" | "errors">;

export type AdminObservabilityTab =
  (typeof ADMIN_OBSERVABILITY_ITEMS)[number]["id"];

type EventDoc = {
  _id: string;
  _creationTime?: number;
  source?: string;
  level?: string;
  category?: string;
  message?: string;
  createdAt?: number;
  correlationId?: string | null;
};

type RunDoc = {
  _id: string;
  _creationTime?: number;
  functionPath?: string;
  kind?: string;
  status?: string;
  startedAt?: number;
};

function AdminObservabilityPage() {
  const search = Route.useSearch();
  const tab: AdminObservabilityTab = search.tab ?? "logs";
  const scope = parseTenantScope(search.tenant);
  const navigate = useNavigate({ from: "/operator/observability" });
  // Collapsing the sub-drawer must not strand the operator without a switch,
  // so the enabled sub-views are also reachable from the icon rail.
  const spec = useMemo<SubDrawerSpec>(
    () => ({
      ...ADMIN_OBSERVABILITY_SUB_DRAWER,
      railItems: ADMIN_OBSERVABILITY_ITEMS.filter((item) => !item.disabled).map(
        (item) => ({
          id: item.id,
          label: item.label,
          icon: item.icon,
          active: tab === item.id,
          onSelect: () => {
            void navigate({
              to: "/operator/observability",
              search: (prev) => ({ ...prev, tab: item.id }),
            });
          },
        }),
      ),
    }),
    [tab, navigate],
  );
  useContributeSubDrawer(spec);
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-admin-observability"
    >
      <PageHeader
        title="Operator observability"
        subtitle="Server-wide logs and runs across every tenant. Tenant filtering is gated until the events table exposes a tenant column."
        trailing={<ScopeChip scope={scope} />}
        testid="admin-observability-header"
      />
      {tab === "logs" ? <LogsTab /> : <RunsTab />}
    </section>
  );
}

function ScopeChip({ scope }: { scope: TenantScope }) {
  const requested = serializeTenantScope(scope);
  if (requested === undefined) {
    return (
      <span
        className="shrink-0 whitespace-nowrap rounded border border-app px-2 py-0.5 font-mono text-xs uppercase tracking-wide text-muted"
        data-testid="admin-observability-scope"
        title="Tenant filter unavailable until events table exposes tenant column"
      >
        tenant filter unavailable
      </span>
    );
  }
  return (
    <span
      className="shrink-0 whitespace-nowrap rounded border border-app px-2 py-0.5 font-mono text-xs uppercase tracking-wide text-muted"
      data-testid="admin-observability-scope"
      title="Tenant filter requested but not honored — events table does not expose tenant column yet"
    >
      tenant {requested} · filter unavailable
    </span>
  );
}

function LogsTab() {
  const events = useQuery(api.events.recent, {
    source: null,
    level: null,
    category: null,
    correlationId: null,
    limit: 200,
  }) as EventDoc[] | undefined;
  return <LogList events={events} />;
}

function LogList({ events }: { events: EventDoc[] | undefined }) {
  if (events === undefined) {
    return (
      <div
        className="flex min-h-0 flex-1 items-center justify-center rounded-md border border-app bg-surface font-mono text-xs text-muted"
        data-testid="admin-observability-logs-loading"
      >
        Loading events…
      </div>
    );
  }
  if (events.length === 0) {
    return (
      <EmptyState
        title="No events yet"
        body="The server has not emitted any events on the active scope. Logs will appear here as functions run and adapters serve traffic."
        testid="admin-observability-empty"
      />
    );
  }
  return (
    <ScrollRegion
      label="Event log"
      className="min-h-0 flex-1 rounded-md border border-app bg-surface"
      data-testid="admin-observability-logs"
    >
      <ul className="divide-y divide-app">
        {events.map((event) => (
          <li key={event._id}>
            <article
              className="grid grid-cols-[auto_auto_auto_1fr] items-baseline gap-2 px-3 py-1.5 text-xs hover:bg-surface-2"
              data-testid={`admin-observability-log-${event._id}`}
            >
              <RelativeTime
                epochMs={event.createdAt ?? event._creationTime ?? 0}
              />
              <StateChip state={event.level ?? "info"} />
              <span className="font-mono text-xs uppercase tracking-wide text-muted">
                {event.source ?? "—"}
                {event.category ? ` · ${event.category}` : ""}
              </span>
              <span className="font-mono text-default truncate">
                {event.message ?? "(no message)"}
              </span>
            </article>
          </li>
        ))}
      </ul>
    </ScrollRegion>
  );
}

function RunsTab() {
  const runs = useQuery(api.runs.recent, {
    bundleId: null,
    functionPath: null,
    status: null,
    limit: 200,
  }) as RunDoc[] | undefined;
  if (runs === undefined) {
    return (
      <div
        className="flex min-h-0 flex-1 items-center justify-center rounded-md border border-app bg-surface font-mono text-xs text-muted"
        data-testid="admin-observability-runs-loading"
      >
        Loading runs…
      </div>
    );
  }
  if (runs.length === 0) {
    return (
      <EmptyState
        title="No runs yet"
        body="Server-wide function and adapter runs will appear here once any tenant has executed a request."
        testid="admin-observability-runs-empty"
      />
    );
  }
  return (
    <div
      className="min-h-0 flex-1 overflow-auto rounded-md border border-app bg-surface"
      data-testid="admin-observability-runs"
    >
      <table className="w-full border-collapse text-sm">
        <thead className="sticky top-0 bg-surface-2 text-xs uppercase tracking-[0.14em] text-muted">
          <tr>
            <Th>Function</Th>
            <Th>Status</Th>
            <Th>Kind</Th>
            <Th>Started</Th>
            <Th>Run id</Th>
          </tr>
        </thead>
        <tbody>
          {runs.map((run) => (
            <tr
              key={run._id}
              className="border-t border-app hover:bg-surface-2"
              data-testid={`admin-observability-run-${run._id}`}
            >
              <Td>
                <span className="font-mono text-default">
                  {run.functionPath ?? shortId(run._id, 12)}
                </span>
              </Td>
              <Td>
                <StateChip state={run.status} />
              </Td>
              <Td>
                <span className="font-mono text-xs uppercase tracking-wide text-muted">
                  {run.kind ?? "—"}
                </span>
              </Td>
              <Td>
                {typeof run.startedAt === "number" ? (
                  <RelativeTime epochMs={run.startedAt} />
                ) : (
                  <span className="tabular text-muted">—</span>
                )}
              </Td>
              <Td>
                <span className="font-mono text-xs text-default">
                  {shortId(run._id, 10)}
                </span>
              </Td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
