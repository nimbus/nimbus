import { createFileRoute } from "@tanstack/react-router";
import { PlaceholderPage } from "../../shell/placeholder-page";

export const Route = createFileRoute("/admin/observability")({
  component: AdminObservabilityPage,
});

function AdminObservabilityPage() {
  return (
    <PlaceholderPage
      title="Observability"
      summary="Server-wide logs and runs. Filter by tenant via the optional ?tenant=<id> query, or leave unset to see every tenant."
      hint="Operator observability reuses the developer observability surface in DU-shell O4 with an additional tenant filter."
    />
  );
}
