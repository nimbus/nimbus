import { ChevronDown, ChevronRight } from "lucide-react";
import { type KeyboardEvent, useMemo, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Kbd, KbdGroup } from "@/components/ui/kbd";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import { shortId } from "../../lib/format";
import { metaGlyph } from "../../lib/platform";
import { useUiStore } from "../../store/ui-store";
import { CopyChip } from "../copy-chip";
import { CategoryPill, Pill } from "../pill";
import { SegmentedControl } from "../segmented-control";
import {
  type ArgField,
  buildArgs,
  type FieldValues,
  parseArgsValidator,
  valuesFromArgs,
} from "./args-validator";

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
      // Set for `function.thrown`: the function's own throw, with the
      // developer's stack and the path the server ran.
      functionPath: string | null;
      stack: string | null;
      durationMs: number;
      raw: unknown;
    };

// The server's class for a handler that threw (crates/nimbus-server
// error_envelope.rs). Every other code is a request, runtime, or service
// fault, and the card keeps the server's remediation copy for those.
const FUNCTION_THROWN = "function.thrown";

type ArgsMode = "form" | "json";

const MODE_OPTIONS = [
  { value: "form", label: "Form", description: "one field per argument" },
  { value: "json", label: "JSON", description: "the argument object as text" },
] as const;

const RUNNABLE_KINDS = new Set(["query", "mutation", "action"]);

