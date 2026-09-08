import { createFileRoute } from "@tanstack/react-router";
import { PageHeader } from "../../components/page-header";
import { PageTabs } from "../../components/page-tabs";
import { LogsTab } from "./observability/-logs";
import { RunsTab } from "./observability/-runs";
import {
  OBSERVABILITY_TABS,
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

export type { ObservabilityTab } from "./observability/-types";

function ObservabilityPage() {
  const search = Route.useSearch();
  const tab: ObservabilityTab = search.tab ?? "logs";
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

function Header({ tab }: { tab: ObservabilityTab }) {
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
      <PageTabs
        label="Observability tabs"
        tabs={OBSERVABILITY_TABS}
        active={tab}
        testid="observability-tabs"
        itemTestid="observability-tab"
      />
    </div>
  );
}
