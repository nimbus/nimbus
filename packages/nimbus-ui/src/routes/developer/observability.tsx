import { createFileRoute, Link } from "@tanstack/react-router";
import { cn } from "@/lib/utils";
import { PageHeader } from "../../components/page-header";
import { CategoryPill } from "../../components/pill";
import {
  type StaticSubPanelSpec,
  useContributeSubPanel,
} from "../../shell/sub-panel";
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

export const OBSERVABILITY_SUB_PANEL = {
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
} as const satisfies StaticSubPanelSpec<ObservabilityTab>;

export type { ObservabilityTab } from "./observability/-types";

function ObservabilityPage() {
  useContributeSubPanel(OBSERVABILITY_SUB_PANEL);
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
    // `shrink-0` on both this column and the tab strip: the page column is
    // `overflow-hidden`, so anything the flexbox compresses here is clipped
    // with no way to scroll it back. Today the tab row survives only because
    // the sibling pane is `min-h-0 flex-1` and absorbs the shrink — an
    // invariant owned by another file. Pin it locally instead.
    <div className="flex shrink-0 flex-col gap-3">
      {/* The tab strip sits below the title rather than in `trailing`, so the
          title molecule is the shared one and the nav is its sibling. */}
      <PageHeader
        title="Observability"
        subtitle={
          <>
            Live event stream and recent runs. Reads stream from the{" "}
            <code className="font-mono text-text-1">_nimbus</code> system
            tenant.
          </>
        }
        testid="observability-header"
      />
      <nav
        aria-label="Observability tabs"
        className="flex shrink-0 gap-px self-start overflow-hidden rounded-md border border-border-2 bg-bg-raised"
        data-testid="observability-tabs"
      >
        {OBSERVABILITY_SUB_PANEL.items.map((item) =>
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
    </div>
  );
}

function DisabledTab({ id, label }: { id: ObservabilityTab; label: string }) {
  return (
    <span
      aria-disabled="true"
      data-testid={`observability-tab-${id}`}
      title={`${label} — coming soon`}
      className={cn(
        "inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium",
        "cursor-not-allowed text-text-3",
      )}
    >
      {label}
      {/* The neutral pill carries the disabled state at the smallest
          sanctioned step; a muted wrapper alone did not read as disabled. */}
      <CategoryPill
        aria-hidden
        value="coming soon"
        data-testid={`observability-tab-${id}-coming-soon`}
      />
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
        "px-3 py-1.5 text-xs font-medium",
        active
          ? "bg-bg-panel text-text-1"
          : "text-text-3 hover:bg-bg-panel hover:text-text-1",
      )}
    >
      {label}
    </Link>
  );
}
