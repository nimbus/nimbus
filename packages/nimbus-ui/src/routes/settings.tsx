import { createFileRoute } from "@tanstack/react-router";

import { PlaceholderPage } from "../shell/placeholder-page";

export const Route = createFileRoute("/settings")({
  component: SettingsPage,
});

function SettingsPage() {
  return (
    <PlaceholderPage
      title="Settings"
      summary="Server info, deploys, token rotation, integrations."
      hint="DU9 ships the deploy diff viewer and adapter capability matrices."
    />
  );
}
