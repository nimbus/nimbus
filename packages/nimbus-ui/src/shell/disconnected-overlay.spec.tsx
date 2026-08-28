import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { connectionState } = vi.hoisted(() => ({
  connectionState: {
    current: {
      isWebSocketConnected: false,
      hasEverConnected: true,
    },
  },
}));

vi.mock("@nimbus/nimbus/react", () => ({
  useNimbusConnectionState: () => connectionState.current,
}));

import { DisconnectedOverlay, probeUiSession } from "./disconnected-overlay";

beforeEach(() => {
  vi.useFakeTimers();
  connectionState.current = {
    isWebSocketConnected: false,
    hasEverConnected: true,
  };
});

afterEach(() => {
  vi.useRealTimers();
});

describe("DisconnectedOverlay", () => {
  it("sends an expired browser session back to the auth page", async () => {
    const probeSession = vi.fn().mockResolvedValue("reauthenticate");
    const onSessionExpired = vi.fn();
    render(
      <DisconnectedOverlay
        probeSession={probeSession}
        onSessionExpired={onSessionExpired}
        probeIntervalMs={25}
      />,
    );

    expect(screen.getByTestId("disconnected-overlay")).toHaveTextContent(
      "Reconnecting",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(25);
    });

    expect(probeSession).toHaveBeenCalledOnce();
    expect(onSessionExpired).toHaveBeenCalledOnce();
  });

  it("does not probe before the first successful connection", async () => {
    connectionState.current = {
      isWebSocketConnected: false,
      hasEverConnected: false,
    };
    const probeSession = vi.fn().mockResolvedValue("reauthenticate");
    render(
      <DisconnectedOverlay probeSession={probeSession} probeIntervalMs={25} />,
    );

    await act(async () => {
      await vi.advanceTimersByTimeAsync(100);
    });

    expect(screen.queryByTestId("disconnected-overlay")).toBeNull();
    expect(probeSession).not.toHaveBeenCalled();
  });
});

describe("probeUiSession", () => {
  it.each([
    [401, "basic"],
    [403, "basic"],
    [307, "basic"],
    [0, "opaqueredirect"],
  ] as const)("classifies status %s / %s as reauthentication", async (status, type) => {
    const fetchImpl = vi.fn().mockResolvedValue({ status, type });
    await expect(
      probeUiSession(fetchImpl as unknown as typeof fetch),
    ).resolves.toBe("reauthenticate");
  });

  it("distinguishes an unavailable server from an authorized response", async () => {
    const unavailable = vi.fn().mockRejectedValue(new Error("offline"));
    const authorized = vi
      .fn()
      .mockResolvedValue({ status: 200, type: "basic" });

    await expect(
      probeUiSession(unavailable as unknown as typeof fetch),
    ).resolves.toBe("unreachable");
    await expect(
      probeUiSession(authorized as unknown as typeof fetch),
    ).resolves.toBe("authorized");
  });
});
