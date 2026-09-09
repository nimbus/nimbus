import { CopyChip } from "../../../components/copy-chip";
import { shortHash } from "../../../lib/format";
import type { DeployHistory } from "../../../lib/types/deploy";
import type { LoadingValue } from "../../../shell/loading-value";

// The sub-panel beside the history: the bundle serving requests now, and
// how many activations the server remembers. It reads the same value as
// the table, so a rollback moves both at once.
export function DeploySubPanel({
  read,
}: {
  read: LoadingValue<DeployHistory>;
}) {
  if (read.kind !== "ok") {
    return (
      <p
        className="px-3 py-2 text-xs text-text-3"
        data-testid="deploys-panel-empty"
      >
        {read.kind === "loading" ? "Reading…" : "No deploy history."}
      </p>
    );
  }
  const { active, activations } = read.value;
  const current = activations.find((row) => row.sha256 === active) ?? null;
  const retained = activations.filter((row) => row.retained).length;
  return (
    <dl
      className="flex flex-col gap-2 px-3 py-2 text-xs"
      data-testid="deploys-panel"
    >
      <div className="flex flex-col gap-0.5">
        <dt className="text-text-3">Active bundle</dt>
        <dd>
          {current ? (
            <CopyChip
              label="active bundle sha256"
              value={current.sha256}
              testid="deploys-panel-active"
            >
              <span className="font-mono">{shortHash(current.sha256)}</span>
            </CopyChip>
          ) : (
            <span className="text-text-3">none</span>
          )}
        </dd>
      </div>
      <div className="flex flex-col gap-0.5">
        <dt className="text-text-3">Generation</dt>
        <dd
          className="font-mono tabular"
          data-testid="deploys-panel-generation"
        >
          {current ? current.generation : "—"}
        </dd>
      </div>
      <div className="flex flex-col gap-0.5">
        <dt className="text-text-3">Functions</dt>
        <dd className="font-mono tabular">
          {current ? current.functions.length : "—"}
        </dd>
      </div>
      <div className="flex flex-col gap-0.5">
        <dt className="text-text-3">Activations</dt>
        <dd className="font-mono tabular" data-testid="deploys-panel-count">
          {activations.length} ({retained} retained)
        </dd>
      </div>
    </dl>
  );
}