// The runner is the bottom drawer of a function page. It invokes the
// function against the active tenant, the same tenant every list on the
// page reads from, so there is no second tenant chooser to get out of step.
// When the function's validator is known the arguments start as a form,
// one field per argument; JSON mode is always there for the shape the form
// cannot hold.
export function FunctionRunner({
  fn,
  onOpenRuns,
}: {
  fn: FunctionRunnerFn;
  // The page owns navigation; the runner stays router-free so it renders in
  // Storybook and in specs without a provider. When set, a thrown-error card
  // offers the function's Runs tab, where the run row keeps the same error.
  onOpenRuns?: () => void;
}) {
  const tenant = useUiStore((s) => s.activeTenant);
  const fields = useMemo(() => parseArgsValidator(fn.argsSchema), [fn]);
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<ArgsMode>(fields ? "form" : "json");
  const [values, setValues] = useState<FieldValues>(() =>
    fields ? valuesFromArgs(fields, {}) : {},
  );
  const [argsText, setArgsText] = useState("{}");
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({});
  const [parseError, setParseError] = useState<string | null>(null);
  const [result, setResult] = useState<RunResult>({ kind: "idle" });
  const lastSubmitRef = useRef(0);

  const kind = (fn.kind ?? "").toLowerCase();
  const runnable = RUNNABLE_KINDS.has(kind);

  const parsedJson = useMemo(() => parseArgsText(argsText), [argsText]);

  const canSubmit =
    !!fn.path &&
    !!tenant &&
    runnable &&
    result.kind !== "running" &&
    (mode === "form" || parsedJson.ok);

  const currentArgs = (): Record<string, unknown> | null => {
    if (mode === "form" && fields) {
      const built = buildArgs(fields, values);
      if (!built.ok) {
        setFieldErrors(built.errors);
        return null;
      }
      setFieldErrors({});
      return built.args;
    }
    if (!parsedJson.ok) {
      setParseError(parsedJson.error);
      return null;
    }
    setParseError(null);
    return parsedJson.value;
  };

  const switchMode = (next: ArgsMode) => {
    if (next === mode) return;
    if (next === "json" && fields) {
      const built = buildArgs(fields, values);
      setArgsText(
        JSON.stringify(
          built.ok ? built.args : looseArgs(fields, values),
          null,
          2,
        ),
      );
    } else if (next === "form" && fields && parsedJson.ok) {
      setValues(valuesFromArgs(fields, parsedJson.value));
    }
    setMode(next);
  };

  const onSubmit = async () => {
    if (!canSubmit || !fn.path || !tenant) return;
    const args = currentArgs();
    if (args === null) return;
    const submitId = Date.now();
    lastSubmitRef.current = submitId;
    setResult({ kind: "running", startedAt: submitId });
    const outcome = await invoke({ tenant, kind, name: fn.path, args });
    // A later submit owns the panel; drop the stale answer.
    if (lastSubmitRef.current !== submitId) return;
    setResult(outcome(Date.now() - submitId));
  };

  // ⌘⏎ (Ctrl+⏎ elsewhere) runs from anywhere in the drawer. A plain Enter
  // in a text field is swallowed: a mutation must not fire because the
  // operator meant to move to the next field.
  const onKeyDown = (event: KeyboardEvent<HTMLFormElement>) => {
    if (event.key !== "Enter") return;
    if (event.metaKey || event.ctrlKey) {
      event.preventDefault();
      void onSubmit();
      return;
    }
    if (event.target instanceof HTMLInputElement) event.preventDefault();
  };

  const setField = (name: string, value: string | boolean) => {
    setValues((prev) => ({ ...prev, [name]: value }));
    setFieldErrors((prev) => {
      if (!(name in prev)) return prev;
      const next = { ...prev };
      delete next[name];
      return next;
    });
  };

  return (
    <div
      className="shrink-0 border-t border-border-2 bg-bg-panel"
      data-testid="function-runner"
      data-open={open ? "true" : "false"}
    >
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        data-testid="function-runner-toggle"
        className="flex h-9 w-full items-center gap-2 px-6 text-left text-xs font-medium text-text-3 hover:text-text-1"
      >
        {open ? (
          <ChevronDown size={12} aria-hidden />
        ) : (
          <ChevronRight size={12} aria-hidden />
        )}
        <span className="text-text-1">Runner</span>
        <CategoryPill value={kind || fn.kind} />
        {fn.adapter ? <CategoryPill value={fn.adapter} /> : null}
        <span className="ml-auto flex items-center gap-2">
          <span
            className="font-mono text-xs text-text-3"
            data-testid="function-runner-target"
          >
            {tenant ? `tenant ${tenant}` : "no tenant"}
          </span>
          <StatusPill result={result} />
        </span>
      </button>
      {open ? (
        <form
          className="grid grid-cols-1 gap-4 border-t border-border-2 px-6 py-4 lg:grid-cols-2"
          onSubmit={(event) => {
            event.preventDefault();
            void onSubmit();
          }}
          onKeyDown={onKeyDown}
        >
          <div className="flex min-w-0 flex-col gap-3">
            <div className="flex items-center justify-between gap-3">
              <span className="text-xs font-medium text-text-3">Arguments</span>
              {fields ? (
                <SegmentedControl
                  label="Argument editor"
                  value={mode}
                  options={MODE_OPTIONS}
                  onChange={switchMode}
                  testid="function-runner-mode"
                />
              ) : null}
            </div>
            {mode === "form" && fields ? (
              <ArgsForm
                fields={fields}
                values={values}
                errors={fieldErrors}
                onChange={setField}
              />
            ) : (
              <div
                className="flex flex-col gap-1"
                data-testid="function-runner-args"
              >
                <label htmlFor="function-runner-args-input" className="sr-only">
                  Arguments as a JSON object
                </label>
                <Textarea
                  id="function-runner-args-input"
                  value={argsText}
                  onChange={(e) => {
                    setArgsText(e.target.value);
                    setParseError(null);
                  }}
                  rows={6}
                  spellCheck={false}
                  data-testid="function-runner-args-input"
                  aria-invalid={parseError ? true : undefined}
                  className="min-h-32 font-mono text-xs"
                  placeholder='{ "key": "value" }'
                />
                {(parseError ?? (!parsedJson.ok && argsText.trim() !== "")) ? (
                  <span className="text-xs text-error">
                    {parseError ?? (parsedJson.ok ? null : parsedJson.error)}
                  </span>
                ) : null}
              </div>
            )}
            <div className="flex items-center gap-3">
              <Button
                type="submit"
                size="sm"
                disabled={!canSubmit}
                data-testid="function-runner-submit"
              >
                {result.kind === "running" ? "Running…" : "Run function"}
                <KbdGroup aria-hidden className="ml-1">
                  <Kbd className="bg-black/10 text-current dark:bg-white/15">
                    {metaGlyph}
                  </Kbd>
                  <Kbd className="bg-black/10 text-current dark:bg-white/15">
                    ⏎
                  </Kbd>
                </KbdGroup>
              </Button>
              {!runnable ? (
                <span className="text-xs text-text-3">
                  Only a query, mutation, or action runs from here.
                </span>
              ) : !tenant ? (
                <span className="text-xs text-text-3">
                  Choose a tenant to run against.
                </span>
              ) : kind === "query" ? (
                <span className="text-xs text-text-3">
                  A query is read-only.
                </span>
              ) : null}
            </div>
          </div>
          <ResultPanel result={result} onOpenRuns={onOpenRuns} />
        </form>
      ) : null}
    </div>
  );
}

