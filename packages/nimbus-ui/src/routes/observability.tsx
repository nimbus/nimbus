import { createFileRoute } from "@tanstack/react-router";

import { PlaceholderPage } from "../shell/placeholder-page";

export const Route = createFileRoute("/observability")({
  component: ObservabilityPage,
});

function ObservabilityPage() {
  return (
    <PlaceholderPage
      title="Observability"
      summary="Logs, events, runs, traces."
      hint="DU8 ships logs and run timelines."
    />
  );
}
