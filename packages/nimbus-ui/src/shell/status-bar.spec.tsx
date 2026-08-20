import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

const { snapshotRef } = vi.hoisted(() => ({
  snapshotRef: {
    current: { state: "hidden", info: null, targetLatest: null } as {
      state: string;
      info: unknown;
      targetLatest: string | null;
    },
  },
}));

vi.mock("../hooks/use-staleness", () => ({
  useStalenessContext: () => ({
    snapshot: snapshotRef.current,
    isLocal: false,
    hasDesktopBridge: false,
    openPopover: vi.fn(),
    closePopover: vi.fn(),
    startUpgrade: vi.fn(),
    copyCommand: vi.fn(),
  }),
}));

import type { VersionInfo } from "../api/system";
import { statePalette } from "../components/state-chip";
import { StatusBar, UPGRADE_TONE_KINDS, type UpgradeTone } from "./status-bar";

beforeEach(() => {
  snapshotRef.current = { state: "hidden", info: null, targetLatest: null };
});

afterEach(() => {
  vi.unstubAllGlobals();
});

// The global test setup stubs matchMedia to always report `matches: false`,
// which is the desktop tier. Narrow it per query to drive the other tiers.
function stubTier(tier: "mobile" | "tablet") {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: tier === "mobile" ? true : query.includes("1023px"),
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  }));
}

const UPGRADE_STATES: Array<[UpgradeTone, string, string]> = [
  ["available", "available", "status-version-available"],
  ["upgrading", "upgrading", "status-version-upgrading"],
  ["upgraded", "upgraded", "status-version-upgraded"],
];

// The available/confirming rows mount UpgradePopover, which reads the whole
// VersionInfo, so the fixture has to be complete rather than just `latest`.
const VERSION_INFO: VersionInfo = {
  current: "0.1.0",
  latest: "0.2.0",
  available: true,
  url: "https://example.invalid/releases/v0.2.0",
  publishedAt: "2026-08-01T00:00:00Z",
  host: "localhost",
  checkStatus: "fresh",
  upgrade: {
    method: "brew",
    command: "brew upgrade nimbus",
    needsSudo: false,
    interactive: false,
    fallbackUrl: "https://example.invalid/INSTALL.md",
  },
};

function showUpgrade(state: string) {
  snapshotRef.current = { state, info: VERSION_INFO, targetLatest: "0.2.0" };
}

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

  // jsdom does not lay out, so these lock the constraint rather than the
  // geometry it produces. DESIGN.md: "The bar never wraps. Truncate
  // aggressively; rely on title attributes for full values."
  describe("never wraps out of its band", () => {
    it("clips the bar instead of letting it wrap or escape to the document", () => {
      render(<StatusBar />);
      const bar = screen.getByRole("contentinfo");
      // Without nowrap the multi-word hints wrapped inside a box pinned to
      // 28px; without overflow-hidden the spill reached html and put a
      // horizontal scrollbar on the whole shell.
      expect(bar.className).toContain("whitespace-nowrap");
      expect(bar.className).toContain("overflow-hidden");
    });

    it("lets the hint group shrink so the connection state never yields first", () => {
      render(<StatusBar />);
      const hints = screen.getByTestId("status-hints");
      // min-w-0 is what lets a flex item go below its min-content width; a
      // multi-word label's min-content is a whole word, which is what wrapped.
      expect(hints.className).toContain("min-w-0");
      expect(hints.className).toContain("overflow-hidden");
    });
  });

  describe("viewport tier", () => {
    it("keeps the keyboard hints at the desktop tier", () => {
      render(<StatusBar />);
      expect(screen.getByTestId("status-hints")).toBeInTheDocument();
    });

    it("keeps the keyboard hints at the tablet tier", () => {
      // A 900px browser window still has a keyboard, so the hints stay.
      stubTier("tablet");
      render(<StatusBar />);
      expect(screen.getByTestId("status-hints")).toBeInTheDocument();
    });

    it("drops the keyboard hints on a touch viewport", () => {
      stubTier("mobile");
      render(<StatusBar />);
      // There is no ⌘ key to press at 390px, and three hints crowd out the
      // connection state and server URL the bar exists to show.
      expect(screen.queryByTestId("status-hints")).toBeNull();
      expect(screen.getByTestId("status-connection")).toBeInTheDocument();
      expect(screen.getByTestId("status-server-url")).toBeInTheDocument();
    });
  });
  describe("UpgradeDot", () => {
    it.each(
      UPGRADE_STATES,
    )("takes the %s colour from the shared state palette, not a private copy", (tone, snapshotState, testid) => {
      showUpgrade(snapshotState);
      render(<StatusBar />);
      // Scoped to the version slot: the connection StateDot also carries
      // data-state and sits earlier in the bar.
      const slot = screen.getByTestId(testid);
      const dot = slot.querySelector("[data-state]") as HTMLElement;
      const kind = UPGRADE_TONE_KINDS[tone];
      expect(dot.dataset.state).toBe(kind);
      expect(dot.style.background).toBe(`var(${statePalette[kind].token})`);
    });

    it.each(
      UPGRADE_STATES,
    )("never animates the %s dot: the footer is always on screen", (_tone, snapshotState) => {
      showUpgrade(snapshotState);
      const { container } = render(<StatusBar />);
      expect(container.innerHTML).not.toMatch(/animate-/);
    });

    // Drift lock: every tone must name a real entry in the shared table, so a
    // renamed or deleted StateKind fails here instead of silently degrading to
    // the unknown-state glyph the way a private table would.
    it("maps every tone to a live state-palette entry", () => {
      const tones = Object.keys(UPGRADE_TONE_KINDS) as UpgradeTone[];
      expect(tones).toHaveLength(3);
      for (const tone of tones) {
        const entry = statePalette[UPGRADE_TONE_KINDS[tone]];
        expect(entry).toBeDefined();
        expect(entry.glyph).not.toBe("question");
      }
    });
  });
});
