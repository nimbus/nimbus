import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@nimbus/nimbus/react", () => ({
  useNimbus: () => ({ url: "http://localhost:9000" }),
  useNimbusConnectionState: () => ({
    isWebSocketConnected: true,
    hasEverConnected: true,
    hasInflightRequests: false,
    inflightMutations: 0,
    inflightActions: 0,
  }),
  useQuery: () => ({ version: "0.1.0", buildHash: "abcdef0" }),
}));

vi.mock("../hooks/use-staleness", () => ({
  useStalenessContext: () => ({
    snapshot: { state: "hidden", info: null, targetLatest: null },
    isLocal: false,
    hasDesktopBridge: false,
    openPopover: vi.fn(),
    closePopover: vi.fn(),
    startUpgrade: vi.fn(),
    copyCommand: vi.fn(),
  }),
}));

import { StatusBar } from "./status-bar";

describe("StatusBar", () => {
  it("shows the connection status", () => {
    render(<StatusBar />);
    expect(screen.getByTestId("status-connection")).toHaveTextContent(
      "Connected",
    );
  });

  it("shows the server URL", () => {
    render(<StatusBar />);
    expect(screen.getByTestId("status-server-url")).toHaveTextContent(
      "http://localhost:9000",
    );
  });

  it("no longer renders a steady-state version (moved to the top nav)", () => {
    render(<StatusBar />);
    expect(screen.queryByTestId("status-version")).toBeNull();
  });

  it("no longer renders a tenant slot in the footer", () => {
    render(<StatusBar />);
    expect(screen.queryByTestId("status-tenant")).toBeNull();
  });
});
