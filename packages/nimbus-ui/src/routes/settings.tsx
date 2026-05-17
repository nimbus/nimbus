import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";

import { api } from "../../convex/_generated/api";
import { AppearanceSection } from "../components/appearance-section";
import { CopyChip } from "../components/copy-chip";
import { StateChip } from "../components/state-chip";
import { RelativeTime, Uptime } from "../components/time";
import { UpgradePopover } from "../components/upgrade-popover";
import { useStalenessContext } from "../hooks/use-staleness";
import { formatRelativeTime, shortId } from "../lib/format";

export const Route = createFileRoute("/settings")({
  component: SettingsPage,
});

type SystemStatusDoc = {
  _id?: string;
  name?: string;
  version?: string;
  health?: string;
  startedAt?: number;
  updatedAt?: number;
  details?: Record<string, unknown> | null;
} | null;

type BundleDoc = {
  _id?: string;
  _creationTime?: number;
  sha256?: string;
  sizeBytes?: number;
  sourceRef?: string;
  status?: string;
};

type FunctionDoc = {
  _id?: string;
  bundleId?: string;
  path?: string;
  kind?: string;
  argsSchema?: unknown;
  returnsSchema?: unknown;
};

type AdapterCapabilityDoc = {
  _id?: string;
  adapter?: string;
  feature?: string;
  status?: string;
  caveat?: string;
  evidence?: string;
};

type LicenseSnapshot = {
  source?: { kind?: string; path?: string };
  kind?: string;
  status?: string;
  issued_to?: string | null;
  issued_by?: string | null;
  issued_at_unix_ms?: number | null;
  expires_at_unix_ms?: number | null;
  trial_expires_at_unix_ms?: number | null;
  revenue_limit_usd?: number | null;
  monthly_active_user_limit?: number | null;
  entitlements?: Record<string, unknown>;
  usage?: {
    month?: string;
    monthly_active_users?: number;
    limit?: number | null;
    limit_exceeded?: boolean | null;
    last_recorded_at_unix_ms?: number | null;
  };
  warnings?: string[];
};

type EncryptionStatus = {
  enabled?: boolean;
  encrypted_families?: string[];
  descriptor?: Record<string, unknown> | null;
};

type RuntimeDiagnostics = {
  limits?: {
    runtime_backend?: string;
    compatibility_target?: string;
    execution_model?: string;
    runtime_mode?: string;
    runtime_language?: string;
    runtime_preset?: string;
    runtime_pool_kind?: string;
    max_heap_mb?: number;
    initial_heap_mb?: number;
    execution_timeout_ms?: number;
    max_concurrent_runtime_instances?: number;
    worker_threads?: number;
    max_active_top_level_invocations_per_tenant?: number;
    max_in_flight_top_level_invocations_per_tenant?: number;
    max_queued_top_level_invocations_per_tenant?: number;
    max_nested_runtime_invocations?: number;
  };
  metrics?: Record<string, unknown>;
};

const ADAPTERS = [
  { id: "convex", label: "Convex" },
  { id: "mongodb", label: "MongoDB" },
  { id: "firebase", label: "Firebase" },
  { id: "cloud_functions", label: "Cloud Functions" },
  { id: "native", label: "Native" },
] as const;

