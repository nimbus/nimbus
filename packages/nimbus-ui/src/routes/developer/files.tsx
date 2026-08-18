import { createFileRoute } from "@tanstack/react-router";
import { useMemo } from "react";

import { EmptyState } from "../../components/empty-state";
import {
  type SubDrawerSpec,
  useContributeSubDrawer,
} from "../../shell/sub-drawer";

export const Route = createFileRoute("/developer/files")({
  component: FilesPage,
});

// No bucket or object query exists on the server (`convex/_generated/api`
// registers none, and no adapter registers an object-storage route), so both
// panes say the surface is absent instead of promising it. The drawer also
// drops its "Filter buckets" field: a search box over a list that cannot exist
// implies a working listing (DESIGN.md: Adapter Honesty).
const NOT_IN_BUILD =
  "Object storage browsing is not available in this build. This server registers no bucket or object API.";

function FilesPage() {
  const spec = useMemo<SubDrawerSpec>(
    () => ({
      kind: "dynamic",
      title: "Files",
      children: (
        <p
          className="px-3 py-6 text-xs text-muted"
          data-testid="files-drawer-note"
        >
          {NOT_IN_BUILD}
        </p>
      ),
    }),
    [],
  );
  useContributeSubDrawer(spec);
  return (
    <section className="flex h-full flex-col" data-testid="page-files">
      <EmptyState
        title="Object storage unavailable"
        body={NOT_IN_BUILD}
        cta={{ label: "View registered routes", to: "/operator/network" }}
        testid="files-empty"
      />
    </section>
  );
}
