import { createFileRoute } from "@tanstack/react-router";
import { PlaceholderPage } from "../../shell/placeholder-page";

export const Route = createFileRoute("/app/files")({
  component: FilesPage,
});

function FilesPage() {
  return (
    <PlaceholderPage
      title="Files"
      summary="Object storage buckets scoped to the active tenant. Upload, preview, and manage signed URLs."
      hint="Bucket list + object browser lands in DU-shell O3 once the file storage API surface is wired."
    />
  );
}