function SettingsPage() {
  const status = useQuery(api.system.status, {}) as SystemStatusDoc | undefined;
  const capabilities = useQuery(api.adapter_capabilities.list, {
    adapter: null,
    status: null,
    limit: 500,
  }) as AdapterCapabilityDoc[] | undefined;
  const bundles = useQuery(api.bundles.list, {
    status: null,
    limit: 50,
  }) as BundleDoc[] | undefined;
  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 500,
  }) as FunctionDoc[] | undefined;

  const [license, setLicense] = useState<LicenseSnapshot | "loading" | "error">(
    "loading",
  );
  const [encryption, setEncryption] = useState<
    EncryptionStatus | "loading" | "error"
  >("loading");
  const [diagnostics, setDiagnostics] = useState<
    RuntimeDiagnostics | "loading" | "error"
  >("loading");

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const res = await fetch("/debug/license/status", {
          credentials: "include",
        });
        if (!res.ok) throw new Error(`license ${res.status}`);
        const body = (await res.json()) as LicenseSnapshot;
        if (!cancelled) setLicense(body);
      } catch {
        if (!cancelled) setLicense("error");
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const res = await fetch("/debug/encryption/status", {
          credentials: "include",
        });
        if (!res.ok) throw new Error(`encryption ${res.status}`);
        const body = (await res.json()) as EncryptionStatus;
        if (!cancelled) setEncryption(body);
      } catch {
        if (!cancelled) setEncryption("error");
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const res = await fetch("/debug/runtime/metrics", {
          credentials: "include",
        });
        if (res.status === 404) {
          if (!cancelled) setDiagnostics({});
          return;
        }
        if (!res.ok) throw new Error(`metrics ${res.status}`);
        const body = (await res.json()) as RuntimeDiagnostics;
        if (!cancelled) setDiagnostics(body);
      } catch {
        if (!cancelled) setDiagnostics("error");
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <section
      className="flex h-full flex-col gap-5 overflow-y-auto px-6 py-5"
      data-testid="page-settings"
    >
      <header className="flex items-baseline justify-between">
        <div>
          <h1
            className="text-xl text-default"
            style={{ fontSize: "var(--text-xl)" }}
          >
            Settings
          </h1>
          <p className="text-sm text-muted">
            Server info, configuration, integrations, deploy history, and
            session lifecycle.
          </p>
        </div>
      </header>

      <AppearanceSection />

      <TenantHeaderStrip status={status} license={license} />

      <ServerInfoSection status={status} encryption={encryption} />

      <ConfigurationSection
        diagnostics={diagnostics}
        license={license}
        status={status}
      />

      <IntegrationsSection capabilities={capabilities} />

      <DeploysSection bundles={bundles} functions={functions} />

      <DangerZoneSection />
    </section>
  );
}

function TenantHeaderStrip({
  status,
  license,
}: {
  status: SystemStatusDoc | undefined;
  license: LicenseSnapshot | "loading" | "error";
}) {
  const details = (status?.details ?? {}) as Record<string, unknown>;
  const storageBackend =
    typeof details.storageBackend === "string"
      ? details.storageBackend
      : typeof details.storage === "string"
        ? details.storage
        : "—";
  const licenseLabel =
    license === "loading"
      ? "loading…"
      : license === "error"
        ? "unavailable"
        : (license.kind ?? "developer");
  const licenseStatus =
    license === "loading" || license === "error"
      ? null
      : (license.status ?? null);
  const usageNow =
    license !== "loading" && license !== "error"
      ? (license.usage?.monthly_active_users ?? null)
      : null;
  const usageLimit =
    license !== "loading" && license !== "error"
      ? (license.monthly_active_user_limit ?? license.usage?.limit ?? null)
      : null;
  const usageLabel =
    usageNow === null
      ? "—"
      : usageLimit
        ? `${usageNow} / ${usageLimit} MAU`
        : `${usageNow} MAU`;
  return (
    <div
      data-testid="settings-tenant-header"
      className="grid grid-cols-2 gap-px overflow-hidden rounded-md border border-app bg-surface-2 md:grid-cols-4"
    >
      <Cell label="Active tenant">
        <CopyChip
          label="active tenant"
          value="_nimbus"
          testid="settings-tenant"
        />
      </Cell>
      <Cell label="Storage backend">
        <span className="font-mono text-xs text-default">{storageBackend}</span>
      </Cell>
      <Cell label="License">
        <span
          className="font-mono text-xs text-default"
          data-testid="settings-license-kind"
        >
          {licenseLabel}
          {licenseStatus ? (
            <span className="ml-1 text-muted">· {licenseStatus}</span>
          ) : null}
        </span>
      </Cell>
      <Cell label="Usage">
        <span
          className="font-mono text-xs text-default tabular"
          data-testid="settings-usage"
        >
          {usageLabel}
        </span>
      </Cell>
    </div>
  );
}

