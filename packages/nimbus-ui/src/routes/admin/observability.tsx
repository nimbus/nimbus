import { createFileRoute, Link } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";

import { api } from "../../../convex/_generated/api";
import { EmptyState } from "../../components/empty-state";
import { StateChip } from "../../components/state-chip";
import { RelativeTime } from "../../components/time";
import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import {
  type StaticSubDrawerSpec,
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

export const Route = createFileRoute("/admin/observability")({
  component: AdminObservabilityPage,
  validateSearch: (search: Record<string, unknown>): AdminObservabilitySearch => ({
    tab: parseTab(search.tab),
    tenant: typeof search.tenant === "string" ? search.tenant : undefined,
  }),
});

function parseTab(value: unknown): AdminObservabilityTab | undefined {
  return value === "logs" || value === "runs" ? value : undefined;
}

export const ADMIN_OBSERVABILITY_SUB_DRAWER = {
  kind: "static",
  title: "Observability",
  items: [
    {
      id: "logs",
      label: "Logs",
      to: "/admin/observability",
      search: { tab: "logs" },
      disabled: false,
    },
    {
      id: "runs",
      label: "Runs",
      to: "/admin/observability",
      search: { tab: "runs" },
      disabled: false,
    },
    {
      id: "events",
      label: "Events",
      to: "/admin/observability",
      search: { tab: "events" },
      disabled: true,
    },
    {
      id: "errors",
      label: "Errors",
      to: "/admin/observability",
      search: { tab: "errors" },
      disabled: true,
    },
  ],
} as const satisfies StaticSubDrawerSpec<
  "logs" | "runs" | "events" | "errors"
>;

export type AdminObservabilityTab =
  (typeof ADMIN_OBSERVABILITY_SUB_DRAWER.items)[number]["id"];

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
  useContributeSubDrawer(ADMIN_OBSERVABILITY_SUB_DRAWER);
  const search = Route.useSearch();
  const tab: AdminObservabilityTab = search.tab ?? "logs";
  const scope = parseTenantScope(search.tenant);
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-admin-observability"
    >
      <Header tab={tab} scope={scope} />
      {tab === "logs" ? <LogsTab /> : <RunsTab />}
    </section>
  );
}

function Header({
  tab,
  scope,
}: {
  tab: AdminObservabilityTab;
  scope: TenantScope;
}) {
  return (
    <header className="flex flex-col gap-3">
      <div className="flex items-baseline justify-between">
        <div>
          <h1 className="text-default" style={{ fontSize: "var(--text-xl)" }}>
            Operator observability
          </h1>
          <p className="text-sm text-muted">
            Server-wide logs and runs across every tenant. Tenant filtering is
            gated until the events table exposes a tenant column.
          </p>
        </div>
        <ScopeChip scope={scope} />
      </div>
      <nav
        aria-label="Operator observability tabs"
        className="flex gap-px overflow-hidden rounded-md border border-app bg-surface-2 self-start"
        data-testid="admin-observability-tabs"
      >
        {ADMIN_OBSERVABILITY_SUB_DRAWER.items.map((item) => (
          <TabLink
            key={item.id}
            id={item.id}
            label={item.label}
            active={tab === item.id}
            disabled={Boolean(item.disabled)}
            scope={scope}
          />
        ))}
      </nav>
    </header>
  );
}

function ScopeChip({ scope }: { scope: TenantScope }) {
  const requested = serializeTenantScope(scope);
  if (requested === undefined) {
    return (
      <span
        className="rounded border border-app px-2 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted"
        data-testid="admin-observability-scope"
        title="Tenant filter unavailable until events table exposes tenant column"
      >
        tenant filter unavailable
      </span>
    );
  }
  return (
    <span
      className="rounded border border-app px-2 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted"
      data-testid="admin-observability-scope"
      title="Tenant filter requested but not honored — events table does not expose tenant column yet"
    >
      tenant {requested} · filter unavailable
    </span>
  );
}

function TabLink({
  id,
  label,
  active,
  disabled,
  scope,
}: {
  id: AdminObservabilityTab;
  label: string;
  active: boolean;
  disabled: boolean;
  scope: TenantScope;
}) {
  if (disabled) {
    return (
      <span
        aria-disabled="true"
        data-testid={`admin-observability-tab-${id}`}
        title={`${label} — coming soon`}
        className={cn(
          "inline-flex items-center gap-1.5 px-3 py-1.5 font-mono text-xs uppercase tracking-wide",
          "cursor-not-allowed text-muted opacity-60",
        )}
      >
        {label}
        <span
          aria-hidden
          className="rounded bg-surface-2 px-1 text-[9px] uppercase tracking-wide text-muted"
          data-testid={`admin-observability-tab-${id}-coming-soon`}
        >
          coming soon
        </span>
      </span>
    );
  }
  const tenantQuery = serializeTenantScope(scope);
  return (
    <Link
      to="/admin/observability"
      search={(prev) => ({ ...prev, tab: id, tenant: tenantQuery })}
      data-testid={`admin-observability-tab-${id}`}
      aria-current={active ? "page" : undefined}
      className={cn(
        "px-3 py-1.5 font-mono text-xs uppercase tracking-wide",
        active
          ? "bg-surface text-default"
          : "text-muted hover:bg-surface hover:text-default",
      )}
    >
      {label}
    </Link>
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
    <div
      className="min-h-0 flex-1 overflow-auto rounded-md border border-app bg-surface"
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
              <span className="font-mono text-[10px] uppercase tracking-wide text-muted">
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
    </div>
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
        <thead className="sticky top-0 bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
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

function Th({ children }: { children: React.ReactNode }) {
  return <th className="px-3 py-2 text-left font-semibold">{children}</th>;
}

function Td({ children }: { children: React.ReactNode }) {
  return <td className="px-3 py-2 text-default">{children}</td>;
}
