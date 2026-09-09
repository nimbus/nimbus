import { useQuery } from "@nimbus/nimbus/react";
import { createFileRoute, redirect } from "@tanstack/react-router";

import { api } from "../../../convex/_generated/api";
import { AppearanceSection } from "../../components/appearance-section";
import { PageHeader } from "../../components/page-header";
import { useContributeSubPanel } from "../../shell/sub-panel";
import { ConfigurationSection } from "./settings/-configuration";
import { DangerZoneSection } from "./settings/-danger-zone";
import { DeploysSection } from "./settings/-deploys";
import {
  useEncryptionStatus,
  useLicenseSnapshot,
  useRuntimeDiagnostics,
} from "./settings/-hooks";
import { IntegrationsSection } from "./settings/-integrations";
import { ServerInfoSection, TenantHeaderStrip } from "./settings/-server-info";
import { ADMIN_SETTINGS_SUB_PANEL } from "./settings/-sub-panel";
import type {
  AdapterCapabilityDoc,
  BundleDoc,
  FunctionDoc,
  SystemStatusDoc,
} from "./settings/-types";

// The sub-panel's five sub-pages are the section space: the route validates
// exactly the ids the menu can produce. `settings.spec.tsx` asserts the two
// stay in step at compile time. Every section has a built pane; a planned
// sub-page joins the list when its pane lands (DESIGN.md: a static menu
// lists only pages that exist).
const SECTIONS = [
  "general",
  "system",
  "deploys",
  "integrations",
  "shutdown",
] as const;

export type SettingsSection = (typeof SECTIONS)[number];
type SettingsSearch = { section: SettingsSection };

export function parseSettingsSection(
  value: unknown,
): SettingsSection | undefined {
  return typeof value === "string" &&
    (SECTIONS as readonly string[]).includes(value)
    ? (value as SettingsSection)
    : undefined;
}

export const Route = createFileRoute("/operator/settings")({
  component: SettingsPage,
  validateSearch: (search: Record<string, unknown>): SettingsSearch => ({
    section: parseSettingsSection(search.section) ?? "general",
  }),
  // `isItemActive` in the sub-panel compares search values exactly, so a bare
  // `/operator/settings` would leave every item inactive. Normalizing the URL
  // to the default section is what makes the menu locate the operator, and it
  // keeps each section deep-linkable (DESIGN.md: "URL is state").
  beforeLoad: ({ search }) => {
    if (
      parseSettingsSection((search as Record<string, unknown>).section) ===
      undefined
    ) {
      throw redirect({
        to: "/operator/settings",
        search: { section: "general" },
        replace: true,
      });
    }
  },
});

const SECTION_SUBTITLES: Record<SettingsSection, string> = {
  general: "Appearance, license and usage, and effective configuration.",
  system:
    "Server identity: version, health, uptime, listen address, data directory, encryption.",
  deploys:
    "Bundle history, active release, and the functions each bundle ships.",
  integrations:
    "Adapter capability matrices — what each protocol surface implements today.",
  shutdown:
    "Session lifecycle: admin-token rotation and graceful server shutdown.",
};

function SettingsPage() {
  useContributeSubPanel(ADMIN_SETTINGS_SUB_PANEL);
  const section = Route.useSearch().section;
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
      data-section={section}
    >
      <PageHeader title="Settings" subtitle={SECTION_SUBTITLES[section]} />

      {section === "deploys" ? (
        <DeploysSection bundles={bundles} functions={functions} />
      ) : section === "integrations" ? (
        <IntegrationsSection capabilities={capabilities} />
      ) : section === "shutdown" ? (
        <DangerZoneSection />
      ) : section === "system" ? (
        <ServerInfoSection status={status} encryption={encryption} />
      ) : (
        <>
          <AppearanceSection />
          <TenantHeaderStrip status={status} license={license} />
          <ConfigurationSection
            diagnostics={diagnostics}
            license={license}
            status={status}
          />
        </>
      )}
    </section>
  );
}
