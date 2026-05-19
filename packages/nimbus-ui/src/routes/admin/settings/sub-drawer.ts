import type { StaticSubDrawerSpec } from "../../../shell/sub-drawer";

export const ADMIN_SETTINGS_SUB_DRAWER = {
  kind: "static",
  title: "Settings",
  items: [
    {
      id: "general",
      label: "General",
      to: "/admin/settings",
      search: { section: "general" },
    },
    {
      id: "endpoints",
      label: "Endpoints",
      to: "/admin/settings",
      search: { section: "endpoints" },
    },
    {
      id: "deploys",
      label: "Deploys",
      to: "/admin/settings",
      search: { section: "deploys" },
    },
    {
      id: "token",
      label: "Token",
      to: "/admin/settings",
      search: { section: "token" },
    },
    {
      id: "environment",
      label: "Environment",
      to: "/admin/settings",
      search: { section: "environment" },
    },
    {
      id: "integrations",
      label: "Integrations",
      to: "/admin/settings",
      search: { section: "integrations" },
    },
    {
      id: "shutdown",
      label: "Shutdown",
      to: "/admin/settings",
      search: { section: "shutdown" },
    },
  ],
} as const satisfies StaticSubDrawerSpec<
  | "general"
  | "endpoints"
  | "deploys"
  | "token"
  | "environment"
  | "integrations"
  | "shutdown"
>;
