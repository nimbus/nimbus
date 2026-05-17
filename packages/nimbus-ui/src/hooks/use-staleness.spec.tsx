import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { VersionInfo } from "../api/system";
import type { DesktopBridge, UpgradeEvent } from "../lib/desktop-bridge";
import { useStaleness } from "./use-staleness";

function makeInfo(over: Partial<VersionInfo> = {}): VersionInfo {
  return {
    current: "0.1.40",
    latest: "0.1.41",
    available: true,
    url: "https://example.com/release",
    publishedAt: "2026-05-15T00:00:00Z",
    host: "localhost",
    checkStatus: "fresh",
    upgrade: {
      method: "brew",
      command: "brew upgrade nimbus/tap/nimbus",
      needsSudo: false,
      interactive: false,
      fallbackUrl: "https://example.com/install",
    },
    ...over,
  };
}

const sonnerMocks = vi.hoisted(() => {
  const base = vi.fn();
  const error = vi.fn();
  const success = vi.fn();
  return { base, error, success };
});

vi.mock("sonner", () => {
  const toast = sonnerMocks.base as unknown as ((
    message: string,
    opts?: unknown,
  ) => void) & {
    error: typeof sonnerMocks.error;
    success: typeof sonnerMocks.success;
  };
  toast.error = sonnerMocks.error;
  toast.success = sonnerMocks.success;
  return { toast };
});

