import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useTenantList } from "./use-tenant-list";

function okOnce(tenants: string[]) {
  return {
    ok: true,
    json: async () => ({ tenants }),
  };
}

function failOnce(message: string) {
  return {
    ok: false,
    status: 503,
    json: async () => ({ error: { message } }),
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

/**
 * The hook fetched once on mount and offered no way back, so a consumer that
 * hit a transient failure could only recover by reloading the whole console —
 * which discards every other panel's data to re-run one request.
 */
describe("useTenantList reload", () => {
  it("issues a second request and recovers the list", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(failOnce("tenant store offline"))
      .mockResolvedValueOnce(okOnce(["acme", "demo"]));
    vi.stubGlobal("fetch", fetchMock);

    const { result } = renderHook(() => useTenantList());

    await waitFor(() => expect(result.current.kind).toBe("error"));
    expect(fetchMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      result.current.reload();
    });

    // The retry has to re-request, not just re-render: one call would mean the
    // button only cleared the error locally and left the panel empty.
    expect(fetchMock).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(result.current.kind).toBe("loaded"));
    expect(
      result.current.kind === "loaded"
        ? result.current.tenants.map((t) => t.id)
        : null,
    ).toEqual(["acme", "demo"]);
  });

  it("returns to the loading state while the retry is in flight", async () => {
    let settleSecond: ((value: unknown) => void) | undefined;
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(failOnce("tenant store offline"))
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            settleSecond = resolve;
          }),
      );
    vi.stubGlobal("fetch", fetchMock);

    const { result } = renderHook(() => useTenantList());
    await waitFor(() => expect(result.current.kind).toBe("error"));

    act(() => {
      result.current.reload();
    });

    // A retry that leaves the stale error on screen while it works reads as a
    // button that did nothing.
    expect(result.current.kind).toBe("loading");

    await act(async () => {
      settleSecond?.(okOnce([]));
    });
    await waitFor(() => expect(result.current.kind).toBe("loaded"));
  });

  it("reports the second failure rather than the first", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(failOnce("tenant store offline"))
      .mockResolvedValueOnce(failOnce("still offline"));
    vi.stubGlobal("fetch", fetchMock);

    const { result } = renderHook(() => useTenantList());
    await waitFor(() => expect(result.current.kind).toBe("error"));

    await act(async () => {
      result.current.reload();
    });

    await waitFor(() => {
      expect(
        result.current.kind === "error" ? result.current.message : null,
      ).toBe("still offline");
    });
  });

  it("keeps a stable reload identity across renders", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(okOnce(["acme"])));

    const { result, rerender } = renderHook(() => useTenantList());
    const first = result.current.reload;
    await waitFor(() => expect(result.current.kind).toBe("loaded"));
    rerender();

    // Consumers pass this straight to an onClick and into effect deps; a new
    // function each render would re-fire both.
    expect(result.current.reload).toBe(first);
  });

  it("ignores a superseded response so the newer answer wins", async () => {
    // A reader who clicks Retry twice has two requests open. Without the
    // abort, whichever the network happens to finish last sets the state — so
    // a slow first response can overwrite a fresher one.
    const settle: Array<(value: unknown) => void> = [];
    const pending = () =>
      new Promise((resolve) => {
        settle.push(resolve);
      });
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValueOnce(failOnce("first"))
        .mockImplementation(pending),
    );

    const { result } = renderHook(() => useTenantList());
    await waitFor(() => expect(result.current.kind).toBe("error"));

    act(() => {
      result.current.reload();
    });
    act(() => {
      result.current.reload();
    });

    await act(async () => {
      settle[1]?.(okOnce(["second-wins"]));
      settle[0]?.(okOnce(["stale"]));
    });

    await waitFor(() => expect(result.current.kind).toBe("loaded"));
    expect(
      result.current.kind === "loaded"
        ? result.current.tenants.map((t) => t.id)
        : null,
    ).toEqual(["second-wins"]);
  });
});
