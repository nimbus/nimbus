import { createFileRoute, Link, useSearch } from "@tanstack/react-router";
import { useQuery } from "nimbus/react";
import { useEffect, useMemo, useRef, useState } from "react";

import { api } from "../../convex/_generated/api";
import { CopyChip } from "../components/copy-chip";
import { cn } from "../lib/cn";
import { shortId } from "../lib/format";

type RunnerSearch = {
  fn?: string;
  tenant?: string;
};

export const Route = createFileRoute("/compute_/runner")({
  validateSearch: (search: Record<string, unknown>): RunnerSearch => ({
    fn: typeof search.fn === "string" ? search.fn : undefined,
    tenant: typeof search.tenant === "string" ? search.tenant : undefined,
  }),
  component: RunnerPage,
});

type FunctionDoc = {
  _id: string;
  path?: string;
  kind?: string;
  adapter?: string;
  bundleId?: string;
  argsSchema?: unknown;
  returnsSchema?: unknown;
};

type RunResult =
  | { kind: "idle" }
  | { kind: "running"; startedAt: number }
  | {
      kind: "ok";
      data: unknown;
      durationMs: number;
      correlationId: string | null;
    }
  | {
      kind: "error";
      code: string | null;
      message: string;
      remediation: string | null;
      requestId: string | null;
      durationMs: number;
      raw: unknown;
    };