beforeEach(() => {
  window.localStorage.clear();
  sonnerMocks.base.mockClear();
  sonnerMocks.error.mockClear();
  sonnerMocks.success.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useStaleness", () => {
  it("starts hidden then transitions to available on first poll", async () => {
    const fetchInfo = vi.fn().mockResolvedValue(makeInfo());
    const { result } = renderHook(() =>
      useStaleness({ fetchInfo, defaultPollMs: 10_000, bridge: null }),
    );
    await waitFor(() => {
      expect(result.current.snapshot.state).toBe("available");
    });
    expect(result.current.snapshot.info?.latest).toBe("0.1.41");
    expect(sonnerMocks.base).toHaveBeenCalledTimes(1);
  });

  it("stays hidden when checkStatus is disabled", async () => {
    const fetchInfo = vi
      .fn()
      .mockResolvedValue(
        makeInfo({ checkStatus: "disabled", available: false, latest: null }),
      );
    const { result } = renderHook(() =>
      useStaleness({ fetchInfo, bridge: null }),
    );
    await waitFor(() => expect(fetchInfo).toHaveBeenCalled());
    expect(result.current.snapshot.state).toBe("hidden");
    expect(sonnerMocks.base).not.toHaveBeenCalled();
  });

  it("does not re-emit the toast for the same latest", async () => {
    const fetchInfo = vi.fn().mockResolvedValue(makeInfo());
    const { result } = renderHook(() =>
      useStaleness({ fetchInfo, defaultPollMs: 5, bridge: null }),
    );
    await waitFor(() => {
      expect(result.current.snapshot.state).toBe("available");
    });
    const before = sonnerMocks.base.mock.calls.length;
    // wait long enough for several poll intervals
    await new Promise((r) => setTimeout(r, 60));
    expect(sonnerMocks.base.mock.calls.length).toBe(before);
  });

  it("re-emits the toast when latest flips to a new version", async () => {
    let info = makeInfo();
    const fetchInfo = vi
      .fn()
      .mockImplementation(() => Promise.resolve({ ...info }));
    const { result } = renderHook(() =>
      useStaleness({ fetchInfo, defaultPollMs: 5, bridge: null }),
    );
    await waitFor(() =>
      expect(result.current.snapshot.state).toBe("available"),
    );
    expect(sonnerMocks.base).toHaveBeenCalledTimes(1);

    info = makeInfo({ latest: "0.1.42" });
    await waitFor(() => {
      expect(result.current.snapshot.info?.latest).toBe("0.1.42");
    });
    await waitFor(() => expect(sonnerMocks.base.mock.calls.length).toBe(2));
  });

  it("skips the toast when localStorage records dismissal for that version", async () => {
    window.localStorage.setItem(
      "nimbus-ui:staleness-dismissed-version",
      "0.1.41",
    );
    const fetchInfo = vi.fn().mockResolvedValue(makeInfo());
    const { result } = renderHook(() =>
      useStaleness({ fetchInfo, bridge: null }),
    );
    await waitFor(() =>
      expect(result.current.snapshot.state).toBe("available"),
    );
    expect(sonnerMocks.base).not.toHaveBeenCalled();
    expect(result.current.snapshot.dismissed).toBe(true);
  });

  it("opens the popover via openPopover and closes via closePopover", async () => {
    const fetchInfo = vi.fn().mockResolvedValue(makeInfo());
    const { result } = renderHook(() =>
      useStaleness({ fetchInfo, bridge: null }),
    );
    await waitFor(() =>
      expect(result.current.snapshot.state).toBe("available"),
    );
    act(() => result.current.openPopover());
    expect(result.current.snapshot.state).toBe("confirming");
    act(() => result.current.closePopover());
    expect(result.current.snapshot.state).toBe("available");
  });

  it("startUpgrade only invokes runUpgrade with the method tag", async () => {
    const events: UpgradeEvent[] = [{ kind: "exit", code: 0 }];
    const runUpgrade = vi.fn(async function* () {
      for (const e of events) yield e;
    });
    const bridge: DesktopBridge = { runUpgrade };
    const fetchInfo = vi.fn().mockResolvedValue(makeInfo());
    const { result } = renderHook(() =>
      useStaleness({ fetchInfo, bridge, defaultPollMs: 5_000 }),
    );
    await waitFor(() =>
      expect(result.current.snapshot.state).toBe("available"),
    );
    await act(async () => {
      await result.current.startUpgrade();
    });
    expect(runUpgrade).toHaveBeenCalledTimes(1);
    expect(runUpgrade).toHaveBeenCalledWith("brew");
    // method tag only — no command string ever crosses the boundary
    const args = runUpgrade.mock.calls[0];
    expect(args).toEqual(["brew"]);
  });

  it("transitions to upgraded once current >= targetLatest", async () => {
    let info = makeInfo();
    const fetchInfo = vi
      .fn()
      .mockImplementation(() => Promise.resolve({ ...info }));
    const runUpgrade = vi.fn(async function* () {
      yield { kind: "exit", code: 0 } as UpgradeEvent;
    });
    const bridge: DesktopBridge = { runUpgrade };
    const { result } = renderHook(() =>
      useStaleness({
        fetchInfo,
        bridge,
        defaultPollMs: 50,
        activePollMs: 10,
      }),
    );
    await waitFor(() =>
      expect(result.current.snapshot.state).toBe("available"),
    );
    await act(async () => {
      await result.current.startUpgrade();
    });
    expect(result.current.snapshot.state).toBe("upgrading");
    // simulate the server reporting the new version after the brew run
    info = makeInfo({ current: "0.1.41", available: false, latest: null });
    await waitFor(() => expect(result.current.snapshot.state).toBe("upgraded"));
  });

  it("reverts from upgrading to available when the bridge yields an error", async () => {
    const runUpgrade = vi.fn(async function* () {
      yield { kind: "error", message: "boom" } as UpgradeEvent;
    });
    const bridge: DesktopBridge = { runUpgrade };
    const fetchInfo = vi.fn().mockResolvedValue(makeInfo());
    const { result } = renderHook(() =>
      useStaleness({ fetchInfo, bridge, defaultPollMs: 5_000 }),
    );
    await waitFor(() =>
      expect(result.current.snapshot.state).toBe("available"),
    );
    await act(async () => {
      await result.current.startUpgrade();
    });
    expect(result.current.snapshot.state).toBe("available");
    expect(sonnerMocks.error).toHaveBeenCalled();
  });

  it("copyCommand transitions to upgrading without invoking the bridge", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    const fetchInfo = vi.fn().mockResolvedValue(makeInfo());
    const runUpgrade = vi.fn();
    const bridge: DesktopBridge = { runUpgrade };
    const { result } = renderHook(() =>
      useStaleness({ fetchInfo, bridge, defaultPollMs: 5_000 }),
    );
    await waitFor(() =>
      expect(result.current.snapshot.state).toBe("available"),
    );
    await act(async () => {
      await result.current.copyCommand();
    });
    expect(writeText).toHaveBeenCalledWith("brew upgrade nimbus/tap/nimbus");
    expect(runUpgrade).not.toHaveBeenCalled();
    expect(result.current.snapshot.state).toBe("upgrading");
  });
});
