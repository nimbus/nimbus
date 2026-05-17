import { createFileRoute } from "@tanstack/react-router";
import { PlaceholderPage } from "../../shell/placeholder-page";

export const Route = createFileRoute("/app/schedules")({
  component: SchedulesPage,
});

function SchedulesPage() {
  return (
    <PlaceholderPage
      title="Schedules"
      summary="Cron and queued jobs for the active tenant. Inspect upcoming runs, retry policies, and recent execution history."
      hint="Schedule list + detail drawer lands in DU-shell O3 alongside the scheduled_jobs table view."
    />
  );
}
