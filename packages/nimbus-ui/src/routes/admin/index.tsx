import { createFileRoute } from "@tanstack/react-router";

import { EmptyState } from "../../components/empty-state";

export const Route = createFileRoute("/admin/")({
  component: SystemOverviewPage,
});

function SystemOverviewPage() {
  return (
    <section
      className="flex h-full flex-col"
      data-testid="page-admin-system"
    >
      <EmptyState
        title="System"
        body="Server-wide health, tenant count, machine state, route adapter status, runtime build, and recent incidents will live here. Jump to Machines or Services for live status in the meantime."
        cta={{ label: "Machines", to: "/admin/machines" }}
        testid="admin-system-empty"
      />
    </section>
  );
}
