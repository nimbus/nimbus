import { Plug, Unplug } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { Button } from "../../../components/ui/button";
import { sessions as sessionApi } from "../../../lib/api-mutations";
import {
  readSessionChannel,
  type SessionChannelFrame,
  sessionChannelStreamPath,
} from "../../../lib/session-channel";

/** The channel a sandbox session streams; sandboxes admit `stdio` and `files`. */
export const CONSOLE_CHANNEL = "stdio";

/** How long the console asks the session to live; the server may shorten it. */
export const CONSOLE_SESSION_TTL_MS = 30 * 60 * 1000;

export type ConsoleLine = {
  seq: number;
  kind: SessionChannelFrame["kind"] | "input" | "error";
  text: string;
};

export type ConsoleConnection =
  | { kind: "idle" }
  | { kind: "opening" }
  | { kind: "attached"; sessionId: string; live: boolean }
  | { kind: "ended"; sessionId: string | null; reason: string }
  | { kind: "error"; message: string };

function lineOfFrame(frame: SessionChannelFrame): ConsoleLine["kind"] {
  return frame.kind;
}

function textOfFrame(frame: SessionChannelFrame): string {
  switch (frame.kind) {
    case "opened":
      return `attached to ${frame.channel} at generation ${frame.targetGeneration}`;
    case "stdout":
    case "stderr":
      return frame.data;
    case "exit":
      return `process exited with code ${frame.code}`;
    case "closed":
      return `stream closed: ${frame.reason}`;
  }
}

