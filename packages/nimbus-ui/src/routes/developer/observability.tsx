import { createFileRoute, Link } from "@tanstack/react-router";

import { cn } from "../../lib/cn";
import {
  type StaticSubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";
import { LogsTab } from "./observability/-logs";
import { RunsTab } from "./observability/-runs";
import {
  ACTIVE_OBSERVABILITY_TABS,
  type ActiveObservabilityTab,
  DISABLED_OBSERVABILITY_TABS,
  type ObservabilitySearch,
  type ObservabilityTab,
  parseBool,
  parseString,
  parseTab,
} from "./observability/-types";

export const Route = createFileRoute("/developer/observability")({
  validateSearch: (search: Record<string, unknown>): ObservabilitySearch => ({
    tab: parseTab(search.tab),
    level: parseString(search.level),
    category: parseString(search.category),
    source: parseString(search.source),
    correlationId: parseString(search.correlationId),
    status: parseString(search.status),
    functionPath: parseString(search.functionPath),
    follow: parseBool(search.follow),
    pauseOnError: parseBool(search.pauseOnError),
  }),
  component: ObservabilityPage,
});

const TAB_LABELS: Record<ObservabilityTab, string> = {
  logs: "Logs",
  runs: "Runs",
  events: "Events",
  errors: "Errors",
};

export const OBSERVABILITY_SUB_DRAWER = {
  kind: "static",
  title: "Observability",
  items: [
    ...ACTIVE_OBSERVABILITY_TABS.map((id) => ({
      id,
      label: TAB_LABELS[id],
      to: "/developer/observability" as const,
      search: { tab: id },
      disabled: false as const,
    })),
    ...DISABLED_OBSERVABILITY_TABS.map((id) => ({
      id,
      label: TAB_LABELS[id],
      to: "/developer/observability" as const,
      search: { tab: id },
      disabled: true as const,
    })),
  ],
} as const satisfies StaticSubDrawerSpec<ObservabilityTab>;

export type { ObservabilityTab } from "./observability/-types";

function ObservabilityPage() {
  useContributeSubDrawer(OBSERVABILITY_SUB_DRAWER);
  const search = Route.useSearch();
  const tab: ActiveObservabilityTab = search.tab ?? "logs";
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-observability"
    >
      <Header tab={tab} />
      {tab === "logs" ? (
        <LogsTab search={search} />
      ) : (
        <RunsTab search={search} />
      )}
    </section>
  );
}

function Header({ tab }: { tab: ActiveObservabilityTab }) {
  return (
    <header className="flex flex-col gap-3">
      <div>
        <h1 className="text-default" style={{ fontSize: "var(--text-xl)" }}>
          Observability
        </h1>
        <p className="text-sm text-muted">
          Live event stream and recent runs. Reads stream from the{" "}
          <code className="font-mono text-default">_nimbus</code> system tenant.
        </p>
      </div>
      <nav
        aria-label="Observability tabs"
        className="flex gap-px overflow-hidden rounded-md border border-app bg-surface-2 self-start"
        data-testid="observability-tabs"
      >
        {OBSERVABILITY_SUB_DRAWER.items.map((item) =>
          item.disabled ? (
            <DisabledTab key={item.id} id={item.id} label={item.label} />
          ) : (
            <ActiveTabLink
              key={item.id}
              id={item.id}
              label={item.label}
              active={tab === item.id}
            />
          ),
        )}
      </nav>
    </header>
  );
}

function DisabledTab({
  id,
  label,
}: {
  id: ObservabilityTab;
  label: string;
}) {
  return (
    <span
      aria-disabled="true"
      data-testid={`observability-tab-${id}`}
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
        data-testid={`observability-tab-${id}-coming-soon`}
      >
        coming soon
      </span>
    </span>
  );
}

function ActiveTabLink({
  id,
  label,
  active,
}: {
  id: ActiveObservabilityTab;
  label: string;
  active: boolean;
}) {
  return (
    <Link
      to="/developer/observability"
      search={(prev) => ({ ...prev, tab: id })}
      data-testid={`observability-tab-${id}`}
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
