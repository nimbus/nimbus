import { createFileRoute } from "@tanstack/react-router";

import { PlaceholderPage } from "../shell/placeholder-page";

export const Route = createFileRoute("/")({
  component: OverviewPage,
});

function OverviewPage() {
  return (
    <PlaceholderPage
      title="Overview"
      summary="Deployment health, recent activity, and the next concrete actions."
      hint="DU4 lights up the dense health/compute/storage/network panels."
    />
  );
}