function ServerInfoSection({
  status,
  encryption,
}: {
  status: SystemStatusDoc | undefined;
  encryption: EncryptionStatus | "loading" | "error";
}) {
  const details = (status?.details ?? {}) as Record<string, unknown>;
  const listenAddress =
    typeof details.listenAddress === "string"
      ? details.listenAddress
      : typeof details.address === "string"
        ? details.address
        : "—";
  const activeOrigin =
    typeof details.activeOrigin === "string"
      ? details.activeOrigin
      : typeof window !== "undefined"
        ? window.location.origin
        : "—";
  const storageBackend =
    typeof details.storageBackend === "string"
      ? details.storageBackend
      : typeof details.storage === "string"
        ? details.storage
        : "—";
  const encryptionEnabled =
    encryption === "loading" || encryption === "error"
      ? encryption
      : (encryption.enabled ?? false);
  const encryptedFamilies =
    encryption === "loading" || encryption === "error"
      ? []
      : (encryption.encrypted_families ?? []);
  return (
    <SectionCard
      title="Server"
      testid="settings-server-info"
      description="Version, uptime, listen address, storage backend, encryption, and health."
    >
      <DefinitionList>
        <Definition label="Health">
          <StateChip state={status?.health ?? "unknown"} />
        </Definition>
        <Definition label="Version">
          <CopyChip
            label="version"
            value={status?.version ?? "—"}
            testid="settings-server-version"
          />
        </Definition>
        <Definition label="Uptime">
          {typeof status?.startedAt === "number" ? (
            <Uptime startedAtMs={status.startedAt} />
          ) : (
            <span className="tabular text-muted">—</span>
          )}
        </Definition>
        <Definition label="Started">
          {typeof status?.startedAt === "number" ? (
            <RelativeTime epochMs={status.startedAt} />
          ) : (
            <span className="tabular text-muted">—</span>
          )}
        </Definition>
        <Definition label="Listen address">
          <CopyChip
            label="listen address"
            value={listenAddress}
            testid="settings-server-listen"
          />
        </Definition>
        <Definition label="Active origin">
          <CopyChip
            label="active origin"
            value={activeOrigin}
            testid="settings-server-origin"
          />
        </Definition>
        <Definition label="Storage backend">
          <span className="font-mono text-xs text-default">
            {storageBackend}
          </span>
        </Definition>
        <Definition label="Encryption">
          {encryptionEnabled === "loading" ? (
            <span className="text-muted">loading…</span>
          ) : encryptionEnabled === "error" ? (
            <span className="text-danger">unavailable</span>
          ) : encryptionEnabled ? (
            <span
              className="font-mono text-xs text-default"
              data-testid="settings-encryption-enabled"
            >
              on
              {encryptedFamilies.length > 0 ? (
                <span className="ml-1 text-muted">
                  · {encryptedFamilies.join(", ")}
                </span>
              ) : null}
            </span>
          ) : (
            <span className="font-mono text-xs text-muted">off</span>
          )}
        </Definition>
        <Definition label="Updates">
          <UpdatesValue />
        </Definition>
      </DefinitionList>
    </SectionCard>
  );
}

function UpdatesValue() {
  const staleness = useStalenessContext();
  const { snapshot, openPopover, closePopover, startUpgrade, copyCommand } =
    staleness;
  const { state, info } = snapshot;

  if (!info) {
    return <span className="text-muted">loading…</span>;
  }

  if (state === "upgrading") {
    return (
      <span
        data-testid="settings-updates-upgrading"
        className="font-mono text-xs text-default"
      >
        Updating to {info.latest}…
      </span>
    );
  }

  if (state === "upgraded") {
    return (
      <StateChip state="ok" className="data-[state=ok]:text-success" showDot />
    );
  }

  if (state === "hidden" || !info.available || !info.latest) {
    return <StateChip state="ok" showDot />;
  }

  const open = state === "confirming";
  return (
    <span
      data-testid="settings-updates-available"
      className="inline-flex items-center gap-2"
    >
      <UpgradePopover
        open={open}
        onOpenChange={(next) => {
          if (next) openPopover();
          else closePopover();
        }}
        info={info}
        isLocal={staleness.isLocal}
        hasDesktopBridge={staleness.hasDesktopBridge}
        onUpdate={startUpgrade}
        onCopyCommand={copyCommand}
        trigger={
          <span className="inline-flex items-center gap-1.5 font-mono text-xs text-default">
            <span
              aria-hidden
              className="inline-block size-2 rounded-full"
              style={{ background: "var(--color-brand)" }}
            />
            {info.latest} available — Update
          </span>
        }
      />
    </span>
  );
}

