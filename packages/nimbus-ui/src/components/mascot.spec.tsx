import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MASCOT_STATES, Mascot } from "./mascot";

function stubReducedMotion(matches: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query === "(prefers-reduced-motion: reduce)" ? matches : false,
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  }));
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("Mascot", () => {
  it("is an image named Nimbus at the asked width with the body aspect", () => {
    render(<Mascot size={48} />);
    const svg = screen.getByRole("img", { name: "Nimbus" });
    expect(svg).toHaveAttribute("width", "48");
    expect(svg).toHaveAttribute("height", "37");
    expect(svg).toHaveAttribute("viewBox", "0 0 120 92");
  });

  it("renders every state with a body, eyes and a mouth", () => {
    for (const state of MASCOT_STATES) {
      const { container, unmount } = render(<Mascot state={state} />);
      const svg = container.querySelector("svg") as SVGSVGElement;
      expect(svg).toHaveAttribute("data-state", state);
      expect(svg.querySelector("[data-part='body']")).not.toBeNull();
      expect(svg.querySelector("[data-part='eyes']")).not.toBeNull();
      expect(svg.querySelector("[data-part='mouth']")).not.toBeNull();
      unmount();
    }
  });

  it("gives each state its own accessory", () => {
    const accessories: Record<string, string | null> = {
      idle: null,
      working: "thinking",
      error: "drop",
      empty: "sleep",
      celebrate: "sparks",
      wink: null,
    };
    for (const [state, part] of Object.entries(accessories)) {
      const { container, unmount } = render(
        <Mascot state={state as (typeof MASCOT_STATES)[number]} />,
      );
      const svg = container.querySelector("svg") as SVGSVGElement;
      const extras = Array.from(svg.querySelectorAll("[data-part]"))
        .map((el) => el.getAttribute("data-part"))
        .filter((p) => p !== "body" && p !== "eyes" && p !== "mouth");
      expect(extras).toEqual(part ? [part] : []);
      unmount();
    }
  });

  it("blinks the open eyes when the system allows motion", () => {
    stubReducedMotion(false);
    const { container } = render(<Mascot state="idle" />);
    const eyes = container.querySelector("[data-part='eyes']") as SVGElement;
    expect(eyes).toHaveAttribute("data-blink", "true");
    expect(eyes.getAttribute("class")).toContain("animate-blink");
  });

  it("winks the right eye in idle, swapping the dot for the arc", () => {
    stubReducedMotion(false);
    const { container } = render(<Mascot state="idle" />);
    const eyes = container.querySelector("[data-part='eyes']") as SVGElement;
    expect(eyes).toHaveAttribute("data-wink", "true");
    // The two halves of the swap: the right dot leaves as the arc arrives.
    expect(eyes.querySelector(".animate-wink-open")).not.toBeNull();
    expect(eyes.querySelector(".animate-wink-shut")).toHaveAttribute(
      "d",
      "M66 54 q6 -7 12 0",
    );
  });

  it("winks only when idle", () => {
    stubReducedMotion(false);
    for (const state of MASCOT_STATES.filter((s) => s !== "idle")) {
      const { container, unmount } = render(<Mascot state={state} />);
      expect(container.querySelector("[data-wink]")).toBeNull();
      expect(container.querySelector(".animate-wink-shut")).toBeNull();
      unmount();
    }
  });

  it("holds the wink as a state, with one eye open and one arc", () => {
    const { container } = render(<Mascot state="wink" size={48} />);
    const eyes = container.querySelector("[data-part='eyes']") as SVGElement;
    expect(eyes.querySelectorAll("circle")).toHaveLength(1);
    expect(eyes.querySelector("circle")).toHaveAttribute("cx", "48");
    expect(eyes.querySelector("path")).toHaveAttribute("d", "M66 54 q6 -7 12 0");
    expect(eyes.getAttribute("class") ?? "").not.toContain("animate");
  });

  it("carries no animation under reduced motion", () => {
    stubReducedMotion(true);
    for (const state of MASCOT_STATES) {
      const { container, unmount } = render(<Mascot state={state} />);
      expect(container.querySelector("[data-blink]")).toBeNull();
      expect(container.querySelector("[data-wink]")).toBeNull();
      expect(container.querySelector("[class*='animate-']")).toBeNull();
      unmount();
    }
  });

  it("never blinks closed or crossed eyes", () => {
    stubReducedMotion(false);
    for (const state of ["error", "empty", "celebrate", "wink"] as const) {
      const { container, unmount } = render(<Mascot state={state} />);
      expect(container.querySelector("[data-blink]")).toBeNull();
      unmount();
    }
  });

  it("fills the body with the mark colour and inks the face, never the accent", () => {
    const { container } = render(<Mascot size={32} />);
    const body = container.querySelector("[data-part='body']") as SVGElement;
    expect(body).toHaveAttribute("fill", "var(--mark)");
    expect(body).not.toHaveAttribute("stroke");
    const eyes = container.querySelector("[data-part='eyes']") as SVGElement;
    expect(eyes).toHaveAttribute("fill", "var(--mark-ink)");
    expect(container.innerHTML).not.toContain("var(--accent)");
  });

  it("thickens the face below 40px", () => {
    for (const size of [16, 24, 32]) {
      const { container, unmount } = render(<Mascot size={size} />);
      expect(container.querySelector("[data-part='mouth']")).toHaveAttribute(
        "stroke-width",
        "5.5",
      );
      expect(
        container.querySelector("[data-part='eyes'] circle"),
      ).toHaveAttribute("r", "4.6");
      unmount();
    }
    const { container } = render(<Mascot size={40} />);
    expect(container.querySelector("[data-part='mouth']")).toHaveAttribute(
      "stroke-width",
      "4",
    );
  });

  it("can be decorative when the neighbouring text names it", () => {
    render(<Mascot decorative />);
    expect(screen.queryByRole("img")).toBeNull();
    expect(document.querySelector("svg")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
  });
});
