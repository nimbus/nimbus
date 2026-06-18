import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "@nimbus/nimbus/react";

import { api } from "../../../convex/_generated/api";
import { AppearanceSection } from "../../components/appearance-section";
import { PageHeader } from "../../components/page-header";
import { useContributeSubDrawer } from "../../shell/sub-drawer";
import { ConfigurationSection } from "./settings/configuration";
import { DangerZoneSection } from "./settings/danger-zone";
import { DeploysSection } from "./settings/deploys";
import {
  useEncryptionStatus,
  useLicenseSnapshot,
  useRuntimeDiagnostics,
} from "./settings/hooks";
import { IntegrationsSection } from "./settings/integrations";
import { ServerInfoSection, TenantHeaderStrip } from "./settings/server-info";
import { ADMIN_SETTINGS_SUB_DRAWER } from "./settings/sub-drawer";
import type {
  AdapterCapabilityDoc,
  BundleDoc,
  FunctionDoc,
  SystemStatusDoc,
} from "./settings/types";

export const Route = createFileRoute("/operator/settings")({
  component: SettingsPage,
});

function SettingsPage() {
  useContributeSubDrawer(ADMIN_SETTINGS_SUB_DRAWER);
  const status = useQuery(api.system.status, {}) as SystemStatusDoc | undefined;
  const capabilities = useQuery(api.adapter_capabilities.list, {
    adapter: null,
    status: null,
    limit: 500,
  }) as AdapterCapabilityDoc[] | undefined;
  const bundles = useQuery(api.bundles.list, {
    status: null,
    limit: 50,
  }) as BundleDoc[] | undefined;
  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 500,
  }) as FunctionDoc[] | undefined;

  const license = useLicenseSnapshot();
  const encryption = useEncryptionStatus();
  const diagnostics = useRuntimeDiagnostics();

  return (
    <section
      className="flex h-full flex-col gap-5 overflow-y-auto px-6 py-5"
      data-testid="page-settings"
    >
      <PageHeader
        title="Settings"
        subtitle="Server info, configuration, integrations, deploy history, and session lifecycle."
      />

      <AppearanceSection />

      <TenantHeaderStrip status={status} license={license} />

      <ServerInfoSection status={status} encryption={encryption} />

      <ConfigurationSection
        diagnostics={diagnostics}
        license={license}
        status={status}
      />

      <IntegrationsSection capabilities={capabilities} />

      <DeploysSection bundles={bundles} functions={functions} />

      <DangerZoneSection />
    </section>
  );
}