function ConfigurationSection({
  diagnostics,
  license,
  status,
}: {
  diagnostics: RuntimeDiagnostics | "loading" | "error";
  license: LicenseSnapshot | "loading" | "error";
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

function IntegrationsSection({
  capabilities,
}: {
  capabilities: AdapterCapabilityDoc[] | undefined;
}) {
  const grouped = useMemo(() => {
    const map = new Map<string, AdapterCapabilityDoc[]>();
    for (const id of ADAPTERS.map((a) => a.id)) map.set(id, []);
    for (const c of capabilities ?? []) {
      const adapter = (c.adapter ?? "").toLowerCase();
      if (!map.has(adapter)) map.set(adapter, []);
      map.get(adapter)?.push(c);
    }
    return map;
  }, [capabilities]);

  return (
    <SectionCard
      title="Integrations"
      testid="settings-integrations"
      description="Adapter capability matrix. Caveats render inline next to the affected feature."
    >
      {capabilities === undefined ? (
        <p className="text-sm text-muted">Loading capability matrix…</p>
      ) : (
        <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
          {ADAPTERS.map(({ id, label }) => {
            const features = grouped.get(id) ?? [];
            return (
              <article
                key={id}
                data-testid={`settings-adapter-${id}`}
                className="rounded-md border border-app bg-surface p-3"
              >
                <header className="mb-2 flex items-baseline justify-between">
                  <span className="text-sm text-default">{label}</span>
                  <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted">
                    {features.length} feature{features.length === 1 ? "" : "s"}
                  </span>
                </header>
                {features.length === 0 ? (
                  <p
                    className="text-xs text-muted"
                    data-testid={`settings-adapter-${id}-empty`}
                  >
                    Not claimed — no capability records published.
                  </p>
                ) : (
                  <ul className="space-y-1.5">
                    {features.map((f) => (
                      <li
                        key={f._id}
                        className="flex flex-col gap-0.5"
                        data-testid={`settings-adapter-${id}-feature-${f.feature ?? ""}`}
                      >
                        <div className="flex items-center justify-between gap-2">
                          <span className="font-mono text-xs text-default">
                            {f.feature ?? "—"}
                          </span>
                          <CapabilityChip status={f.status ?? "unknown"} />
                        </div>
                        {f.caveat ? (
                          <p className="text-[11px] text-warning">
                            ⚠ {f.caveat}
                          </p>
                        ) : null}
                      </li>
                    ))}
                  </ul>
                )}
              </article>
            );
          })}
        </div>
      )}
    </SectionCard>
  );
}

function CapabilityChip({ status }: { status: string }) {
  const lower = status.toLowerCase();
  const tone =
    lower === "supported" || lower === "claimed" || lower === "available"
      ? "success"
      : lower === "caveat" ||
          lower === "supported_with_caveats" ||
          lower === "limited"
        ? "warning"
        : lower === "not_supported" || lower === "not_claimed"
          ? "muted"
          : "muted";
  const colorClass =
    tone === "success"
      ? "text-success"
      : tone === "warning"
        ? "text-warning"
        : "text-muted";
  return (
    <span
      className={`font-mono text-[10px] uppercase tracking-[0.14em] ${colorClass}`}
    >
      {status}
    </span>
  );
}

function DeploysSection({
  bundles,
  functions,
}: {
  bundles: BundleDoc[] | undefined;
  functions: FunctionDoc[] | undefined;
}) {
  const sorted = useMemo(() => {
    const arr = [...(bundles ?? [])];
    arr.sort(
      (a, b) =>
        (b._creationTime ?? 0) - (a._creationTime ?? 0) ||
        (a.sha256 ?? "").localeCompare(b.sha256 ?? ""),
    );
    return arr;
  }, [bundles]);

  const functionsByBundle = useMemo(() => {
    const map = new Map<string, FunctionDoc[]>();
    for (const fn of functions ?? []) {
      if (!fn.bundleId) continue;
      if (!map.has(fn.bundleId)) map.set(fn.bundleId, []);
      map.get(fn.bundleId)?.push(fn);
    }
    return map;
  }, [functions]);

  const active = sorted.find((b) => b.status === "active") ?? sorted[0];
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [showDiff, setShowDiff] = useState(false);

  const toggle = (id: string | undefined) => {
    if (!id) return;
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else if (next.size < 2) next.add(id);
      else {
        const first = next.values().next().value;
        if (first) next.delete(first);
        next.add(id);
      }
      return next;
    });
  };

  const selectedIds = Array.from(selected);
  const canCompare = selectedIds.length === 2;

  return (
    <SectionCard
      title="Deploys"
      testid="settings-deploys"
      description="Active bundle, function inventory, deploy history, and bundle diff. Trigger new deploys with `nimbus deploy` from the CLI."
    >
      {bundles === undefined ? (
        <p className="text-sm text-muted">Loading bundles…</p>
      ) : sorted.length === 0 ? (
        <p className="text-sm text-muted" data-testid="settings-deploys-empty">
          No bundles deployed yet. Run <code>nimbus deploy</code> against this
          server to publish a Convex or Cloud Functions app.
        </p>
      ) : (
        <>
          {active ? (
            <ActiveBundlePanel
              bundle={active}
              functions={
                active._id ? (functionsByBundle.get(active._id) ?? []) : []
              }
            />
          ) : null}
          <div className="mt-4">
            <div className="mb-2 flex items-baseline justify-between">
              <h3 className="text-xs uppercase tracking-[0.14em] text-muted">
                History
              </h3>
              <button
                type="button"
                data-testid="settings-deploys-compare"
                disabled={!canCompare}
                onClick={() => setShowDiff(true)}
                className="rounded border border-app bg-surface px-2 py-1 font-mono text-xs uppercase tracking-[0.14em] text-default hover:border-strong disabled:cursor-not-allowed disabled:text-muted"
              >
                Compare ({selectedIds.length}/2)
              </button>
            </div>
            <ul
              className="divide-y divide-app overflow-hidden rounded-md border border-app"
              data-testid="settings-deploys-history"
            >
              {sorted.slice(0, 20).map((b) => {
                const id = b._id ?? "";
                const fns = id ? (functionsByBundle.get(id) ?? []) : [];
                const isSelected = selected.has(id);
                return (
                  <li
                    key={id}
                    className={`flex items-center gap-3 px-3 py-2 text-xs ${isSelected ? "bg-surface-2" : "bg-surface"}`}
                  >
                    <input
                      type="checkbox"
                      checked={isSelected}
                      onChange={() => toggle(id)}
                      aria-label={`Select bundle ${shortId(b.sha256 ?? id)}`}
                      data-testid={`settings-deploys-row-${b.sha256 ?? id}`}
                    />
                    <StateChip state={b.status ?? "—"} />
                    <CopyChip
                      label="bundle sha256"
                      value={b.sha256 ?? "—"}
                      testid={`settings-deploys-sha-${b.sha256 ?? id}`}
                    >
                      <span className="font-mono text-xs">
                        {shortId(b.sha256 ?? id, 12)}
                      </span>
                    </CopyChip>
                    <span className="font-mono text-xs text-muted">
                      {b.sourceRef ?? "—"}
                    </span>
                    <span className="ml-auto tabular text-muted">
                      {fns.length} fn
                    </span>
                    <span className="tabular text-muted">
                      {b._creationTime
                        ? formatRelativeTime(b._creationTime)
                        : "—"}
                    </span>
                  </li>
                );
              })}
            </ul>
          </div>
          {showDiff && canCompare ? (
            <DiffPanel
              a={sorted.find((b) => b._id === selectedIds[0])}
              b={sorted.find((b) => b._id === selectedIds[1])}
              fnsA={functionsByBundle.get(selectedIds[0]) ?? []}
              fnsB={functionsByBundle.get(selectedIds[1]) ?? []}
              onClose={() => setShowDiff(false)}
            />
          ) : null}
        </>
      )}
    </SectionCard>
  );
}

