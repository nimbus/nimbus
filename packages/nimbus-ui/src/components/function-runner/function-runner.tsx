import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { CopyChip } from "../copy-chip";
import { cn } from "../../lib/cn";
import { shortId } from "../../lib/format";
import { useUiStore } from "../../store/ui-store";

export type FunctionRunnerFn = {
  _id: string;
  path?: string;
  kind?: string;
  adapter?: string;
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

export function FunctionRunner({ fn }: { fn: FunctionRunnerFn }) {
  const activeTenant = useUiStore((s) => s.activeTenant);
  const [open, setOpen] = useState(false);
  const [argsText, setArgsText] = useState("{}");
  const [tenantOverride, setTenantOverride] = useState<string | null>(null);
  const [parseError, setParseError] = useState<string | null>(null);
  const [result, setResult] = useState<RunResult>({ kind: "idle" });
  const lastSubmitRef = useRef<number>(0);

  const tenantList = useTenantList();
  const tenant = tenantOverride ?? activeTenant ?? null;

  useEffect(() => {
    setArgsText("{}");
    setResult({ kind: "idle" });
    setParseError(null);
  }, [fn._id]);

  const inferredKind = (fn.kind ?? "").toLowerCase();
  const isQuery = inferredKind === "query";

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
    !!fn.path &&
    !!tenant &&
    parsedArgs.ok &&
    result.kind !== "running" &&
    (inferredKind === "query" ||
      inferredKind === "mutation" ||
      inferredKind === "action");

  const onSubmit = async () => {
    if (!fn.path || !tenant) return;
    if (!parsedArgs.ok) {
      setParseError(parsedArgs.error);
      return;
    }
    setParseError(null);
    const submitId = Date.now();
    lastSubmitRef.current = submitId;
    setResult({ kind: "running", startedAt: submitId });

    const endpoint = `/convex/${encodeURIComponent(tenant)}/${inferredKind}`;
    try {
      const response = await fetch(endpoint, {
        method: "POST",
        credentials: "include",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          name: fn.path,
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
    <div
      className="shrink-0 border-t border-app bg-surface"
      data-testid="function-runner"
      data-open={open ? "true" : "false"}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        data-testid="function-runner-toggle"
        className="flex w-full items-center gap-2 px-6 py-2 text-left font-mono text-xs uppercase tracking-wide text-muted hover:text-default"
      >
        {open ? (
          <ChevronDown size={12} aria-hidden />
        ) : (
          <ChevronRight size={12} aria-hidden />
        )}
        <span>Runner</span>
        <span className="text-[10px] text-muted">
          {open ? "click to collapse" : "click to invoke this function"}
        </span>
        <span className="ml-auto">
          {result.kind === "running" ? (
            <span className="rounded border border-app px-1.5 py-0.5 text-[10px] uppercase text-muted">
              running…
            </span>
          ) : result.kind === "ok" ? (
            <span className="rounded border border-app px-1.5 py-0.5 text-[10px] uppercase text-success">
              ok · {result.durationMs}ms
            </span>
          ) : result.kind === "error" ? (
            <span className="rounded border border-app px-1.5 py-0.5 text-[10px] uppercase text-danger">
              error
            </span>
          ) : null}
        </span>
      </button>
      {open ? (
        <div className="grid grid-cols-[1fr_1fr] gap-3 border-t border-app px-6 py-3">
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2">
              <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted">
                tenant
              </span>
              {tenantList.error ? (
                <span className="font-mono text-[11px] text-danger">
                  failed: {tenantList.error}
                </span>
              ) : (
                <select
                  value={tenant ?? ""}
                  onChange={(e) => setTenantOverride(e.target.value || null)}
                  disabled={tenantList.loading}
                  data-testid="function-runner-tenant"
                  className="rounded border border-app bg-surface-2 px-2 py-1 font-mono text-xs text-default"
                >
                  <option value="" disabled>
                    {tenantList.loading ? "loading…" : "select tenant"}
                  </option>
                  {tenantList.tenants.map((t) => (
                    <option key={t} value={t}>
                      {t}
                    </option>
                  ))}
                </select>
              )}
            </div>
            <label className="flex flex-col gap-1" data-testid="function-runner-args">
              <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted">
                arguments (json object)
              </span>
              <textarea
                value={argsText}
                onChange={(e) => setArgsText(e.target.value)}
                rows={6}
                spellCheck={false}
                data-testid="function-runner-args-input"
                className="rounded border border-app bg-surface-2 px-2 py-1 font-mono text-xs text-default placeholder:text-muted/70"
                placeholder='{ "key": "value" }'
              />
              {parseError ?? (!parsedArgs.ok && parsedArgs.error) ? (
                <span className="font-mono text-[11px] text-danger">
                  {parseError ?? (parsedArgs.ok ? null : parsedArgs.error)}
                </span>
              ) : null}
            </label>
            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={onSubmit}
                disabled={!canSubmit}
                data-testid="function-runner-submit"
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
                  queries are read-only
                </span>
              ) : null}
            </div>
          </div>
          <ResultPanel result={result} />
        </div>
      ) : null}
    </div>
  );
}

function ResultPanel({ result }: { result: RunResult }) {
  if (result.kind === "idle") {
    return (
      <div
        className="rounded border border-app bg-surface-2 px-3 py-2 font-mono text-[11px] text-muted"
        data-testid="function-runner-result-idle"
      >
        No result yet — submit to invoke.
      </div>
    );
  }
  if (result.kind === "running") {
    return (
      <div
        className="rounded border border-app bg-surface-2 px-3 py-2 font-mono text-[11px] text-muted"
        data-testid="function-runner-result-running"
      >
        Running…
      </div>
    );
  }
  if (result.kind === "error") {
    return (
      <div
        className="flex flex-col gap-2 rounded border border-app bg-surface-2 px-3 py-3"
        data-testid="function-runner-result-error"
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
            testid="function-runner-result-error-correlation"
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
      data-testid="function-runner-result-ok"
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
            testid="function-runner-result-ok-correlation"
          >
            {shortId(result.correlationId, 16)}
          </CopyChip>
        ) : null}
      </div>
      <pre
        className="max-h-48 overflow-auto rounded border border-app bg-surface p-2 font-mono text-[11px] text-default"
        data-testid="function-runner-result-json"
      >
        {JSON.stringify(result.data, null, 2)}
      </pre>
    </div>
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
