import { describe, expect, it } from "vitest";
import { buildBenchmarkShadowSeries, buildContributionNeutralSeries } from "./benchmark-compare";

describe("buildContributionNeutralSeries", () => {
  it("keeps deposits out of the gain and measures return on total capital", () => {
    // Cripto (Tangem) YTD 2026: 21.7k fell to 14k, then a 71.2k deposit, ending at 96.2k.
    const series = buildContributionNeutralSeries([
      { date: "2025-12-31", totalValue: 21680.97, netContribution: 21537.42 },
      { date: "2026-08-18", totalValue: 14004.1, netContribution: 21537.42 },
      { date: "2026-08-19", totalValue: 87679.1, netContribution: 92764.49 },
      { date: "2026-09-24", totalValue: 96166.49, netContribution: 92764.49 },
    ]);

    const last = series[series.length - 1];
    expect(series[0].gain).toBe(0);
    expect(series[2].flow).toBeCloseTo(71227.07, 2);
    expect(last.gain).toBeCloseTo(3258.45, 2);
    expect(last.capital).toBeCloseTo(21680.97 + 71227.07, 2);
    expect(last.returnOnCapital).toBeCloseTo(3258.45 / 92908.04, 6);
    expect(last.returnOnCapital).toBeGreaterThan(0);
  });

  it("does not grow capital on withdrawals", () => {
    const series = buildContributionNeutralSeries([
      { date: "2026-01-01", totalValue: 1000, netContribution: 1000 },
      { date: "2026-01-02", totalValue: 600, netContribution: 500 },
    ]);
    expect(series[1].flow).toBe(-500);
    expect(series[1].gain).toBe(100);
    expect(series[1].capital).toBe(1000);
    expect(series[1].returnOnCapital).toBeCloseTo(0.1);
  });

  it("returns null return when there is no capital yet", () => {
    const series = buildContributionNeutralSeries([
      { date: "2026-01-01", totalValue: 0, netContribution: 0 },
    ]);
    expect(series[0].returnOnCapital).toBeNull();
  });
});

describe("buildBenchmarkShadowSeries", () => {
  const neutral = buildContributionNeutralSeries([
    { date: "2026-01-01", totalValue: 1000, netContribution: 1000 },
    { date: "2026-01-02", totalValue: 1000, netContribution: 1000 },
    { date: "2026-01-03", totalValue: 2000, netContribution: 2000 },
    { date: "2026-01-04", totalValue: 2000, netContribution: 2000 },
  ]);

  it("invests the opening value and each deposit in the index on the same day", () => {
    const shadow = buildBenchmarkShadowSeries(neutral, [
      { date: "2026-01-01", value: 0 },
      { date: "2026-01-02", value: 0.1 },
      // 2026-01-03 missing: carries the 2026-01-02 close
      { date: "2026-01-04", value: 0.21 },
    ]);

    expect(shadow).not.toBeNull();
    const points = shadow!;
    expect(points[1].gain).toBeCloseTo(100);
    expect(points[2].gain).toBeCloseTo(100);
    // 1000 bought at 1.0 and 1000 bought at 1.1, both marked at 1.21
    expect(points[3].gain).toBeCloseTo(1000 * 1.21 + (1000 / 1.1) * 1.21 - 2000);
    expect(points[3].returnOnCapital).toBeCloseTo(points[3].gain / 2000);
    expect(points[3].priceIndex).toBeCloseTo(1.21);
  });

  it("uses the first quote for dates before the index series starts", () => {
    const shadow = buildBenchmarkShadowSeries(neutral, [{ date: "2026-01-03", value: 0 }]);
    expect(shadow!.every((point) => point.gain === 0)).toBe(true);
  });

  it("returns null without benchmark data", () => {
    expect(buildBenchmarkShadowSeries(neutral, [])).toBeNull();
  });
});