function ActiveBundlePanel({
  bundle,
  functions,
}: {
  bundle: BundleDoc;
  functions: FunctionDoc[];
}) {
  return (
    <article
      className="rounded-md border border-app bg-surface p-3"
      data-testid="settings-deploys-active"
    >
      <header className="mb-2 flex items-baseline justify-between">
        <span className="text-xs uppercase tracking-[0.14em] text-muted">
          Active bundle
        </span>
        <StateChip state={bundle.status ?? "active"} />
      </header>
      <DefinitionList compact>
        <Definition label="sha256">
          <CopyChip
            label="bundle sha256"
            value={bundle.sha256 ?? "—"}
            testid="settings-deploys-active-sha"
          />
        </Definition>
        <Definition label="Source">
          <span className="font-mono text-xs">{bundle.sourceRef ?? "—"}</span>
        </Definition>
        <Definition label="Size">
          <span className="font-mono text-xs tabular">
            {typeof bundle.sizeBytes === "number"
              ? `${(bundle.sizeBytes / 1024).toFixed(1)} KB`
              : "—"}
          </span>
        </Definition>
        <Definition label="Deployed">
          {bundle._creationTime ? (
            <RelativeTime epochMs={bundle._creationTime} />
          ) : (
            <span className="text-muted">—</span>
          )}
        </Definition>
        <Definition label="Functions">
          <span className="font-mono text-xs tabular">{functions.length}</span>
        </Definition>
      </DefinitionList>
      {functions.length > 0 ? (
        <details className="mt-2">
          <summary className="cursor-pointer text-xs text-muted hover:text-default">
            Function inventory ({functions.length})
          </summary>
          <ul
            className="mt-1 max-h-48 overflow-y-auto space-y-0.5 pl-4 text-xs"
            data-testid="settings-deploys-active-functions"
          >
            {functions.map((fn) => (
              <li key={fn._id} className="flex items-baseline gap-2">
                <span className="font-mono text-default">{fn.path ?? "—"}</span>
                <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted">
                  {fn.kind ?? "—"}
                </span>
              </li>
            ))}
          </ul>
        </details>
      ) : null}
    </article>
  );
}

