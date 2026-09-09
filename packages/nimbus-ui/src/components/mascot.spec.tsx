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
    expect(svg).toHaveAttribute("data-mascot", "outline");
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

  it("carries no animation under reduced motion", () => {
    stubReducedMotion(true);
    for (const state of MASCOT_STATES) {
      const { container, unmount } = render(<Mascot state={state} />);
      expect(container.querySelector("[data-blink]")).toBeNull();
      expect(container.querySelector(".animate-blink")).toBeNull();
      unmount();
    }
  });

  it("never blinks closed or crossed eyes", () => {
    stubReducedMotion(false);
    for (const state of ["error", "empty", "celebrate"] as const) {
      const { container, unmount } = render(<Mascot state={state} />);
      expect(container.querySelector("[data-blink]")).toBeNull();
      unmount();
    }
  });

  it("solid variant fills the body with the accent and inks the face", () => {
    const { container } = render(<Mascot variant="solid" size={32} />);
    const body = container.querySelector("[data-part='body']") as SVGElement;
    expect(body).toHaveAttribute("fill", "var(--accent)");
    const eyes = container.querySelector("[data-part='eyes']") as SVGElement;
    expect(eyes).toHaveAttribute("fill", "var(--accent-ink)");
    expect(container.querySelector("svg")).toHaveAttribute(
      "data-mascot",
      "solid",
    );
  });

  it("outline variant strokes the body in currentColor on the panel ground", () => {
    const { container } = render(<Mascot size={48} />);
    const body = container.querySelector("[data-part='body']") as SVGElement;
    expect(body).toHaveAttribute("stroke", "currentColor");
    expect(body).toHaveAttribute("stroke-width", "5");
    expect(container.querySelector("[fill='var(--bg-panel)']")).not.toBeNull();
  });

  it("thickens the outline and the face below 40px", () => {
    for (const size of [16, 24, 32]) {
      const { container, unmount } = render(<Mascot size={size} />);
      expect(container.querySelector("[data-part='body']")).toHaveAttribute(
        "stroke-width",
        "7",
      );
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
    expect(container.querySelector("[data-part='body']")).toHaveAttribute(
      "stroke-width",
      "5",
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
