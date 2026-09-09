import { createFileRoute } from "@tanstack/react-router";

import { EmptyState } from "../../components/empty-state";
import {
  type SubPanelSpec,
  useContributeSubPanel,
} from "../../shell/sub-panel";

export const Route = createFileRoute("/developer/settings")({
  component: TenantSettingsPage,
});

const TENANT_SETTINGS_SUB_PANEL: SubPanelSpec = {
  kind: "static",
  title: "Settings",
  items: [
    {
      id: "environment",
      label: "Environment",
      to: "/developer/settings",
      search: { section: "environment" },
    },
    {
      id: "secrets",
      label: "Secrets",
      to: "/developer/settings",
      search: { section: "secrets" },
    },
    {
      id: "schema",
      label: "Schema",
      to: "/developer/settings",
      search: { section: "schema" },
    },
    {
      id: "integrations",
      label: "Integrations",
      to: "/developer/settings",
      search: { section: "integrations" },
    },
  ],
};

function TenantSettingsPage() {
  useContributeSubPanel(TENANT_SETTINGS_SUB_PANEL);
  return (
    <section className="flex h-full flex-col" data-testid="page-settings">
      <EmptyState
        title="Tenant settings"
        body="Members, API keys, environment variables, deploy keys, and appearance preferences will live here. Server-wide configuration lives under the operator console."
        cta={{ label: "Operator settings", to: "/operator/settings" }}
        testid="settings-empty"
      />
    </section>
  );
}