function DiffPanel({
  a,
  b,
  fnsA,
  fnsB,
  onClose,
}: {
  a: BundleDoc | undefined;
  b: BundleDoc | undefined;
  fnsA: FunctionDoc[];
  fnsB: FunctionDoc[];
  onClose: () => void;
}) {
  const pathsA = new Set(fnsA.map((fn) => fn.path ?? ""));
  const pathsB = new Set(fnsB.map((fn) => fn.path ?? ""));
  const added = [...pathsB].filter((p) => p && !pathsA.has(p)).sort();
  const removed = [...pathsA].filter((p) => p && !pathsB.has(p)).sort();
  const aByPath = new Map(fnsA.map((fn) => [fn.path ?? "", fn]));
  const bByPath = new Map(fnsB.map((fn) => [fn.path ?? "", fn]));
  const changed: string[] = [];
  for (const path of pathsA) {
    if (!path || !pathsB.has(path)) continue;
    const fa = aByPath.get(path);
    const fb = bByPath.get(path);
    if (!fa || !fb) continue;
    const argsDiff =
      JSON.stringify(fa.argsSchema ?? null) !==
      JSON.stringify(fb.argsSchema ?? null);
    const returnsDiff =
      JSON.stringify(fa.returnsSchema ?? null) !==
      JSON.stringify(fb.returnsSchema ?? null);
    if (argsDiff || returnsDiff || fa.kind !== fb.kind) changed.push(path);
  }
  changed.sort();
  return (
    <div
      className="mt-4 rounded-md border border-app bg-surface p-3"
      data-testid="settings-deploys-diff"
    >
      <header className="mb-2 flex items-baseline justify-between">
        <h3 className="text-xs uppercase tracking-[0.14em] text-muted">
          Diff: {shortId(a?.sha256 ?? "")} → {shortId(b?.sha256 ?? "")}
        </h3>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close diff"
          className="rounded border border-app bg-surface px-2 py-1 font-mono text-xs uppercase tracking-[0.14em] hover:border-strong"
        >
          Close
        </button>
      </header>
      <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
        <DiffColumn
          title="Added"
          tone="success"
          items={added}
          testid="settings-deploys-diff-added"
        />
        <DiffColumn
          title="Changed"
          tone="warning"
          items={changed}
          testid="settings-deploys-diff-changed"
        />
        <DiffColumn
          title="Removed"
          tone="danger"
          items={removed}
          testid="settings-deploys-diff-removed"
        />
      </div>
    </div>
  );
}

