import { createFileRoute } from "@tanstack/react-router";

import { PageHeader } from "../../components/page-header";
import { PageTabs } from "../../components/page-tabs";
import { useUiStore } from "../../store/ui-store";
import { ErrorsTab } from "./observability/-errors";
import type { ObservabilityTabProps } from "./observability/-facets";
import { LogsTab } from "./observability/-logs";
import { useObservabilityNavigation } from "./observability/-navigation";
import { RunsTab } from "./observability/-runs";
import { TracesTab } from "./observability/-traces";
import {
  OBSERVABILITY_TABS,
  type ObservabilityTab,
  parseObservabilitySearch,
} from "./observability/-types";

export const Route = createFileRoute("/developer/observability")({
  validateSearch: parseObservabilitySearch,
  component: ObservabilityPage,
});

export type { ObservabilityTab } from "./observability/-types";

function ObservabilityPage() {
  const search = Route.useSearch();
  const tab: ObservabilityTab = search.tab ?? "logs";
  const activeTenant = useUiStore((s) => s.activeTenant);
  // The developer surface reads one tenant: the one in the address, else
  // the active one. It never reads across tenants; that is the operator
  // page and the `_nimbus` lens.
  const tenantId = search.tenant ?? activeTenant ?? null;
  const { setSearch, setSearchAction } = useObservabilityNavigation(
    "/developer/observability",
  );
  const tabProps = {
    search,
    tenantId,
    allowAllTenants: false,
    setSearch,
    setSearchAction,
  };
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-observability"
    >
      <Header tab={tab} />
      <ObservabilityTabBody tab={tab} {...tabProps} />
    </section>
  );
}

// One switch for both surfaces: the operator page renders the same tabs
// with a different tenant scope.
export function ObservabilityTabBody({
  tab,
  ...tabProps
}: { tab: ObservabilityTab } & ObservabilityTabProps) {
  switch (tab) {
    case "runs":
      return <RunsTab {...tabProps} />;
    case "traces":
      return <TracesTab {...tabProps} />;
    case "errors":
      return <ErrorsTab {...tabProps} />;
    default:
      return <LogsTab {...tabProps} />;
  }
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
            Logs, runs, traces, and error groups for the active tenant, read
            from the <code className="font-mono text-text-1">_nimbus</code>{" "}
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