function ArgsForm({
  fields,
  values,
  errors,
  onChange,
}: {
  fields: ReadonlyArray<ArgField>;
  values: FieldValues;
  errors: Record<string, string>;
  onChange: (name: string, value: string | boolean) => void;
}) {
  if (fields.length === 0) {
    return (
      <p
        className="text-xs text-text-3"
        data-testid="function-runner-form-empty"
      >
        This function takes no arguments.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-2" data-testid="function-runner-form">
      {fields.map((field) => {
        const id = `function-runner-field-${field.name}`;
        const error = errors[field.name];
        const value = values[field.name];
        return (
          <div
            key={field.name}
            className="grid grid-cols-[minmax(0,12rem)_1fr] items-start gap-x-3 gap-y-1"
          >
            <label
              htmlFor={id}
              className="flex min-w-0 items-baseline gap-1.5 pt-1.5 font-mono text-xs"
            >
              <span className="truncate text-text-1">{field.name}</span>
              <span className="shrink-0 text-text-3">{field.type}</span>
            </label>
            <div className="flex min-w-0 flex-col gap-1">
              {field.input === "boolean" ? (
                <div className="flex h-8 items-center">
                  <Checkbox
                    id={id}
                    checked={value === true}
                    onCheckedChange={(checked) =>
                      onChange(field.name, checked === true)
                    }
                    data-testid={id}
                  />
                </div>
              ) : field.input === "json" ? (
                <Textarea
                  id={id}
                  value={typeof value === "string" ? value : ""}
                  onChange={(e) => onChange(field.name, e.target.value)}
                  rows={2}
                  spellCheck={false}
                  aria-invalid={error ? true : undefined}
                  className="min-h-8 font-mono text-xs"
                  placeholder={field.optional ? "omitted when empty" : "{}"}
                  data-testid={id}
                />
              ) : (
                <Input
                  id={id}
                  type="text"
                  inputMode={field.input === "number" ? "decimal" : undefined}
                  value={typeof value === "string" ? value : ""}
                  onChange={(e) => onChange(field.name, e.target.value)}
                  spellCheck={false}
                  aria-invalid={error ? true : undefined}
                  className={cn(
                    "text-xs",
                    field.input === "number" && "font-mono tabular",
                  )}
                  placeholder={field.optional ? "omitted when empty" : ""}
                  data-testid={id}
                />
              )}
              {error ? (
                <span
                  className="text-xs text-error"
                  data-testid={`${id}-error`}
                >
                  {error}
                </span>
              ) : null}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function StatusPill({ result }: { result: RunResult }) {
  if (result.kind === "running") return <Pill tone="neutral">running</Pill>;
  if (result.kind === "ok") {
    return (
      <Pill tone="success" className="tabular">
        ok · {result.durationMs}ms
      </Pill>
    );
  }
  if (result.kind === "error") return <Pill tone="error">error</Pill>;
  return null;
}

function ResultPanel({
  result,
  onOpenRuns,
}: {
  result: RunResult;
  onOpenRuns?: () => void;
}) {
  if (result.kind === "idle") {
    return (
      <div
        className="flex min-h-32 items-center justify-center rounded-lg border border-dashed border-border-2 px-3 py-2 text-xs text-text-3"
        data-testid="function-runner-result-idle"
      >
        The result shows here after a run.
      </div>
    );
  }
  if (result.kind === "running") {
    return (
      <div
        className="flex min-h-32 items-center justify-center rounded-lg border border-border-2 bg-bg-raised px-3 py-2 text-xs text-text-3"
        data-testid="function-runner-result-running"
      >
        Running…
      </div>
    );
  }
  if (result.kind === "error") {
    const thrown = result.code === FUNCTION_THROWN;
    return (
      <div
        className="flex flex-col gap-2 rounded-lg border border-border-2 bg-bg-raised px-3 py-3"
        data-testid="function-runner-result-error"
        data-error-class={thrown ? "function" : "service"}
      >
        <div className="flex items-center gap-2">
          <Pill tone="error">{thrown ? "threw" : "error"}</Pill>
          {result.code ? (
            <span className="font-mono text-xs text-text-3">{result.code}</span>
          ) : null}
          <span className="tabular font-mono text-xs text-text-3">
            {result.durationMs}ms
          </span>
        </div>
        {thrown ? (
          <p
            className="text-xs text-text-3"
            data-testid="function-runner-result-error-function"
          >
            <span className="font-mono text-text-1">
              {result.functionPath ?? "The function"}
            </span>{" "}
            threw. The message and the stack below are the function's own.
          </p>
        ) : null}
        <p
          className="font-mono text-xs text-text-1 whitespace-pre-wrap"
          data-testid="function-runner-result-error-message"
        >
          {result.message}
        </p>
        {thrown && result.stack ? (
          <details
            className="text-xs"
            data-testid="function-runner-result-error-stack"
          >
            <summary className="cursor-pointer text-text-3 hover:text-text-1">
              Stack
            </summary>
            <pre className="mt-1 max-h-48 overflow-auto font-mono text-xs text-text-3 whitespace-pre">
              {result.stack}
            </pre>
          </details>
        ) : null}
        {!thrown && result.remediation ? (
          <p className="text-xs text-text-3">{result.remediation}</p>
        ) : null}
        {thrown && onOpenRuns ? (
          <div>
            <Button
              variant="outline"
              size="sm"
              onClick={onOpenRuns}
              data-testid="function-runner-result-error-runs"
            >
              View runs
            </Button>
          </div>
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
      className="flex min-w-0 flex-col gap-2 rounded-lg border border-border-2 bg-bg-raised px-3 py-3"
      data-testid="function-runner-result-ok"
    >
      <div className="flex items-center gap-2">
        <Pill tone="success">ok</Pill>
        <span className="tabular font-mono text-xs text-text-3">
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
        className="max-h-48 overflow-auto rounded-md border border-border-2 bg-bg-panel p-2 font-mono text-xs text-text-1"
        data-testid="function-runner-result-json"
      >
        {JSON.stringify(result.data, null, 2)}
      </pre>
    </div>
  );
}

type ParsedArgs =
  | { ok: true; value: Record<string, unknown> }
  | { ok: false; error: string };

function parseArgsText(text: string): ParsedArgs {
  const trimmed = text.trim();
  if (trimmed === "") return { ok: true, value: {} };
  try {
    const value: unknown = JSON.parse(trimmed);
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
      return { ok: false, error: "Arguments must be a JSON object." };
    }
    return { ok: true, value: value as Record<string, unknown> };
  } catch (err) {
    return {
      ok: false,
      error: err instanceof Error ? err.message : "Invalid JSON.",
    };
  }
}

// looseArgs carries every field into JSON mode as typed, so a value the
// form rejected is still there to fix as text.
function looseArgs(
  fields: ReadonlyArray<ArgField>,
  values: FieldValues,
): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const field of fields) out[field.name] = values[field.name] ?? "";
  return out;
}

// invoke posts the call and returns a function of the elapsed time, so the
// caller stamps the duration only when it decides the answer still counts.
async function invoke({
  tenant,
  kind,
  name,
  args,
}: {
  tenant: string;
  kind: string;
  name: string;
  args: Record<string, unknown>;
}): Promise<(durationMs: number) => RunResult> {
  const endpoint = `/convex/${encodeURIComponent(tenant)}/${kind}`;
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      credentials: "include",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name, args }),
    });
    const correlationId = response.headers.get("x-nimbus-correlation-id");
    let body: unknown = null;
    try {
      body = await response.json();
    } catch {
      body = null;
    }
    if (!response.ok || (isErrorEnvelope(body) && body.error)) {
      const env = isErrorEnvelope(body) ? body.error : null;
      return (durationMs) => ({
        kind: "error",
        code: env?.code ?? null,
        message: env?.message ?? `Request failed with ${response.status}`,
        remediation: env?.remediation?.message ?? null,
        requestId: env?.requestId ?? correlationId ?? null,
        functionPath:
          typeof env?.detail?.functionPath === "string"
            ? env.detail.functionPath
            : null,
        stack: typeof env?.detail?.stack === "string" ? env.detail.stack : null,
        durationMs,
        raw: body,
      });
    }
    return (durationMs) => ({
      kind: "ok",
      data: body,
      durationMs,
      correlationId,
    });
  } catch (err) {
    return (durationMs) => ({
      kind: "error",
      code: "network.error",
      message: err instanceof Error ? err.message : String(err),
      remediation: "Confirm the server is reachable and retry.",
      requestId: null,
      functionPath: null,
      stack: null,
      durationMs,
      raw: null,
    });
  }
}

function isErrorEnvelope(value: unknown): value is {
  error: {
    code?: string;
    message?: string;
    requestId?: string;
    remediation?: { message?: string };
    detail?: { functionPath?: unknown; stack?: unknown };
  };
} {
  return (
    typeof value === "object" &&
    value !== null &&
    "error" in value &&
    typeof (value as { error: unknown }).error === "object"
  );
}
