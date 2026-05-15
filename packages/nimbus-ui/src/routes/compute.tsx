import { createFileRoute } from "@tanstack/react-router";

import { PlaceholderPage } from "../shell/placeholder-page";

export const Route = createFileRoute("/compute")({
  component: ComputePage,
});

function ComputePage() {
  return (
    <PlaceholderPage
      title="Compute"
      summary="Functions, actions, HTTP routes, scheduled jobs, services."
      hint="DU6 ships the function explorer; DU6.5 adds the function runner."
    />
  );
}
