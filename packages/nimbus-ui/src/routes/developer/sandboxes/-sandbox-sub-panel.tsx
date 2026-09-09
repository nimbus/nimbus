import { Link } from "@tanstack/react-router";
import { cn } from "@/lib/utils";
import { StateDot } from "../../../components/state-dot";
import type { SandboxResource } from "../../../lib/types/sandbox";
import { sandboxDisplayName } from "../../../lib/types/sandbox";
import type { LoadingValue } from "../../../shell/loading-value";
import { useSubPanelSearch } from "../../../shell/sub-panel";
import type { SandboxListRead } from "./-sandbox-read";

// The dynamic list beside the sandbox pages: one row per live sandbox in
// the active tenant with its state dot. Both the list and the detail page
// contribute it, so the list keeps its shape when a row is opened.
export function SandboxSubPanel({
  read,
  activeId,
  activeTenant,
}: {
  read: LoadingValue<SandboxListRead>;
  activeId?: string;
  activeTenant: string | null;
}) {
  const filter = useSubPanelSearch().trim().toLowerCase();
  if (activeTenant === null) {
    return (
      <div className="px-3 py-6 text-xs text-text-3">
        Choose a tenant to list its sandboxes.
      </div>
    );
  }
  if (read.kind === "loading") {
    return (
      <div className="px-3 py-6 text-xs text-text-3">Reading sandboxes…</div>
    );
  }
  if (read.kind !== "ok") {
    return (
      <div className="px-3 py-6 text-xs text-text-3">
        The sandbox list did not load.
      </div>
    );
  }
  if (read.value.kind === "unavailable") {
    return (
      <div className="px-3 py-6 text-xs text-text-3">
        Sandbox routes are not available on this server.
      </div>
    );
  }
  const items = read.value.items;
  const filtered = filter
    ? items.filter(
        (s) =>
          s.metadata.id.toLowerCase().includes(filter) ||
          sandboxDisplayName(s).toLowerCase().includes(filter) ||
          s.status.lifecycleState.toLowerCase().includes(filter) ||
          s.spec.profile.toLowerCase().includes(filter),
      )
    : items;
  if (items.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-text-3">
        No live sandboxes in tenant {activeTenant}.
      </div>
    );
  }
  if (filtered.length === 0) {
    return (
      <div className="px-3 py-6 text-xs text-text-3">
        No sandboxes match the filter.
      </div>
    );
  }
  return (
    <ul className="flex flex-col gap-px px-2 py-2">
      {filtered.map((sandbox) => (
        <SubPanelRow
          key={sandbox.metadata.id}
          sandbox={sandbox}
          active={sandbox.metadata.id === activeId}
        />
      ))}
    </ul>
  );
}

function SubPanelRow({
  sandbox,
  active,
}: {
  sandbox: SandboxResource;
  active: boolean;
}) {
  const id = sandbox.metadata.id;
  return (
    <li>
      <Link
        to="/developer/sandboxes/$sandbox"
        params={{ sandbox: id }}
        data-testid={`sub-panel-item-dev-sandbox-${id}`}
        className={cn(
          "flex h-8 items-center gap-2 rounded-md px-2 text-sm",
          active
            ? "bg-bg-raised text-text-1"
            : "text-text-3 hover:bg-bg-raised hover:text-text-1",
        )}
      >
        <StateDot state={sandbox.status.lifecycleState} />
        <span className="flex-1 truncate font-mono text-xs">
          {sandboxDisplayName(sandbox)}
        </span>
        <span className="tabular text-xs font-medium text-text-3">
          {sandbox.status.lifecycleState}
        </span>
      </Link>
    </li>
  );
}