function RunnerPage() {
  const search = useSearch({ from: "/compute_/runner" });
  const navigate = Route.useNavigate();

  const functions = useQuery(api.functions.list, {
    bundleId: null,
    kind: null,
    limit: 500,
  }) as FunctionDoc[] | undefined;

  const tenants = useTenantList();

  const selectedFn = useMemo<FunctionDoc | null>(() => {
    if (!functions || !search.fn) return null;
    return functions.find((f) => f.path === search.fn) ?? null;
  }, [functions, search.fn]);

  const inferredKind = (selectedFn?.kind ?? "").toLowerCase();
  const isQuery = inferredKind === "query";

  const [argsText, setArgsText] = useState("{}");
  const [parseError, setParseError] = useState<string | null>(null);
  const [result, setResult] = useState<RunResult>({ kind: "idle" });
  const lastSubmitRef = useRef<number>(0);

  // biome-ignore lint/correctness/useExhaustiveDependencies: reset state when the selected function or tenant changes
  useEffect(() => {
    setArgsText("{}");
    setResult({ kind: "idle" });
    setParseError(null);
  }, [search.fn, search.tenant]);

  const parsedArgs = useMemo(() => {
    const trimmed = argsText.trim();
    if (trimmed === "") return { ok: true as const, value: {} };
    try {
      const value = JSON.parse(trimmed);
      if (value === null || typeof value !== "object" || Array.isArray(value)) {
        return {
          ok: false as const,
          error: "Arguments must be a JSON object.",
        };
      }
      return { ok: true as const, value };
    } catch (err) {
      return {
        ok: false as const,
        error: err instanceof Error ? err.message : "Invalid JSON.",
      };
    }
  }, [argsText]);

  const canSubmit =
    !!selectedFn?.path &&
    !!search.tenant &&
    parsedArgs.ok &&
    result.kind !== "running" &&
    (inferredKind === "query" ||
      inferredKind === "mutation" ||
      inferredKind === "action");

  const onSubmit = async () => {
    if (!selectedFn?.path || !search.tenant) return;
    if (!parsedArgs.ok) {
      setParseError(parsedArgs.error);
      return;
    }
    setParseError(null);
    const submitId = Date.now();
    lastSubmitRef.current = submitId;
    setResult({ kind: "running", startedAt: submitId });

    const endpoint = `/convex/${encodeURIComponent(
      search.tenant,
    )}/${inferredKind}`;
    try {
      const response = await fetch(endpoint, {
        method: "POST",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          name: selectedFn.path,
          args: parsedArgs.value,
        }),
      });
      if (lastSubmitRef.current !== submitId) return;
      const durationMs = Date.now() - submitId;
      const correlationId = response.headers.get("x-nimbus-correlation-id");
      let body: unknown = null;
      try {
        body = await response.json();
      } catch {
        body = null;
      }
      if (!response.ok || (isErrorEnvelope(body) && body.error)) {
        const env = isErrorEnvelope(body) ? body.error : null;
        setResult({
          kind: "error",
          code: env?.code ?? null,
          message: env?.message ?? `Request failed with ${response.status}`,
          remediation: env?.remediation?.message ?? null,
          requestId: env?.requestId ?? correlationId ?? null,
          durationMs,
          raw: body,
        });
        return;
      }
      setResult({
        kind: "ok",
        data: body,
        durationMs,
        correlationId,
      });
    } catch (err) {
      if (lastSubmitRef.current !== submitId) return;
      setResult({
        kind: "error",
        code: "network.error",
        message: err instanceof Error ? err.message : String(err),
        remediation: "Confirm the server is reachable and retry.",
        requestId: null,
        durationMs: Date.now() - submitId,
        raw: null,
      });
    }
  };

  return (
    <section
      className="flex h-full flex-col gap-4 overflow-hidden px-6 py-5"
      data-testid="page-runner"
    >
      <header className="flex items-baseline justify-between">
        <div>
          <h1
            className="text-xl text-default"
            style={{ fontSize: "var(--text-xl)" }}
          >
            Function runner
          </h1>
          <p className="text-sm text-muted">
            Invoke registered functions against any tenant. Identity is{" "}
            <span className="font-mono text-xs text-default">admin-local</span>—
            same-origin browser session; no auth provider configured.
          </p>
        </div>
        <Link
          to="/compute"
          className="font-mono text-[11px] uppercase tracking-wide text-muted hover:text-default"
          data-testid="runner-back-to-compute"
        >
          ← compute
        </Link>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[320px_1fr] gap-4 overflow-hidden">
        <FunctionPicker
          functions={functions}
          selectedPath={search.fn ?? null}
          onSelect={(path) =>
            navigate({
              search: (prev) => ({ ...prev, fn: path }),
              replace: true,
            })
          }
        />

        <div className="flex min-h-0 flex-col gap-4 overflow-auto rounded-md border border-app bg-surface p-4">
          {selectedFn ? (
            <>
              <FunctionHeader fn={selectedFn} />
              <TenantSelect
                tenants={tenants}
                value={search.tenant ?? null}
                onChange={(tenant) =>
                  navigate({
                    search: (prev) => ({ ...prev, tenant }),
                    replace: true,
                  })
                }
              />
              <ArgsEditor
                value={argsText}
                onChange={setArgsText}
                parseError={
                  parseError ?? (parsedArgs.ok ? null : parsedArgs.error)
                }
                schema={selectedFn.argsSchema}
              />
              <div className="flex items-center gap-3">
                <button
                  type="button"
                  onClick={onSubmit}
                  disabled={!canSubmit}
                  data-testid="runner-submit"
                  className={cn(
                    "rounded-md border px-3 py-1.5 font-mono text-xs uppercase tracking-wide",
                    canSubmit
                      ? "border-strong bg-surface-2 text-default hover:bg-surface"
                      : "border-app bg-surface-2 text-muted",
                  )}
                >
                  {result.kind === "running"
                    ? "Running…"
                    : `Run ${inferredKind || "function"}`}
                </button>
                {isQuery ? (
                  <span className="font-mono text-[10px] uppercase tracking-wide text-muted">
                    queries are read-only — submit re-runs once
                  </span>
                ) : null}
              </div>
              <ResultPanel result={result} />
            </>
          ) : (
            <EmptyPicker functions={functions} />
          )}
        </div>
      </div>
    </section>
  );
}

function isErrorEnvelope(value: unknown): value is {
  error: {
    code?: string;
    message?: string;
    requestId?: string;
    remediation?: { message?: string };
  };
} {
  return (
    typeof value === "object" &&
    value !== null &&
    "error" in value &&
    typeof (value as { error: unknown }).error === "object"
  );
}

