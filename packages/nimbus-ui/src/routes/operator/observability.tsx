import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute } from "@tanstack/react-router";

import { api } from "../../../convex/_generated/api";
import { EmptyState } from "../../components/empty-state";
import { PageHeader } from "../../components/page-header";
import { PageTabs } from "../../components/page-tabs";
import { StatePill } from "../../components/pill";
import { ScrollRegion } from "../../components/scroll-region";
import { Td, Th } from "../../components/table-cells";
import { RelativeTime } from "../../components/time";
import { shortId } from "../../lib/format";
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
    // Resolve the default here, not at render, so a bare
    // /operator/observability has the same address as ?tab=logs and the tab
    // strip shows Logs as selected.
    tab: parseTab(search.tab) ?? "logs",
    tenant: typeof search.tenant === "string" ? search.tenant : undefined,
  }),
});

function parseTab(value: unknown): AdminObservabilityTab | undefined {
  return ADMIN_OBSERVABILITY_TABS.find((tab) => tab.id === value)?.id;
}

// The tab strip is the only Logs/Runs switch on this surface. Events and
// Errors join it when their pages exist; until then the console does not
// name them.
export const ADMIN_OBSERVABILITY_TABS = [
  { id: "logs", label: "Logs" },
  { id: "runs", label: "Runs" },
] as const;

export type AdminObservabilityTab =
  (typeof ADMIN_OBSERVABILITY_TABS)[number]["id"];

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
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-admin-observability"
    >
      <div className="flex shrink-0 flex-col gap-3">
        <PageHeader
          title="Operator observability"
          subtitle="Logs and runs across every tenant. Tenant filtering waits on a tenant column in the events table."
          trailing={<ScopeChip scope={scope} />}
          testid="admin-observability-header"
        />
        <PageTabs
          label="Operator observability tabs"
          tabs={ADMIN_OBSERVABILITY_TABS}
          active={tab}
          testid="admin-observability-tabs"
          itemTestid="admin-observability-tab"
        />
      </div>
      {tab === "logs" ? <LogsTab /> : <RunsTab />}
    </section>
  );
}

function ScopeChip({ scope }: { scope: TenantScope }) {
  const requested = serializeTenantScope(scope);
  if (requested === undefined) {
    return (
      <span
        className="shrink-0 whitespace-nowrap rounded-xs border border-border-2 px-2 py-0.5 text-xs font-medium text-text-3"
        data-testid="admin-observability-scope"
        title="Tenant filter unavailable until events table exposes tenant column"
      >
        tenant filter unavailable
      </span>
    );
  }
  return (
    <span
      className="shrink-0 whitespace-nowrap rounded-xs border border-border-2 px-2 py-0.5 text-xs font-medium text-text-3"
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
        className="flex min-h-0 flex-1 items-center justify-center rounded-md border border-border-2 bg-bg-panel font-mono text-xs text-text-3"
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
      className="min-h-0 flex-1 rounded-md border border-border-2 bg-bg-panel"
      data-testid="admin-observability-logs"
    >
      <ul className="divide-y divide-border-2">
        {events.map((event) => (
          <li key={event._id}>
            <article
              className="grid grid-cols-[auto_auto_auto_1fr] items-baseline gap-2 px-3 py-1.5 text-xs hover:bg-bg-raised"
              data-testid={`admin-observability-log-${event._id}`}
            >
              <RelativeTime
                epochMs={event.createdAt ?? event._creationTime ?? 0}
              />
              <StatePill state={event.level ?? "info"} />
              <span className="text-xs font-medium text-text-3">
                {event.source ?? "—"}
                {event.category ? ` · ${event.category}` : ""}
              </span>
              <span className="font-mono text-text-1 truncate">
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
        className="flex min-h-0 flex-1 items-center justify-center rounded-md border border-border-2 bg-bg-panel font-mono text-xs text-text-3"
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
      className="min-h-0 flex-1 overflow-auto rounded-md border border-border-2 bg-bg-panel"
      data-testid="admin-observability-runs"
    >
      <table className="w-full border-collapse text-sm">
        <thead className="sticky top-0 bg-bg-raised text-xs font-medium text-text-3">
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
              className="border-t border-border-2 hover:bg-bg-raised"
              data-testid={`admin-observability-run-${run._id}`}
            >
              <Td>
                <span className="font-mono text-text-1">
                  {run.functionPath ?? shortId(run._id, 12)}
                </span>
              </Td>
              <Td>
                <StatePill state={run.status} />
              </Td>
              <Td>
                <span className="text-xs font-medium text-text-3">
                  {run.kind ?? "—"}
                </span>
              </Td>
              <Td>
                {typeof run.startedAt === "number" ? (
                  <RelativeTime epochMs={run.startedAt} />
                ) : (
                  <span className="tabular text-text-3">—</span>
                )}
              </Td>
              <Td>
                <span className="font-mono text-xs text-text-1">
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
