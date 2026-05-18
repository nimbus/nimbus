import { createFileRoute } from "@tanstack/react-router";
import { useMemo } from "react";

import { EmptyState } from "../../components/empty-state";
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
        <div className="px-3 py-6 text-xs text-muted">No buckets yet.</div>
      ),
    }),
    [],
  );
  useContributeSubDrawer(spec);
  return (
    <section
      className="flex h-full flex-col"
      data-testid="page-files"
    >
      <EmptyState
        title="Object storage"
        body="Buckets, object browsing, and signed-URL uploads scoped to the active tenant will live here."
        testid="files-empty"
      />
    </section>
  );
}