function useTenantList(): {
  loading: boolean;
  tenants: string[];
  error: string | null;
} {
  const [tenants, setTenants] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    fetch("/api/tenants", { credentials: "include" })
      .then(async (response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const body = await response.json();
        const list = Array.isArray(body?.tenants)
          ? body.tenants.filter(
              (t: unknown): t is string => typeof t === "string",
            )
          : [];
        if (!cancelled) {
          setTenants(list);
          setError(null);
        }
      })
      .catch((err) => {
        if (!cancelled)
          setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return { loading, tenants, error };
}

function FunctionPicker({
  functions,
  selectedPath,
  onSelect,
}: {
  functions: FunctionDoc[] | undefined;
  selectedPath: string | null;
  onSelect: (path: string) => void;
}) {
  const [filter, setFilter] = useState("");
  const filtered = useMemo(() => {
    if (!functions) return undefined;
    const needle = filter.trim().toLowerCase();
    if (!needle) return functions;
    return functions.filter(
      (f) =>
        (f.path ?? "").toLowerCase().includes(needle) ||
        (f.kind ?? "").toLowerCase().includes(needle),
    );
  }, [functions, filter]);

  return (
    <aside
      className="flex min-h-0 flex-col gap-2 overflow-hidden rounded-md border border-app bg-surface"
      data-testid="runner-picker"
    >
      <div className="border-b border-app px-3 py-2">
        <label className="flex items-center gap-2">
          <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted">
            function
          </span>
          <input
            type="search"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="path or kind"
            data-inline-search
            data-testid="runner-picker-filter"
            className="w-full rounded border border-app bg-surface-2 px-2 py-1 font-mono text-xs text-default placeholder:text-muted/70"
          />
        </label>
      </div>
      <div
        className="min-h-0 flex-1 overflow-auto"
        data-testid="runner-picker-list"
      >
        {filtered === undefined ? (
          <div className="px-3 py-2 text-xs text-muted">Loading functions…</div>
        ) : filtered.length === 0 ? (
          <div className="px-3 py-3 text-xs text-muted">
            {functions && functions.length === 0
              ? "No functions registered."
              : "No functions match the filter."}
          </div>
        ) : (
          <ul className="flex flex-col">
            {filtered.map((fn) => {
              const isActive = selectedPath === fn.path;
              return (
                <li key={fn._id}>
                  <button
                    type="button"
                    onClick={() => fn.path && onSelect(fn.path)}
                    data-testid={`runner-picker-fn-${fn.path ?? fn._id}`}
                    aria-current={isActive ? "true" : undefined}
                    className={cn(
                      "flex w-full flex-col items-start gap-0.5 border-b border-app px-3 py-2 text-left",
                      isActive
                        ? "bg-surface-2 text-default"
                        : "text-muted hover:bg-surface-2 hover:text-default",
                    )}
                  >
                    <span className="font-mono text-xs text-default">
                      {fn.path ?? "(unnamed)"}
                    </span>
                    <span className="font-mono text-[10px] uppercase tracking-wide text-muted">
                      {fn.kind ?? "unknown"}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </aside>
  );
}

function FunctionHeader({ fn }: { fn: FunctionDoc }) {
  const kind = (fn.kind ?? "").toLowerCase();
  const adapter = fn.adapter ?? inferAdapter(kind);
  return (
    <div
      className="flex flex-wrap items-baseline gap-3 border-b border-app pb-3"
      data-testid="runner-fn-header"
    >
      <span className="font-mono text-sm text-default">{fn.path ?? "—"}</span>
      <span className="rounded border border-app px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted">
        {kind || "unknown"}
      </span>
      <span className="rounded border border-app px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted">
        {adapter}
      </span>
      {fn.bundleId ? (
        <CopyChip
          label="bundle id"
          value={fn.bundleId}
          testid="runner-fn-bundle"
        >
          {shortId(fn.bundleId, 12)}
        </CopyChip>
      ) : null}
    </div>
  );
}

function inferAdapter(kind: string): string {
  if (kind === "query" || kind === "mutation" || kind === "action") {
    return "convex";
  }
  if (kind === "http" || kind === "route") return "http";
  return "native";
}

function TenantSelect({
  tenants,
  value,
  onChange,
}: {
  tenants: { loading: boolean; tenants: string[]; error: string | null };
  value: string | null;
  onChange: (tenant: string) => void;
}) {
  return (
    <div className="flex flex-col gap-1" data-testid="runner-tenant">
      <label
        htmlFor="runner-tenant-select"
        className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted"
      >
        tenant
      </label>
      {tenants.error ? (
        <span
          id="runner-tenant-select"
          className="rounded border border-app bg-surface-2 px-2 py-1 font-mono text-xs text-danger"
          data-testid="runner-tenant-error"
        >
          Failed to load tenants: {tenants.error}
        </span>
      ) : (
        <select
          id="runner-tenant-select"
          value={value ?? ""}
          onChange={(e) => onChange(e.target.value)}
          disabled={tenants.loading}
          data-testid="runner-tenant-select"
          className="w-72 rounded border border-app bg-surface-2 px-2 py-1 font-mono text-xs text-default"
        >
          <option value="" disabled>
            {tenants.loading ? "loading…" : "select tenant"}
          </option>
          {tenants.tenants.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}

function ArgsEditor({
  value,
  onChange,
  parseError,
  schema,
}: {
  value: string;
  onChange: (next: string) => void;
  parseError: string | null;
  schema: unknown;
}) {
  const hasSchema = schema !== undefined && schema !== null;
  return (
    <label className="flex flex-col gap-1" data-testid="runner-args">
      <span className="flex items-center justify-between font-mono text-[10px] uppercase tracking-[0.14em] text-muted">
        <span>arguments (json object)</span>
        <span>
          {hasSchema
            ? "schema-aware editor: raw json fallback"
            : "raw json — no argsSchema published"}
        </span>
      </span>
      <textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        rows={8}
        spellCheck={false}
        data-testid="runner-args-input"
        className="rounded border border-app bg-surface-2 px-2 py-1 font-mono text-xs text-default placeholder:text-muted/70"
        placeholder='{ "key": "value" }'
      />
      {parseError ? (
        <span
          className="font-mono text-[11px] text-danger"
          data-testid="runner-args-error"
        >
          {parseError}
        </span>
      ) : null}
    </label>
  );
}

function ResultPanel({ result }: { result: RunResult }) {
  if (result.kind === "idle") {
    return (
      <div
        className="rounded border border-app bg-surface-2 px-3 py-2 font-mono text-[11px] text-muted"
        data-testid="runner-result-idle"
      >
        No result yet — submit to invoke.
      </div>
    );
  }
  if (result.kind === "running") {
    return (
      <div
        className="rounded border border-app bg-surface-2 px-3 py-2 font-mono text-[11px] text-muted"
        data-testid="runner-result-running"
      >
        Running…
      </div>
    );
  }
  if (result.kind === "error") {
    return (
      <div
        className="flex flex-col gap-2 rounded border border-app bg-surface-2 px-3 py-3"
        data-testid="runner-result-error"
      >
        <div className="flex items-center gap-2">
          <span className="rounded border border-app px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-danger">
            error
          </span>
          {result.code ? (
            <span className="font-mono text-[11px] uppercase tracking-wide text-muted">
              {result.code}
            </span>
          ) : null}
          <span className="tabular text-[11px] text-muted">
            {result.durationMs}ms
          </span>
        </div>
        <p className="font-mono text-xs text-default">{result.message}</p>
        {result.remediation ? (
          <p className="text-xs text-muted">{result.remediation}</p>
        ) : null}
        {result.requestId ? (
          <CopyChip
            label="request id"
            value={result.requestId}
            testid="runner-result-error-correlation"
          >
            {shortId(result.requestId, 16)}
          </CopyChip>
        ) : null}
      </div>
    );
  }
  return (
    <div
      className="flex flex-col gap-2 rounded border border-app bg-surface-2 px-3 py-3"
      data-testid="runner-result-ok"
    >
      <div className="flex items-center gap-2">
        <span className="rounded border border-app px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-success">
          ok
        </span>
        <span className="tabular text-[11px] text-muted">
          {result.durationMs}ms
        </span>
        {result.correlationId ? (
          <CopyChip
            label="correlation id"
            value={result.correlationId}
            testid="runner-result-ok-correlation"
          >
            {shortId(result.correlationId, 16)}
          </CopyChip>
        ) : null}
      </div>
      <pre
        className="overflow-auto rounded border border-app bg-surface p-2 font-mono text-[11px] text-default"
        data-testid="runner-result-json"
      >
        {JSON.stringify(result.data, null, 2)}
      </pre>
    </div>
  );
}

function EmptyPicker({ functions }: { functions: FunctionDoc[] | undefined }) {
  return (
    <div
      className="flex h-full flex-col items-center justify-center gap-2 text-center"
      data-testid="runner-empty"
    >
      <span className="font-mono text-sm text-default">
        {functions === undefined
          ? "Loading functions…"
          : functions.length === 0
            ? "No functions registered"
            : "Pick a function from the list"}
      </span>
      <span className="max-w-md text-xs text-muted">
        {functions === undefined
          ? "Streaming the live function inventory from _nimbus."
          : functions.length === 0
            ? "Deploy a Convex, Nimbus, or Cloud Functions app to populate the inventory. The runner targets registered function paths against a chosen tenant."
            : "Select a row on the left to view its kind, adapter, and schema, then enter arguments and submit."}
      </span>
    </div>
  );
}
