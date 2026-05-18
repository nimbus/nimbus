import { createFileRoute } from "@tanstack/react-router";

import { EmptyState } from "../../components/empty-state";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";

type AdminObservabilitySearch = {
  tab?: string;
  tenant?: string;
};

export const Route = createFileRoute("/admin/observability")({
  component: AdminObservabilityPage,
  validateSearch: (search: Record<string, unknown>): AdminObservabilitySearch =>
    ({
      tab: typeof search.tab === "string" ? search.tab : undefined,
      tenant: typeof search.tenant === "string" ? search.tenant : undefined,
    }) as AdminObservabilitySearch,
});

const ADMIN_OBSERVABILITY_SUB_DRAWER: SubDrawerSpec = {
  kind: "static",
  title: "Observability",
  items: [
    {
      id: "logs",
      label: "Logs",
      to: "/admin/observability",
      search: { tab: "logs" },
    },
    {
      id: "events",
      label: "Events",
      to: "/admin/observability",
      search: { tab: "events" },
    },
    {
      id: "traces",
      label: "Traces",
      to: "/admin/observability",
      search: { tab: "traces" },
    },
    {
      id: "errors",
      label: "Errors",
      to: "/admin/observability",
      search: { tab: "errors" },
    },
  ],
};

function AdminObservabilityPage() {
  useContributeSubDrawer(ADMIN_OBSERVABILITY_SUB_DRAWER);
  return (
    <section
      className="flex h-full flex-col"
      data-testid="page-admin-observability"
    >
      <EmptyState
        title="Operator observability"
        body="Server-wide logs, runs, events, and errors across every tenant. Filter by tenant via the ?tenant=<id> query, or leave unset to see every tenant."
        testid="admin-observability-empty"
      />
    </section>
  );
}