function DiffColumn({
  title,
  tone,
  items,
  testid,
}: {
  title: string;
  tone: "success" | "warning" | "danger";
  items: string[];
  testid: string;
}) {
  const toneClass =
    tone === "success"
      ? "text-success"
      : tone === "warning"
        ? "text-warning"
        : "text-danger";
  return (
    <div data-testid={testid}>
      <h4 className={`mb-1 text-xs uppercase tracking-[0.14em] ${toneClass}`}>
        {title} ({items.length})
      </h4>
      {items.length === 0 ? (
        <p className="text-xs text-muted">—</p>
      ) : (
        <ul className="space-y-0.5">
          {items.map((path) => (
            <li key={path} className="font-mono text-xs">
              {path}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function DangerZoneSection() {
  const [rotateOpen, setRotateOpen] = useState(false);
  const [shutdownOpen, setShutdownOpen] = useState(false);
  return (
    <SectionCard
      title="Session lifecycle"
      testid="settings-danger-zone"
      description="Rotate the local admin token, or shut down the running server. Both actions invalidate the current session."
      tone="danger"
    >
      <div className="flex flex-wrap items-center gap-3">
        <button
          type="button"
          data-testid="settings-rotate-open"
          onClick={() => setRotateOpen(true)}
          className="rounded border border-danger bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] text-danger hover:bg-surface-2"
        >
          Rotate admin token
        </button>
        <button
          type="button"
          data-testid="settings-shutdown-open"
          onClick={() => setShutdownOpen(true)}
          className="rounded border border-danger bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] text-danger hover:bg-surface-2"
        >
          Shut down server
        </button>
        <p className="text-xs text-muted">
          Token rotation requires pasting the current admin bearer. Shutdown
          uses the active session cookie.
        </p>
      </div>
      {rotateOpen ? (
        <RotateTokenDialog onClose={() => setRotateOpen(false)} />
      ) : null}
      {shutdownOpen ? (
        <ShutdownDialog onClose={() => setShutdownOpen(false)} />
      ) : null}
    </SectionCard>
  );
}

function RotateTokenDialog({ onClose }: { onClose: () => void }) {
  const [token, setToken] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<{ generation: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const tokenInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    tokenInputRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const submit = useCallback(async () => {
    if (!token.trim()) {
      setError("Paste the current admin bearer token to confirm rotation.");
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      const res = await fetch("/api/system/token/rotate", {
        method: "POST",
        credentials: "include",
        headers: {
          Authorization: `Bearer ${token.trim()}`,
        },
      });
      const body = (await res.json()) as {
        generation?: number;
        error?: string;
      };
      if (!res.ok) {
        setError(body.error ?? `Rotation failed (${res.status}).`);
        setSubmitting(false);
        return;
      }
      setResult({ generation: body.generation ?? 0 });
      toast.success("Admin token rotated", {
        description: `New generation ${body.generation}. All other sessions invalidated.`,
      });
    } catch (e) {
      setError(`Rotation failed: ${(e as Error).message}`);
    } finally {
      setSubmitting(false);
    }
  }, [token]);

  return (
    <DialogShell
      ref={dialogRef}
      title="Rotate admin token"
      onClose={onClose}
      testid="settings-rotate-dialog"
    >
      {result ? (
        <div className="space-y-3" data-testid="settings-rotate-result">
          <p className="text-sm text-default">
            New token issued (generation{" "}
            <span className="font-mono">{result.generation}</span>). Other
            sessions have been invalidated; this browser keeps its session until
            the next protected request.
          </p>
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-app bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] hover:border-strong"
          >
            Close
          </button>
        </div>
      ) : (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
          className="space-y-3"
        >
          <label
            htmlFor="settings-rotate-token"
            className="flex flex-col gap-1 text-xs text-muted"
          >
            <span>Current admin bearer</span>
            <input
              ref={tokenInputRef}
              id="settings-rotate-token"
              type="password"
              value={token}
              autoComplete="off"
              onChange={(e) => setToken(e.target.value)}
              data-testid="settings-rotate-token"
              className="rounded border border-app bg-surface px-2 py-1 font-mono text-xs text-default focus:border-strong focus:outline-none"
              placeholder="Paste the value of `nimbus token show`"
            />
          </label>
          {error ? (
            <p
              className="text-xs text-danger"
              data-testid="settings-rotate-error"
            >
              {error}
            </p>
          ) : null}
          <div className="flex items-center gap-2">
            <button
              type="submit"
              data-testid="settings-rotate-submit"
              disabled={submitting}
              className="rounded border border-danger bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] text-danger hover:bg-surface-2 disabled:cursor-not-allowed disabled:text-muted"
            >
              {submitting ? "Rotating…" : "Rotate"}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="rounded border border-app bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] hover:border-strong"
            >
              Cancel
            </button>
          </div>
        </form>
      )}
    </DialogShell>
  );
}

function ShutdownDialog({ onClose }: { onClose: () => void }) {
  const [submitting, setSubmitting] = useState(false);
  const [accepted, setAccepted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const submit = useCallback(async () => {
    setSubmitting(true);
    setError(null);
    try {
      const res = await fetch("/api/system/shutdown", {
        method: "POST",
        credentials: "include",
      });
      const body = (await res.json()) as {
        accepted?: boolean;
        error?: string;
      };
      if (!res.ok) {
        setError(body.error ?? `Shutdown failed (${res.status}).`);
        setSubmitting(false);
        return;
      }
      setAccepted(true);
      toast("Shutdown requested", {
        description:
          "Server will close listeners. The disconnect overlay will appear shortly.",
      });
    } catch (e) {
      setError(`Shutdown failed: ${(e as Error).message}`);
    } finally {
      setSubmitting(false);
    }
  }, []);

  return (
    <DialogShell
      ref={dialogRef}
      title="Shut down server"
      onClose={onClose}
      testid="settings-shutdown-dialog"
    >
      {accepted ? (
        <p
          className="text-sm text-default"
          data-testid="settings-shutdown-accepted"
        >
          Shutdown accepted. The WebSocket will drop and the disconnect overlay
          will take over the UI.
        </p>
      ) : (
        <div className="space-y-3">
          <p className="text-sm text-default">
            This will stop the running <code>nimbus start</code> process. All
            connected clients will disconnect. To restart, run{" "}
            <code>nimbus start</code> again from a terminal.
          </p>
          {error ? (
            <p
              className="text-xs text-danger"
              data-testid="settings-shutdown-error"
            >
              {error}
            </p>
          ) : null}
          <div className="flex items-center gap-2">
            <button
              type="button"
              data-testid="settings-shutdown-submit"
              onClick={() => void submit()}
              disabled={submitting}
              className="rounded border border-danger bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] text-danger hover:bg-surface-2 disabled:cursor-not-allowed disabled:text-muted"
            >
              {submitting ? "Stopping…" : "Confirm shutdown"}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="rounded border border-app bg-surface px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] hover:border-strong"
            >
              Cancel
            </button>
          </div>
        </div>
      )}
    </DialogShell>
  );
}

function DialogShell({
  ref,
  title,
  onClose,
  testid,
  children,
}: {
  ref: React.Ref<HTMLDivElement>;
  title: string;
  onClose: () => void;
  testid: string;
  children: React.ReactNode;
}) {
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    previouslyFocusedRef.current =
      (document.activeElement as HTMLElement | null) ?? null;
    return () => {
      previouslyFocusedRef.current?.focus?.();
    };
  }, []);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 px-4"
      data-testid={`${testid}-backdrop`}
    >
      <button
        type="button"
        aria-label="Close dialog"
        onClick={onClose}
        className="absolute inset-0 cursor-default"
      />
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        data-testid={testid}
        className="relative z-10 w-full max-w-md rounded-md border border-app bg-surface p-4 shadow-lg"
      >
        <header className="mb-3 flex items-baseline justify-between">
          <h2 className="text-sm text-default">{title}</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="Dismiss"
            className="font-mono text-xs text-muted hover:text-default"
          >
            ✕
          </button>
        </header>
        {children}
      </div>
    </div>
  );
}

function SectionCard({
  title,
  description,
  testid,
  tone,
  children,
}: {
  title: string;
  description?: string;
  testid: string;
  tone?: "default" | "danger";
  children: React.ReactNode;
}) {
  const borderClass = tone === "danger" ? "border-danger/40" : "border-app";
  return (
    <section
      data-testid={testid}
      className={`rounded-md border ${borderClass} bg-surface p-4`}
    >
      <header className="mb-3">
        <h2
          className="text-sm text-default"
          style={{ fontSize: "var(--text-base)" }}
        >
          {title}
        </h2>
        {description ? (
          <p className="text-xs text-muted">{description}</p>
        ) : null}
      </header>
      {children}
    </section>
  );
}

function DefinitionList({
  children,
  compact,
}: {
  children: React.ReactNode;
  compact?: boolean;
}) {
  return (
    <dl
      className={`grid grid-cols-1 gap-x-4 gap-y-2 sm:grid-cols-2 ${compact ? "" : "lg:grid-cols-3"}`}
    >
      {children}
    </dl>
  );
}

function Definition({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-[10px] uppercase tracking-[0.14em] text-muted">
        {label}
      </dt>
      <dd className="text-sm text-default">{children}</dd>
    </div>
  );
}

function Cell({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1 bg-surface px-3 py-2">
      <span className="text-[10px] uppercase tracking-[0.14em] text-muted">
        {label}
      </span>
      <span className="text-sm">{children}</span>
    </div>
  );
}
