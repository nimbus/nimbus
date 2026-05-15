import { createFileRoute } from "@tanstack/react-router";

import { PlaceholderPage } from "../shell/placeholder-page";

export const Route = createFileRoute("/network")({
  component: NetworkPage,
});

function NetworkPage() {
  return (
    <PlaceholderPage
      title="Network"
      summary="Listeners, routes, subscriptions, published ports, machine API."
      hint="DU6 ships route and subscription panels."
    />
  );
}
