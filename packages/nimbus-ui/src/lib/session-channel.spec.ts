import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

import { ndjsonBody } from "../test/sandbox-fixtures";
import {
  parseSessionChannelFrame,
  readSessionChannel,
  type SessionChannelFrame,
  sessionChannelStreamPath,
} from "./session-channel";

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe("sessionChannelStreamPath", () => {
  it("names the stream route with the tenant in the query", () => {
    expect(sessionChannelStreamPath("sess 1", "stdio", "acme/2")).toBe(
      "/api/sessions/sess%201/channels/stdio/stream?tenantId=acme%2F2",
    );
  });
});

describe("parseSessionChannelFrame", () => {
  it("accepts each frame kind with its fields", () => {
    expect(
      parseSessionChannelFrame(
        '{"kind":"opened","channel":"stdio","targetGeneration":4}',
      ),
    ).toEqual({ kind: "opened", channel: "stdio", targetGeneration: 4 });
    expect(
      parseSessionChannelFrame('{"kind":"stdout","data":"hi\\n"}'),
    ).toEqual({ kind: "stdout", data: "hi\n" });
    expect(parseSessionChannelFrame('{"kind":"stderr","data":"no"}')).toEqual({
      kind: "stderr",
      data: "no",
    });
    expect(parseSessionChannelFrame('{"kind":"exit","code":3}')).toEqual({
      kind: "exit",
      code: 3,
    });
    expect(
      parseSessionChannelFrame('{"kind":"closed","reason":"done"}'),
    ).toEqual({ kind: "closed", reason: "done" });
  });

  it("refuses a malformed line, an unknown kind, or a missing field", () => {
    expect(parseSessionChannelFrame("not json")).toBeNull();
    expect(parseSessionChannelFrame('{"kind":"video","data":""}')).toBeNull();
    expect(parseSessionChannelFrame('{"kind":"stdout"}')).toBeNull();
    expect(parseSessionChannelFrame('{"kind":"exit","code":"3"}')).toBeNull();
    expect(parseSessionChannelFrame("")).toBeNull();
  });
});

describe("readSessionChannel", () => {
  it("delivers the frames in arrival order, including one split across chunks", async () => {
    const encoder = new TextEncoder();
    const chunks = [
      '{"kind":"opened","channel":"stdio","targetGeneration":1}\n{"kind":"stdout","da',
      'ta":"one\\n"}\n{"kind":"stderr","data":"two\\n"}\n',
      '{"kind":"exit","code":0}\n{"kind":"closed","reason":"exited"}',
    ];
    server.use(
      http.get("*/api/sessions/:id/channels/:channel/stream", () => {
        const stream = new ReadableStream<Uint8Array>({
          async start(controller) {
            for (const chunk of chunks) {
              controller.enqueue(encoder.encode(chunk));
              await new Promise((resolve) => setTimeout(resolve, 2));
            }
            controller.close();
          },
        });
        return new HttpResponse(stream, {
          headers: { "content-type": "application/x-ndjson" },
        });
      }),
    );
    const frames: SessionChannelFrame[] = [];
    const end = await readSessionChannel(
      sessionChannelStreamPath("sess-1", "stdio", "acme"),
      (frame) => frames.push(frame),
      new AbortController().signal,
    );
    expect(end).toEqual({ ok: true, aborted: false });
    expect(frames.map((frame) => frame.kind)).toEqual([
      "opened",
      "stdout",
      "stderr",
      "exit",
      "closed",
    ]);
    expect(frames[1]).toEqual({ kind: "stdout", data: "one\n" });
  });

  it("reports a refused stream with the envelope message and status", async () => {
    server.use(
      http.get("*/api/sessions/:id/channels/:channel/stream", () =>
        HttpResponse.json(
          { error: { message: "session sess-1 is not attached" } },
          { status: 409 },
        ),
      ),
    );
    const frames: SessionChannelFrame[] = [];
    const end = await readSessionChannel(
      sessionChannelStreamPath("sess-1", "stdio", "acme"),
      (frame) => frames.push(frame),
      new AbortController().signal,
    );
    expect(end).toEqual({
      ok: false,
      error: "session sess-1 is not attached",
      status: 409,
    });
    expect(frames).toEqual([]);
  });

  it("ends as aborted when the caller aborts mid-stream", async () => {
    server.use(
      http.get("*/api/sessions/:id/channels/:channel/stream", () => {
        const stream = ndjsonBody(
          [
            { kind: "opened", channel: "stdio", targetGeneration: 1 },
            { kind: "stdout", data: "tick\n" },
            { kind: "stdout", data: "tick\n" },
            { kind: "stdout", data: "tick\n" },
          ],
          20,
        );
        return new HttpResponse(stream, {
          headers: { "content-type": "application/x-ndjson" },
        });
      }),
    );
    const controller = new AbortController();
    const frames: SessionChannelFrame[] = [];
    const reading = readSessionChannel(
      sessionChannelStreamPath("sess-1", "stdio", "acme"),
      (frame) => {
        frames.push(frame);
        if (frame.kind === "stdout") controller.abort();
      },
      controller.signal,
    );
    const end = await reading;
    expect(end).toEqual({ ok: true, aborted: true });
    expect(frames.length).toBeLessThan(4);
    expect(frames[0]?.kind).toBe("opened");
  });
});
