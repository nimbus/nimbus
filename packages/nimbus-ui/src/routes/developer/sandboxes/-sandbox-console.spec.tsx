import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

import { makeSession, ndjsonBody } from "../../../test/sandbox-fixtures";
import {
  CONSOLE_CHANNEL,
  CONSOLE_SESSION_TTL_MS,
  SandboxConsole,
} from "./-sandbox-console";

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

const FRAMES = [
  { kind: "opened", channel: "stdio", targetGeneration: 1 },
  { kind: "stdout", data: "hello\n" },
  { kind: "stderr", data: "careful\n" },
  { kind: "exit", code: 0 },
  { kind: "closed", reason: "process exited" },
];

function stream(frames: unknown[], gapMs = 2) {
  return new HttpResponse(ndjsonBody(frames, gapMs), {
    headers: { "content-type": "application/x-ndjson" },
  });
}

// One msw session backend: records the open and close bodies and the
// channel writes, and answers the stream with the given frames.
function serveSession(frames: unknown[] | (() => Response)) {
  const opens: unknown[] = [];
  const closes: Array<{ path: string; body: unknown }> = [];
  const writes: Array<{ path: string; body: unknown }> = [];
  server.use(
    http.post("*/api/sessions", async ({ request }) => {
      opens.push(await request.json());
      return HttpResponse.json(makeSession(), { status: 201 });
    }),
    http.post("*/api/sessions/:id/close", async ({ request }) => {
      const url = new URL(request.url);
      closes.push({
        path: url.pathname + url.search,
        body: await request.json(),
      });
      return HttpResponse.json(makeSession());
    }),
    http.post(
      "*/api/sessions/:id/channels/:channel/input",
      async ({ request }) => {
        const url = new URL(request.url);
        writes.push({
          path: url.pathname + url.search,
          body: await request.json(),
        });
        return new HttpResponse(null, { status: 202 });
      },
    ),
    http.get("*/api/sessions/:id/channels/:channel/stream", () =>
      typeof frames === "function" ? frames() : stream(frames),
    ),
  );
  return { opens, closes, writes };
}

function lines() {
  return screen.getAllByTestId("console-line").map((line) => ({
    kind: line.getAttribute("data-kind"),
    text: line.textContent,
  }));
}

function renderConsole(lifecycleState = "ready") {
  return render(
    <SandboxConsole
      tenant="acme"
      sandboxId="sb-1"
      lifecycleState={lifecycleState}
      testid="console"
    />,
  );
}

describe("SandboxConsole", () => {
  // The acceptance criterion of the task: the panel appends the channel's
  // frames in the order they arrive, stdout and stderr interleaved as the
  // process wrote them, and nothing is merged or reordered.
  it("opens a stdio session on a ready sandbox and appends the frames in order", async () => {
    const { opens, closes } = serveSession(FRAMES);
    renderConsole();

    await waitFor(() =>
      expect(screen.getAllByTestId("console-line")).toHaveLength(5),
    );
    expect(lines()).toEqual([
      { kind: "opened", text: "attached to stdio at generation 1" },
      { kind: "stdout", text: "hello\n" },
      { kind: "stderr", text: "careful\n" },
      { kind: "exit", text: "process exited with code 0" },
      { kind: "closed", text: "stream closed: process exited" },
    ]);
    expect(opens).toEqual([
      {
        tenantId: "acme",
        target: { sandbox: { id: "sb-1" } },
        channels: [CONSOLE_CHANNEL],
        requestedTtlMs: CONSOLE_SESSION_TTL_MS,
      },
    ]);
    expect(screen.getByTestId("console")).toHaveAttribute(
      "data-connection",
      "ended",
    );
    expect(screen.getByTestId("console-status")).toHaveTextContent(
      "Stream ended: process exited.",
    );
    expect(screen.getByTestId("console-input")).toBeDisabled();
    expect(closes).toEqual([]);
  });

  it("writes an input line to the channel while attached and closes the session on unmount", async () => {
    // The stream opens and then holds, as a live shell does.
    const { writes, closes } = serveSession(() =>
      stream(
        [{ kind: "opened", channel: "stdio", targetGeneration: 1 }],
        10_000,
      ),
    );
    const { unmount } = renderConsole();

    await waitFor(() =>
      expect(screen.getByTestId("console")).toHaveAttribute(
        "data-connection",
        "attached",
      ),
    );
    await waitFor(() =>
      expect(screen.getByTestId("console-input")).toBeEnabled(),
    );
    const input = screen.getByTestId("console-input");
    fireEvent.change(input, { target: { value: "ls -la" } });
    const form = input.closest("form");
    if (!form) throw new Error("input outside its form");
    fireEvent.submit(form);

    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]).toEqual({
      path: "/api/sessions/sess-1/channels/stdio/input?tenantId=acme",
      body: { data: "ls -la\n" },
    });
    expect(lines().at(-1)).toEqual({ kind: "input", text: "› ls -la" });
    expect(input).toHaveValue("");

    unmount();
    await waitFor(() => expect(closes).toHaveLength(1));
    expect(closes[0]).toEqual({
      path: "/api/sessions/sess-1/close?tenantId=acme",
      body: { reason: "console closed" },
    });
  });

  it("waits for the operator when the sandbox is not ready, then connects on demand", async () => {
    const { opens } = serveSession(FRAMES);
    renderConsole("starting");

    expect(screen.getByTestId("console-status")).toHaveTextContent(
      "The sandbox is starting",
    );
    expect(screen.getByTestId("console-empty")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(opens).toHaveLength(0);

    fireEvent.click(screen.getByTestId("console-connect"));
    await waitFor(() =>
      expect(screen.getAllByTestId("console-line")).toHaveLength(5),
    );
    expect(opens).toHaveLength(1);
  });

  it("shows a refused session as an error line and keeps the Connect control", async () => {
    server.use(
      http.post("*/api/sessions", () =>
        HttpResponse.json(
          { error: { message: "sandbox sb-1 is not ready" } },
          { status: 409 },
        ),
      ),
    );
    renderConsole();
    await waitFor(() =>
      expect(screen.getByTestId("console")).toHaveAttribute(
        "data-connection",
        "error",
      ),
    );
    expect(lines()).toEqual([
      { kind: "error", text: "session refused: sandbox sb-1 is not ready" },
    ]);
    expect(screen.getByTestId("console-connect")).toBeInTheDocument();
  });
});
