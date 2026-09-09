import { apiErrorMessage } from "./api-mutations";

// One line of the session channel stream
// (`GET /api/sessions/{id}/channels/{channel}/stream`, `application/x-ndjson`).
// The server writes `opened` first and `closed` last; between them the
// bytes of the channel arrive as `stdout` and `stderr` text and an `exit`
// when the process ends.
export type SessionChannelFrame =
  | { kind: "opened"; channel: string; targetGeneration: number }
  | { kind: "stdout"; data: string }
  | { kind: "stderr"; data: string }
  | { kind: "exit"; code: number }
  | { kind: "closed"; reason: string };

export type SessionChannelEnd =
  // The stream ran to its end (or the caller aborted it).
  | { ok: true; aborted: boolean }
  // The server refused the stream, or the connection dropped mid-way.
  | { ok: false; error: string; status?: number };

const enc = encodeURIComponent;

export function sessionChannelStreamPath(
  sessionId: string,
  channel: string,
  tenant: string,
): string {
  return `/api/sessions/${enc(sessionId)}/channels/${enc(channel)}/stream?tenantId=${enc(tenant)}`;
}

// A line that is not one of the five frames is dropped, never thrown: a
// truncated last line on a dropped connection must not take the panel down.
export function parseSessionChannelFrame(
  line: string,
): SessionChannelFrame | null {
  let value: unknown;
  try {
    value = JSON.parse(line);
  } catch {
    return null;
  }
  if (!value || typeof value !== "object") return null;
  const frame = value as Record<string, unknown>;
  switch (frame.kind) {
    case "opened":
      return typeof frame.channel === "string" &&
        typeof frame.targetGeneration === "number"
        ? {
            kind: "opened",
            channel: frame.channel,
            targetGeneration: frame.targetGeneration,
          }
        : null;
    case "stdout":
    case "stderr":
      return typeof frame.data === "string"
        ? { kind: frame.kind, data: frame.data }
        : null;
    case "exit":
      return typeof frame.code === "number"
        ? { kind: "exit", code: frame.code }
        : null;
    case "closed":
      return typeof frame.reason === "string"
        ? { kind: "closed", reason: frame.reason }
        : null;
    default:
      return null;
  }
}

// Read one channel stream to its end, handing each frame to `onFrame` in
// arrival order. The read is a plain `fetch` with the console's cookie
// session, so it needs no token in the page; `signal` aborts it.
export async function readSessionChannel(
  path: string,
  onFrame: (frame: SessionChannelFrame) => void,
  signal: AbortSignal,
): Promise<SessionChannelEnd> {
  let response: Response;
  try {
    response = await fetch(path, {
      credentials: "include",
      headers: { accept: "application/x-ndjson" },
      signal,
    });
  } catch (err) {
    if (signal.aborted) return { ok: true, aborted: true };
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    };
  }
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as unknown;
    return {
      ok: false,
      status: response.status,
      error: apiErrorMessage(body, response.status),
    };
  }
  if (!response.body) {
    return { ok: false, error: "The stream response carried no body." };
  }

  const reader = response.body.getReader();
  // An abort must stop delivery at once, not after the next chunk lands:
  // the panel that aborted has already let go of its session. Cancelling
  // the reader also unblocks a pending `read()` on a quiet stream.
  const cancel = () => {
    reader.cancel().catch(() => undefined);
  };
  if (signal.aborted) {
    cancel();
    return { ok: true, aborted: true };
  }
  signal.addEventListener("abort", cancel, { once: true });

  const decoder = new TextDecoder();
  let buffered = "";
  const deliver = (chunk: string) => {
    buffered += chunk;
    let newline = buffered.indexOf("\n");
    while (newline !== -1 && !signal.aborted) {
      const line = buffered.slice(0, newline).trim();
      buffered = buffered.slice(newline + 1);
      if (line.length > 0) {
        const frame = parseSessionChannelFrame(line);
        if (frame) onFrame(frame);
      }
      newline = buffered.indexOf("\n");
    }
  };

  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done || signal.aborted) break;
      deliver(decoder.decode(value, { stream: true }));
    }
    if (!signal.aborted) {
      deliver(decoder.decode());
      const tail = buffered.trim();
      if (tail.length > 0) {
        const frame = parseSessionChannelFrame(tail);
        if (frame) onFrame(frame);
      }
    }
    return { ok: true, aborted: signal.aborted };
  } catch (err) {
    if (signal.aborted) return { ok: true, aborted: true };
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    };
  } finally {
    signal.removeEventListener("abort", cancel);
  }
}
