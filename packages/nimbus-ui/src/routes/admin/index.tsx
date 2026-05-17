import { createFileRoute } from "@tanstack/react-router";
import { PlaceholderPage } from "../../shell/placeholder-page";

export const Route = createFileRoute("/admin/")({
  component: SystemOverviewPage,
});

function SystemOverviewPage() {
  return (
    <PlaceholderPage
      title="System"
      summary="Server-wide health: tenant count, machine state, route adapter status, runtime build, schema/storage versions, and recent incidents."
      hint="System overview cards land in DU-shell O2 once the operator metrics surface is finalised."
    />
  );
}