// The console of one sandbox: its stdout and stderr as they arrive, and an
// input line that writes to its stdin. One session per mounted panel: the
// panel opens it with the `stdio` channel, streams the channel to its end,
// and closes the session when it unmounts or the operator disconnects.
// The line list is append-only in arrival order; nothing is reordered or
// merged, so what the operator reads is what the channel sent.
export function SandboxConsole({
  tenant,
  sandboxId,
  lifecycleState,
  testid,
}: {
  tenant: string;
  sandboxId: string;
  lifecycleState: string;
  testid: string;
}) {
  const [connection, setConnection] = useState<ConsoleConnection>({
    kind: "idle",
  });
  const [lines, setLines] = useState<ConsoleLine[]>([]);
  const [input, setInput] = useState("");
  const [inputError, setInputError] = useState<string | null>(null);
  const seq = useRef(0);
  const abort = useRef<AbortController | null>(null);
  const sessionRef = useRef<string | null>(null);
  const logRef = useRef<HTMLOListElement | null>(null);

  const append = useCallback((kind: ConsoleLine["kind"], text: string) => {
    seq.current += 1;
    const line = { seq: seq.current, kind, text };
    setLines((prev) => [...prev, line]);
  }, []);

  const disconnect = useCallback(
    (reason: string) => {
      abort.current?.abort();
      abort.current = null;
      const id = sessionRef.current;
      sessionRef.current = null;
      if (id) void sessionApi.close(id, tenant, reason);
      setConnection((prev) =>
        prev.kind === "attached" || prev.kind === "opening"
          ? { kind: "ended", sessionId: id, reason }
          : prev,
      );
    },
    [tenant],
  );

  const connect = useCallback(async () => {
    abort.current?.abort();
    const controller = new AbortController();
    abort.current = controller;
    setConnection({ kind: "opening" });
    setInputError(null);
    const opened = await sessionApi.open({
      tenantId: tenant,
      target: { sandbox: { id: sandboxId } },
      channels: [CONSOLE_CHANNEL],
      requestedTtlMs: CONSOLE_SESSION_TTL_MS,
    });
    if (controller.signal.aborted) return;
    if (!opened.ok) {
      setConnection({ kind: "error", message: opened.error });
      append("error", `session refused: ${opened.error}`);
      return;
    }
    const sessionId = opened.data.metadata.id;
    sessionRef.current = sessionId;
    setConnection({ kind: "attached", sessionId, live: false });
    const end = await readSessionChannel(
      sessionChannelStreamPath(sessionId, CONSOLE_CHANNEL, tenant),
      (frame) => {
        append(lineOfFrame(frame), textOfFrame(frame));
        if (frame.kind === "opened") {
          setConnection({ kind: "attached", sessionId, live: true });
        } else if (frame.kind === "closed") {
          setConnection({ kind: "ended", sessionId, reason: frame.reason });
        }
      },
      controller.signal,
    );
    if (controller.signal.aborted) return;
    if (!end.ok) {
      setConnection({ kind: "error", message: end.error });
      append("error", `stream failed: ${end.error}`);
      return;
    }
    // The stream ran out without a closed frame (the server always sends
    // one; a proxy may cut the body short).
    setConnection((prev) =>
      prev.kind === "attached"
        ? { kind: "ended", sessionId, reason: "stream ended" }
        : prev,
    );
  }, [tenant, sandboxId, append]);

  // Attach on mount when the sandbox can take a session; otherwise wait for
  // the operator, who sees why. Close whatever is open on unmount. The
  // panel attaches once per sandbox: a later state change is shown, not
  // acted on, so a stopping sandbox keeps its last lines on screen.
  const attachOnMount = useRef({ lifecycleState, connect, disconnect });
  attachOnMount.current = { lifecycleState, connect, disconnect };
  useEffect(() => {
    const initial = attachOnMount.current;
    if (initial.lifecycleState === "ready") void initial.connect();
    return () => attachOnMount.current.disconnect("console closed");
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll on every new line
  useEffect(() => {
    const log = logRef.current;
    if (log) log.scrollTop = log.scrollHeight;
  }, [lines]);

  const canWrite = connection.kind === "attached" && connection.live;

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!canWrite || connection.kind !== "attached") return;
    const data = input;
    setInput("");
    setInputError(null);
    append("input", data);
    const result = await sessionApi.writeChannel(
      connection.sessionId,
      CONSOLE_CHANNEL,
      tenant,
      `${data}\n`,
    );
    if (!result.ok) {
      setInputError(result.error);
      append("error", `input refused: ${result.error}`);
    }
  };

  return (
    <div
      className="flex h-full min-h-0 flex-col overflow-hidden"
      data-testid={testid}
      data-connection={connection.kind}
    >
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-border-2 px-6 py-2">
        <span className="text-xs text-text-3" data-testid={`${testid}-status`}>
          <ConnectionText
            connection={connection}
            lifecycleState={lifecycleState}
          />
        </span>
        {connection.kind === "attached" || connection.kind === "opening" ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => disconnect("operator disconnected")}
            data-testid={`${testid}-disconnect`}
          >
            <Unplug className="size-3.5" aria-hidden /> Disconnect
          </Button>
        ) : (
          <Button
            type="button"
            size="sm"
            onClick={() => void connect()}
            data-testid={`${testid}-connect`}
          >
            <Plug className="size-3.5" aria-hidden /> Connect
          </Button>
        )}
      </div>
      <ol
        ref={logRef}
        className="min-h-0 flex-1 overflow-auto bg-bg-base px-6 py-3 font-mono text-xs"
        aria-label="Sandbox console output"
        aria-live="polite"
        data-testid={`${testid}-log`}
      >
        {lines.length === 0 ? (
          <li className="text-text-3" data-testid={`${testid}-empty`}>
            {connection.kind === "idle"
              ? "Nothing received yet. Connect to attach to the sandbox's stdio."
              : "Waiting for the first bytes…"}
          </li>
        ) : null}
        {lines.map((line) => (
          <li
            key={line.seq}
            className={LINE_CLASS[line.kind]}
            data-kind={line.kind}
            data-testid={`${testid}-line`}
          >
            {line.kind === "input" ? "› " : ""}
            {line.text}
          </li>
        ))}
      </ol>
      <form
        onSubmit={(event) => void submit(event)}
        className="flex shrink-0 items-center gap-2 border-t border-border-2 px-6 py-2"
      >
        <span className="font-mono text-xs text-text-3" aria-hidden>
          ›
        </span>
        <input
          value={input}
          onChange={(event) => setInput(event.target.value)}
          disabled={!canWrite}
          placeholder={
            canWrite ? "Line to send to stdin" : "Connect to send input"
          }
          aria-label="Sandbox stdin"
          autoComplete="off"
          spellCheck={false}
          className="h-[26px] min-w-0 flex-1 rounded-xs border border-border-2 bg-bg-panel px-2 font-mono text-xs text-text-1 placeholder:text-text-3 focus-visible:border-accent-edge disabled:opacity-60"
          data-testid={`${testid}-input`}
        />
        <Button
          type="submit"
          size="sm"
          variant="outline"
          disabled={!canWrite}
          data-testid={`${testid}-send`}
        >
          Send
        </Button>
      </form>
      {inputError ? (
        <p
          className="shrink-0 px-6 pb-2 text-xs text-error"
          data-testid={`${testid}-input-error`}
        >
          {inputError}
        </p>
      ) : null}
    </div>
  );
}

function ConnectionText({
  connection,
  lifecycleState,
}: {
  connection: ConsoleConnection;
  lifecycleState: string;
}) {
  switch (connection.kind) {
    case "idle":
      return lifecycleState === "ready"
        ? "Not attached."
        : `The sandbox is ${lifecycleState}; a session attaches once it is ready.`;
    case "opening":
      return "Opening a session…";
    case "attached":
      return connection.live
        ? `Attached to stdio through session ${connection.sessionId}. Live.`
        : `Session ${connection.sessionId} open; attaching to stdio…`;
    case "ended":
      return `Stream ended: ${connection.reason}.`;
    case "error":
      return `Console error: ${connection.message}`;
  }
}

const LINE_CLASS: Record<ConsoleLine["kind"], string> = {
  opened: "text-text-3",
  stdout: "whitespace-pre-wrap text-text-1",
  stderr: "whitespace-pre-wrap text-warning",
  exit: "text-text-3",
  closed: "text-text-3",
  input: "whitespace-pre-wrap text-accent-edge",
  error: "text-error",
};
