import { createFileRoute } from "@tanstack/react-router";

import { PlaceholderPage } from "../shell/placeholder-page";

export const Route = createFileRoute("/machines")({
  component: MachinesPage,
});

function MachinesPage() {
  return (
    <PlaceholderPage
      title="Machines"
      summary="Host/guest lifecycle, boot image, services, ports."
      hint="DU5 ships the list, detail drawer, and optimistic actions."
    />
  );
}
