import { createFileRoute } from "@tanstack/react-router";

import { PageHeader } from "../../components/page-header";
import { PageTabs } from "../../components/page-tabs";
import { LogsTab } from "../developer/observability/-logs";
import { useObservabilityNavigation } from "../developer/observability/-navigation";
import { RunsTab } from "../developer/observability/-runs";
import {
  OBSERVABILITY_TABS,
  type ObservabilitySearch,
  parseObservabilitySearch,
} from "../developer/observability/-types";

export const Route = createFileRoute("/operator/observability")({
  component: AdminObservabilityPage,
  validateSearch: (search: Record<string, unknown>): ObservabilitySearch => {
    const parsed = parseObservabilitySearch(search);
    // Resolve the default here, not at render, so a bare
    // /operator/observability has the same address as ?tab=logs and the tab
    // strip shows Logs as selected.
    return { ...parsed, tab: parsed.tab ?? "logs" };
  },
});

// The tab strip is the only Logs/Runs switch on this surface. Events and
// Errors join it when their pages exist; until then the console does not
// name them.
export const ADMIN_OBSERVABILITY_TABS = OBSERVABILITY_TABS;

export type AdminObservabilityTab =
  (typeof ADMIN_OBSERVABILITY_TABS)[number]["id"];

function AdminObservabilityPage() {
  const search = Route.useSearch();
  const tab: AdminObservabilityTab = search.tab ?? "logs";
  // The operator surface reads every tenant unless the address names one.
  const tenantId = search.tenant ?? null;
  const { setSearch, setSearchAction } = useObservabilityNavigation(
    "/operator/observability",
  );
  const tabProps = {
    search,
    tenantId,
    allowAllTenants: true,
    setSearch,
    setSearchAction,
  };
  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-admin-observability"
    >
      <div className="flex shrink-0 flex-col gap-3">
        <PageHeader
          title="Operator observability"
          subtitle="Logs and runs across every tenant. Pick a tenant in the facet bar to narrow the view."
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
      {tab === "logs" ? <LogsTab {...tabProps} /> : <RunsTab {...tabProps} />}
    </section>
  );
}
