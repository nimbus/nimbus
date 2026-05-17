import { createFileRoute } from "@tanstack/react-router";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";
import { PlaceholderPage } from "../../shell/placeholder-page";

export const Route = createFileRoute("/app/schedules")({
  component: SchedulesPage,
});

const SCHEDULES_SUB_DRAWER: SubDrawerSpec = {
  kind: "static",
  title: "Schedules",
  items: [
    {
      id: "scheduled",
      label: "Scheduled",
      to: "/app/schedules",
      search: { section: "scheduled" },
    },
    {
      id: "cron",
      label: "Cron",
      to: "/app/schedules",
      search: { section: "cron" },
    },
  ],
};

function SchedulesPage() {
  useContributeSubDrawer(SCHEDULES_SUB_DRAWER);
  return (
    <PlaceholderPage
      title="Schedules"
      summary="Cron and queued jobs for the active tenant. Inspect upcoming runs, retry policies, and recent execution history."
      hint="Schedule list + detail drawer lands in DU-shell O4 alongside the scheduled_jobs table view."
    />
  );
}
