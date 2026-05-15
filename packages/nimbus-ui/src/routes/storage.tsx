import { createFileRoute } from "@tanstack/react-router";

import { PlaceholderPage } from "../shell/placeholder-page";

export const Route = createFileRoute("/storage")({
  component: StoragePage,
});

function StoragePage() {
  return (
    <PlaceholderPage
      title="Storage"
      summary="Tenants, tables, documents, schema, indexes."
      hint="DU7 ships the data browser and tenant lifecycle."
    />
  );
}
