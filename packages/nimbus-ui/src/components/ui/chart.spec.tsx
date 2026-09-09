import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AreaChart, BarChart, CHART, Sparkline } from "./chart";

const DATA = [
  { label: "09:00", value: 4 },
  { label: "09:05", value: 9 },
  { label: "09:10", value: 6 },
];

describe("chart seam", () => {
  it("renders each shape as a labelled figure", () => {
    render(
      <>
        <Sparkline data={DATA} ariaLabel="Requests, last 15 minutes" />
        <AreaChart data={DATA} ariaLabel="Requests over time" />
        <BarChart data={DATA} ariaLabel="Requests per bucket" />
      </>,
    );
    expect(
      screen.getByLabelText("Requests, last 15 minutes"),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Requests over time")).toBeInTheDocument();
    expect(screen.getByLabelText("Requests per bucket")).toBeInTheDocument();
  });

  it("never offers the accent as a series colour", () => {
    for (const value of Object.values(CHART)) {
      expect(value).not.toContain("--accent");
    }
  });

  it("takes a per-datum colour for bars", () => {
    render(
      <BarChart
        data={DATA}
        color={(point) => (point.value > 8 ? CHART.error : CHART.neutral)}
        ariaLabel="Errors per bucket"
      />,
    );
    expect(screen.getByLabelText("Errors per bucket")).toBeInTheDocument();
  });
});
