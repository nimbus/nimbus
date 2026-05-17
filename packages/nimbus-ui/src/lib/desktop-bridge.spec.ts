import { afterEach, describe, expect, it, vi } from "vitest";

import { getDesktopBridge, isLocalHost } from "./desktop-bridge";

afterEach(() => {
  vi.unstubAllGlobals();
  Reflect.deleteProperty(
    window as unknown as Record<string, unknown>,
    "nimbus",
  );
});

describe("isLocalHost", () => {
  it("treats null and empty as local", () => {
    expect(isLocalHost(null)).toBe(true);
    expect(isLocalHost(undefined)).toBe(true);
    expect(isLocalHost("")).toBe(true);
  });

  it("treats loopback aliases as local", () => {
    for (const value of ["localhost", "127.0.0.1", "::1", "[::1]"]) {
      expect(isLocalHost(value)).toBe(true);
    }
  });

  it("treats matching hostname as local", () => {
    expect(isLocalHost(window.location.hostname)).toBe(true);
  });

  it("flags a different host as remote", () => {
    expect(isLocalHost("server.example.com")).toBe(false);
  });
});

describe("getDesktopBridge", () => {
  it("returns null when window.nimbus is missing", () => {
    expect(getDesktopBridge()).toBe(null);
  });

  it("returns the bridge when runUpgrade is a function", () => {
    const bridge = { runUpgrade: vi.fn() };
    (window as unknown as Record<string, unknown>).nimbus = bridge;
    expect(getDesktopBridge()).toBe(bridge);
  });

  it("returns null when runUpgrade is not a function", () => {
    (window as unknown as Record<string, unknown>).nimbus = {
      runUpgrade: "nope",
    };
    expect(getDesktopBridge()).toBe(null);
  });
});
