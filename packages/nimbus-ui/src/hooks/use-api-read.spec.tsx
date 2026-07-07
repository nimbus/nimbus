import { renderHook, waitFor } from "@testing-library/react";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import {
  afterAll,
  afterEach,
  beforeAll,
  describe,
  expect,
  it,
  vi,
} from "vitest";

import { useApiRead } from "./use-api-read";

const server = setupServer();

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

describe("useApiRead", () => {
  it("resolves a successful body to a LoadingValue ok", async () => {
    server.use(
      http.get("*/api/thing", () => HttpResponse.json({ n: 42 })),
    );
    const { result } = renderHook(() =>
      useApiRead<{ n: number }>("/api/thing", []),
    );
    expect(result.current).toEqual({ kind: "loading" });
    await waitFor(() =>
      expect(result.current).toEqual({ kind: "ok", value: { n: 42 } }),
    );
  });

  it("maps a failing read to a LoadingValue error with the envelope message", async () => {
    server.use(
      http.get("*/api/thing", () =>
        HttpResponse.json({ error: { message: "boom" } }, { status: 500 }),
      ),
    );
    const { result } = renderHook(() => useApiRead("/api/thing", []));
    await waitFor(() =>
      expect(result.current).toEqual({ kind: "error", message: "boom" }),
    );
  });

  it("aborts the in-flight read and never updates state after unmount", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const gate = deferred<void>();
    let capturedSignal: AbortSignal | null = null;
    server.use(
      http.get("*/api/thing", async ({ request }) => {
        capturedSignal = request.signal;
        await gate.promise;
        return HttpResponse.json({ n: 1 });
      }),
    );

    const { result, unmount } = renderHook(() =>
      useApiRead<{ n: number }>("/api/thing", []),
    );

    await waitFor(() => expect(capturedSignal).not.toBeNull());
    const signal = capturedSignal as unknown as AbortSignal;
    expect(signal.aborted).toBe(false);

    unmount();
    expect(signal.aborted).toBe(true);

    // The read may still resolve after unmount; the hook must swallow it.
    gate.resolve();
    await new Promise((r) => setTimeout(r, 10));
    expect(result.current).toEqual({ kind: "loading" });
    expect(errorSpy).not.toHaveBeenCalled();
    errorSpy.mockRestore();
  });
});
