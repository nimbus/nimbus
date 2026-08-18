import { StateChip } from "../../../components/state-chip";
import { RelativeTime } from "../../../components/time";
import type { LoadingValue } from "../../../shell/loading-value";
import { Definition, DefinitionList, SectionCard } from "./-primitives";
import type {
  LicenseSnapshot,
  RuntimeDiagnostics,
  RuntimeLaneDiagnostics,
  SystemStatusDoc,
} from "./-types";

export function ConfigurationSection({
  diagnostics,
  license,
  status,
}: {
  diagnostics: LoadingValue<RuntimeDiagnostics>;
  license: LoadingValue<LicenseSnapshot>;
  status: SystemStatusDoc | undefined;
}) {
  const diagnosticsSnap =
    diagnostics.kind === "ok" ? diagnostics.value : null;
  const diagnosticsState: "loading" | "error" | "ok" =
    diagnostics.kind === "loading"
      ? "loading"
      : diagnosticsSnap
        ? "ok"
        : "error";
  const licenseSnap = license.kind === "ok" ? license.value : null;
  const licenseState: "loading" | "error" | "ok" =
    license.kind === "loading" ? "loading" : licenseSnap ? "ok" : "error";
  const limits = diagnosticsSnap?.limits ?? null;
  const lanes = diagnosticsSnap?.lanes ?? [];
  const details = (status?.details ?? {}) as Record<string, unknown>;
  const authProvider =
    typeof details.authProvider === "string"
      ? details.authProvider
      : typeof details.auth === "string"
        ? details.auth
        : "admin-local";
  const adaptersEnabledRaw =
    typeof details.adapters === "object" && details.adapters !== null
      ? (details.adapters as Record<string, unknown>)
      : null;
  const adaptersEnabled = adaptersEnabledRaw
    ? Object.keys(adaptersEnabledRaw).filter((k) => adaptersEnabledRaw[k])
    : null;
  const licenseWarnings = licenseSnap?.warnings ?? [];
  return (
    <SectionCard
      title="Configuration"
      testid="settings-configuration"
      description="Runtime limits, runtime lanes, license entitlements, auth provider, adapter enablement, and storage topology."
    >
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <div className="space-y-4">
          <h3 className="mb-2 text-xs uppercase tracking-[0.14em] text-muted">
            Runtime limits
          </h3>
          {diagnosticsState === "loading" ? (
            <p className="text-sm text-muted">Loading runtime metrics…</p>
          ) : diagnosticsState === "error" ? (
            <p className="text-sm text-danger">Runtime metrics unavailable.</p>
          ) : limits === null || Object.keys(limits).length === 0 ? (
            <p className="text-sm text-muted">
              No active app generation — deploy a bundle to populate runtime
              limits.
            </p>
          ) : (
            <DefinitionList compact>
              <Definition label="Backend">
                <span className="font-mono text-xs">
                  {limits.runtime_backend ?? "—"}
                </span>
              </Definition>
              <Definition label="Language">
                <span className="font-mono text-xs">
                  {limits.runtime_language ?? "—"}
                </span>
              </Definition>
              <Definition label="Preset">
                <span className="font-mono text-xs">
                  {limits.runtime_preset ?? "—"}
                </span>
              </Definition>
              <Definition label="Mode">
                <span className="font-mono text-xs">
                  {limits.runtime_mode ?? "—"}
                </span>
              </Definition>
              <Definition label="Memory">
                <span
                  className="font-mono text-xs"
                  data-testid="settings-runtime-memory-enforcement"
                >
                  {limits.memory_enforcement ?? "—"}
                </span>
              </Definition>
              <Definition label="Heap (MB)">
                <span className="font-mono text-xs tabular">
                  {limits.initial_heap_mb ?? "—"} → {limits.max_heap_mb ?? "—"}
                </span>
              </Definition>
              <Definition label="Exec timeout">
                <span className="font-mono text-xs tabular">
                  {typeof limits.execution_timeout_ms === "number"
                    ? `${limits.execution_timeout_ms}ms`
                    : "—"}
                </span>
              </Definition>
              <Definition label="Workers">
                <span className="font-mono text-xs tabular">
                  {limits.worker_threads ?? "—"}
                </span>
              </Definition>
              <Definition label="Concurrent runtimes">
                <span className="font-mono text-xs tabular">
                  {limits.max_concurrent_runtime_instances ?? "—"}
                </span>
              </Definition>
              <Definition label="Active per tenant">
                <span className="font-mono text-xs tabular">
                  {limits.max_active_top_level_invocations_per_tenant ?? "—"}
                </span>
              </Definition>
              <Definition label="Queued per tenant">
                <span className="font-mono text-xs tabular">
                  {limits.max_queued_top_level_invocations_per_tenant ?? "—"}
                </span>
              </Definition>
            </DefinitionList>
          )}
          <div>
            <h3 className="mb-2 text-xs uppercase tracking-[0.14em] text-muted">
              Runtime lanes
            </h3>
            {diagnosticsState === "loading" ? (
              <p className="text-sm text-muted">Loading runtime lanes…</p>
            ) : diagnosticsState === "error" ? (
              <p className="text-sm text-danger">Runtime lanes unavailable.</p>
            ) : lanes.length === 0 ? (
              <p className="text-sm text-muted">
                No runtime lanes advertised yet.
              </p>
            ) : (
              <RuntimeLaneTable lanes={lanes} />
            )}
          </div>
        </div>
        <div>
          <h3 className="mb-2 text-xs uppercase tracking-[0.14em] text-muted">
            Auth & topology
          </h3>
          <DefinitionList compact>
            <Definition label="Auth provider">
              <span
                className="font-mono text-xs text-default"
                data-testid="settings-auth-provider"
              >
                {authProvider}
              </span>
            </Definition>
            <Definition label="Adapters enabled">
              <span className="font-mono text-xs text-default">
                {adaptersEnabled
                  ? adaptersEnabled.join(", ")
                  : "convex, native, ui"}
              </span>
            </Definition>
          </DefinitionList>
          <h3 className="mt-4 mb-2 text-xs uppercase tracking-[0.14em] text-muted">
            License
          </h3>
          {licenseState === "loading" ? (
            <p className="text-sm text-muted">Loading license snapshot…</p>
          ) : licenseState === "error" ? (
            <p className="text-sm text-danger">License unavailable.</p>
          ) : (
            <DefinitionList compact>
              <Definition label="Kind">
                <span className="font-mono text-xs">
                  {licenseSnap?.kind ?? "—"}
                </span>
              </Definition>
              <Definition label="Status">
                <StateChip state={licenseSnap?.status ?? "unknown"} />
              </Definition>
              <Definition label="Issued to">
                <span className="font-mono text-xs">
                  {licenseSnap?.issued_to ?? "—"}
                </span>
              </Definition>
              <Definition label="Issued by">
                <span className="font-mono text-xs">
                  {licenseSnap?.issued_by ?? "—"}
                </span>
              </Definition>
              <Definition label="MAU">
                <span className="font-mono text-xs tabular">
                  {typeof licenseSnap?.usage?.monthly_active_users === "number"
                    ? licenseSnap.usage.monthly_active_users
                    : "—"}
                  {licenseSnap?.monthly_active_user_limit
                    ? ` / ${licenseSnap.monthly_active_user_limit}`
                    : ""}
                </span>
              </Definition>
              <Definition label="Expires">
                {typeof licenseSnap?.expires_at_unix_ms === "number" ? (
                  <RelativeTime epochMs={licenseSnap.expires_at_unix_ms} />
                ) : (
                  <span className="text-muted">—</span>
                )}
              </Definition>
            </DefinitionList>
          )}
          {licenseWarnings.length > 0 ? (
            <ul
              className="mt-2 list-disc space-y-1 pl-5 text-xs text-warning"
              data-testid="settings-license-warnings"
            >
              {licenseWarnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          ) : null}
        </div>
      </div>
    </SectionCard>
  );
}

function RuntimeLaneTable({ lanes }: { lanes: RuntimeLaneDiagnostics[] }) {
  return (
    <div
      className="overflow-x-auto rounded-md border border-app"
      data-testid="settings-runtime-lanes"
    >
      <table className="min-w-full border-collapse text-left text-xs">
        <thead className="bg-surface-2 text-[10px] uppercase tracking-[0.14em] text-muted">
          <tr>
            <th className="px-2 py-2 font-normal">Lane</th>
            <th className="px-2 py-2 font-normal">Backend</th>
            <th className="px-2 py-2 font-normal">Adapter</th>
            <th className="px-2 py-2 font-normal">Artifact</th>
            <th className="px-2 py-2 font-normal">Executor</th>
            <th className="px-2 py-2 font-normal">Memory</th>
          </tr>
        </thead>
        <tbody>
          {lanes.map((lane) => {
            const limits = lane.limits ?? {};
            const artifact = lane.execution_adapter_artifact;
            const artifactRef =
              artifact?.manifest?.source_ref ?? artifact?.expected?.source_ref;
            return (
              <tr
                key={lane.lane_name}
                data-testid={`settings-runtime-lane-${testIdPart(lane.lane_name)}`}
                className="border-t border-app"
              >
                <td className="px-2 py-2 align-top">
                  <div className="flex flex-col gap-0.5">
                    <span className="font-mono text-xs text-default">
                      {lane.lane_name}
                    </span>
                    {lane.default_lane ? (
                      <span className="font-mono text-[10px] uppercase tracking-wide text-muted">
                        default lane
                      </span>
                    ) : null}
                  </div>
                </td>
                <td className="px-2 py-2 align-top">
                  <div className="flex flex-col gap-0.5">
                    <span className="font-mono text-xs text-default">
                      {limits.runtime_backend ?? "—"}
                    </span>
                    <span className="font-mono text-[10px] text-muted">
                      {limits.compatibility_target ?? "—"}
                    </span>
                  </div>
                </td>
                <td className="px-2 py-2 align-top">
                  <span className="font-mono text-xs text-default">
                    {lane.execution_adapter_state ?? "—"}
                  </span>
                </td>
                <td className="px-2 py-2 align-top">
                  <div className="flex flex-col gap-0.5">
                    <span className="font-mono text-xs text-default">
                      {artifact?.status ?? "—"}
                    </span>
                    <span className="font-mono text-[10px] text-muted">
                      {artifact?.source ?? "—"}
                    </span>
                    {artifactRef ? (
                      <span className="font-mono text-[10px] text-muted">
                        {artifactRef}
                      </span>
                    ) : null}
                  </div>
                </td>
                <td className="px-2 py-2 align-top">
                  <span className="font-mono text-xs text-default">
                    {lane.executor_started ? "started" : "lazy"}
                  </span>
                </td>
                <td className="px-2 py-2 align-top">
                  <span className="font-mono text-xs text-default">
                    {limits.memory_enforcement ?? "—"}
                  </span>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function testIdPart(value: string) {
  return value.replace(/[^a-zA-Z0-9_-]/g, "_");
}
