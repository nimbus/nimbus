import { describe, expect, it } from "vitest";

import {
  type ConnectionSnapshot,
  isLoading,
  isOffline,
  type LoadingValue,
  toLoadingValue,
} from "./loading-value";

const CONNECTED: ConnectionSnapshot = {
  isWebSocketConnected: true,
  hasEverConnected: true,
};
const RECONNECTING: ConnectionSnapshot = {
  isWebSocketConnected: false,
  hasEverConnected: true,
};
const INITIAL: ConnectionSnapshot = {
  isWebSocketConnected: false,
  hasEverConnected: false,
};

describe("toLoadingValue", () => {
  describe("kind: ok", () => {
    it("returns ok with the value when defined and non-null", () => {
      expect(toLoadingValue(42, CONNECTED)).toEqual({
        kind: "ok",
        value: 42,
      });
    });

    it("returns ok even when offline if a value is already in hand", () => {
      expect(toLoadingValue("v1", RECONNECTING)).toEqual({
        kind: "ok",
        value: "v1",
      });
    });

    it("narrows NonNullable<T> on ok", () => {
      const v: LoadingValue<number> = toLoadingValue<number | null | undefined>(
        7,
        CONNECTED,
      );
      if (v.kind === "ok") {
        const n: number = v.value;
        expect(n).toBe(7);
      }
    });
  });

  describe("kind: loading", () => {
    it("returns loading when value is undefined and WS connected", () => {
      expect(toLoadingValue(undefined, CONNECTED)).toEqual({ kind: "loading" });
    });

    it("returns loading when value is null and WS connected", () => {
      expect(toLoadingValue(null, CONNECTED)).toEqual({ kind: "loading" });
    });

    it("returns loading on initial load (never connected, no value)", () => {
      expect(toLoadingValue(undefined, INITIAL)).toEqual({ kind: "loading" });
    });
  });

  describe("kind: offline", () => {
    it("returns offline when value is missing and we have lost an established connection", () => {
      expect(toLoadingValue(undefined, RECONNECTING)).toEqual({
        kind: "offline",
      });
    });
  });
});

describe("isLoading / isOffline", () => {
  it("identifies loading", () => {
    expect(isLoading({ kind: "loading" })).toBe(true);
    expect(isLoading({ kind: "ok", value: 1 })).toBe(false);
  });
  it("identifies offline", () => {
    expect(isOffline({ kind: "offline" })).toBe(true);
    expect(isOffline({ kind: "loading" })).toBe(false);
  });
});
