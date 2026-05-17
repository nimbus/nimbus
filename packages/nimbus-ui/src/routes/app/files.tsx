import { createFileRoute } from "@tanstack/react-router";
import { useMemo } from "react";
import { PlaceholderPage } from "../../shell/placeholder-page";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";

export const Route = createFileRoute("/app/files")({
  component: FilesPage,
});

function FilesPage() {
  const spec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Files",
      search: { placeholder: "Filter buckets" },
      children: (
        <div className="px-3 py-6 text-xs text-muted">
          <p>No buckets yet.</p>
          <p className="mt-2">
            Buckets appear here once the file storage API is wired in.
          </p>
        </div>
      ),
    }),
    [],
  );
  useContributeSubDrawer(spec);
  return (
    <PlaceholderPage
      title="Files"
      summary="Object storage buckets scoped to the active tenant. Upload, preview, and manage signed URLs."
      hint="Bucket list + object browser lands in DU-shell O3 once the file storage API surface is wired."
    />
  );
}
