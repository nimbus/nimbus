import { useMemo, useState } from "react";
import { CopyChip } from "../../../components/copy-chip";
import { StateChip } from "../../../components/state-chip";
import { RelativeTime } from "../../../components/time";
import { formatRelativeTime, shortId } from "../../../lib/format";
import { Definition, DefinitionList, SectionCard } from "./primitives";
import type { BundleDoc, FunctionDoc } from "./types";

export function DeploysSection({
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
