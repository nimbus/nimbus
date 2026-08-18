import { useMemo } from "react";
import { PageSection } from "./-primitives";
import type { AdapterCapabilityDoc } from "./-types";

const ADAPTERS = [
  { id: "convex", label: "Convex" },
  { id: "mongodb", label: "MongoDB" },
  { id: "firebase", label: "Firebase" },
  { id: "cloud_functions", label: "Cloud Functions" },
  { id: "native", label: "Native" },
] as const;

export function IntegrationsSection({
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
    <PageSection
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
                  <span className="font-mono text-xs uppercase tracking-[0.14em] text-muted">
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
                          <p className="text-xs text-warning">⚠ {f.caveat}</p>
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
    </PageSection>
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
      className={`font-mono text-xs uppercase tracking-[0.14em] ${colorClass}`}
    >
      {status}
    </span>
  );
}
