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
  it("is an image named Nimbus at the asked width, fitted to the body", () => {
    render(<Mascot size={48} />);
    const svg = screen.getByRole("img", { name: "Nimbus" });
    expect(svg).toHaveAttribute("width", "48");
    expect(svg).toHaveAttribute("height", "40");
    expect(svg).toHaveAttribute("viewBox", "12 8 96 80");
  });

  // The fitted box is derived from where the body actually draws, so the two
  // have to stay in step: move a lobe and the crop has to move with it. This
  // reads the bounds back off the shapes and checks that the crop still frames
  // them with the 2-across, 4-down margin it claims. Condition 19 of
  // `scripts/verify-nimbus-docs-site.sh` holds the website copy to the same
  // shapes and the same crop, so this covers both drawings.
  it("fits the box to the bounds the body actually draws", () => {
    const { container } = render(<Mascot />);
    const svg = container.querySelector("svg") as SVGSVGElement;
    const num = (el: Element, name: string) => Number(el.getAttribute(name));
    let left = Infinity;
    let top = Infinity;
    let right = -Infinity;
    let bottom = -Infinity;
    for (const shape of svg.querySelectorAll("[data-part='body'] > *")) {
      const box =
        shape.tagName === "circle"
          ? {
              x: num(shape, "cx") - num(shape, "r"),
              y: num(shape, "cy") - num(shape, "r"),
              w: 2 * num(shape, "r"),
              h: 2 * num(shape, "r"),
            }
          : {
              x: num(shape, "x"),
              y: num(shape, "y"),
              w: num(shape, "width"),
              h: num(shape, "height"),
            };
      left = Math.min(left, box.x);
      top = Math.min(top, box.y);
      right = Math.max(right, box.x + box.w);
      bottom = Math.max(bottom, box.y + box.h);
    }
    expect([left, top, right - left, bottom - top]).toEqual([14, 12, 92, 72]);
    expect(svg.getAttribute("viewBox")).toBe(
      `${left - 2} ${top - 4} ${right - left + 4} ${bottom - top + 8}`,
    );
  });

  it("reserves the accessory room for every state that draws outside the body", () => {
    for (const state of MASCOT_STATES.filter(
      (s) => s !== "idle" && s !== "wink",
    )) {
      const { container, unmount } = render(
        <Mascot state={state} size={48} />,
      );
      const svg = container.querySelector("svg") as SVGSVGElement;
      expect(svg).toHaveAttribute("viewBox", "0 0 120 92");
      expect(svg).toHaveAttribute("height", "37");
      unmount();
    }
  });

  it("reserves the accessory room on request, so a changing state keeps one box", () => {
    for (const state of MASCOT_STATES) {
      const { container, unmount } = render(
        <Mascot state={state} size={48} reserveAccessories />,
      );
      const svg = container.querySelector("svg") as SVGSVGElement;
      expect(svg).toHaveAttribute("viewBox", "0 0 120 92");
      expect(svg).toHaveAttribute("height", "37");
      unmount();
    }
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

  // The rule is the drawn scale, not the width: the face thickens below a
  // third of a pixel per viewBox unit, so the boundary is a third of the box
  // the state uses -- 32px fitted, 40px reserved. A fitted mark and a reserved
  // one of the same drawn size therefore carry the same face.
  it("thickens the face below a third of a pixel per viewBox unit", () => {
    const thick = (size: number, reserve: boolean) => {
      const { container, unmount } = render(
        <Mascot size={size} reserveAccessories={reserve} />,
      );
      const width = container
        .querySelector("[data-part='mouth']")
        ?.getAttribute("stroke-width");
      const radius = container
        .querySelector("[data-part='eyes'] circle")
        ?.getAttribute("r");
      unmount();
      return `${width}/${radius}`;
    };
    for (const size of [16, 24, 31]) {
      expect(thick(size, false)).toBe("5.5/4.6");
    }
    expect(thick(32, false)).toBe("4/3.7");
    for (const size of [16, 24, 32, 39]) {
      expect(thick(size, true)).toBe("5.5/4.6");
    }
    expect(thick(40, true)).toBe("4/3.7");
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
