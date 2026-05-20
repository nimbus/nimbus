import { StateChip } from "../../../components/state-chip";
import { RelativeTime } from "../../../components/time";
import { Definition, DefinitionList, SectionCard } from "./primitives";
import type {
  AsyncSnapshot,
  LicenseSnapshot,
  RuntimeDiagnostics,
  SystemStatusDoc,
} from "./types";

export function ConfigurationSection({
  diagnostics,
  license,
  status,
}: {
  diagnostics: AsyncSnapshot<RuntimeDiagnostics>;
  license: AsyncSnapshot<LicenseSnapshot>;
  status: SystemStatusDoc | undefined;
}) {
  const limits =
    diagnostics === "loading" || diagnostics === "error"
      ? null
      : (diagnostics.limits ?? null);
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
  const licenseWarnings =
    license !== "loading" && license !== "error"
      ? (license.warnings ?? [])
      : [];
  return (
    <SectionCard
      title="Configuration"
      testid="settings-configuration"
      description="Runtime limits, license entitlements, auth provider, adapter enablement, and storage topology."
    >
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <div>
          <h3 className="mb-2 text-xs uppercase tracking-[0.14em] text-muted">
            Runtime limits
          </h3>
          {diagnostics === "loading" ? (
            <p className="text-sm text-muted">Loading runtime metrics…</p>
          ) : diagnostics === "error" ? (
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
          {license === "loading" ? (
            <p className="text-sm text-muted">Loading license snapshot…</p>
          ) : license === "error" ? (
            <p className="text-sm text-danger">License unavailable.</p>
          ) : (
            <DefinitionList compact>
              <Definition label="Kind">
                <span className="font-mono text-xs">{license.kind ?? "—"}</span>
              </Definition>
              <Definition label="Status">
                <StateChip state={license.status ?? "unknown"} />
              </Definition>
              <Definition label="Issued to">
                <span className="font-mono text-xs">
                  {license.issued_to ?? "—"}
                </span>
              </Definition>
              <Definition label="Issued by">
                <span className="font-mono text-xs">
                  {license.issued_by ?? "—"}
                </span>
              </Definition>
              <Definition label="MAU">
                <span className="font-mono text-xs tabular">
                  {typeof license.usage?.monthly_active_users === "number"
                    ? license.usage.monthly_active_users
                    : "—"}
                  {license.monthly_active_user_limit
                    ? ` / ${license.monthly_active_user_limit}`
                    : ""}
                </span>
              </Definition>
              <Definition label="Expires">
                {typeof license.expires_at_unix_ms === "number" ? (
                  <RelativeTime epochMs={license.expires_at_unix_ms} />
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
