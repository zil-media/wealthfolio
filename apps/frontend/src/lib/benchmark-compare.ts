import type { ReturnData } from "@/lib/types";

export interface ValuationPoint {
  date: string;
  totalValue: number;
  netContribution: number;
}

export interface ContributionNeutralPoint {
  date: string;
  totalValue: number;
  /** Change in net contribution on this date (deposit > 0, withdrawal < 0). */
  flow: number;
  /** Value minus net contribution, measured from the first point. Flows never count as gain. */
  gain: number;
  /** Opening value plus every deposit made up to this date. */
  capital: number;
  /** gain / capital, so it always carries the same sign as the gain. */
  returnOnCapital: number | null;
}

export interface BenchmarkShadowPoint {
  date: string;
  gain: number;
  returnOnCapital: number | null;
  /** Price index relative to the first point (1 = start). */
  priceIndex: number;
}

/**
 * Rebases a valuation history so deposits and withdrawals disappear from the curve.
 * The first point is the base: its value is the opening capital and its gain is zero.
 */
export function buildContributionNeutralSeries(
  points: readonly ValuationPoint[],
): ContributionNeutralPoint[] {
  if (points.length === 0) return [];
  const base = points[0];
  const baseGain = base.totalValue - base.netContribution;
  let deposits = 0;

  return points.map((point, index) => {
    const flow = index === 0 ? 0 : point.netContribution - points[index - 1].netContribution;
    if (flow > 0) deposits += flow;
    const gain = point.totalValue - point.netContribution - baseGain;
    const capital = base.totalValue + deposits;
    return {
      date: point.date,
      totalValue: point.totalValue,
      flow,
      gain,
      capital,
      returnOnCapital: capital > 0 ? gain / capital : null,
    };
  });
}

/**
 * Replays the same cash flows into a benchmark: the opening value and every deposit or
 * withdrawal buy or sell the index on the same date. `benchmarkSeries` is the cumulative
 * price return the symbol performance endpoint returns (0 on its first date).
 * Dates before the first benchmark quote use that first quote; later gaps carry the last close.
 */
export function buildBenchmarkShadowSeries(
  neutral: readonly ContributionNeutralPoint[],
  benchmarkSeries: readonly ReturnData[],
): BenchmarkShadowPoint[] | null {
  if (neutral.length === 0 || benchmarkSeries.length === 0) return null;
  const sorted = [...benchmarkSeries].sort((a, b) => a.date.localeCompare(b.date));

  let cursor = 0;
  const levels = neutral.map((point) => {
    while (cursor + 1 < sorted.length && sorted[cursor + 1].date <= point.date) cursor++;
    return 1 + sorted[cursor].value;
  });
  if (levels.some((level) => !(level > 0))) return null;

  const base = neutral[0];
  let units = base.totalValue / levels[0];
  let cumulativeFlow = 0;

  return neutral.map((point, index) => {
    if (index > 0) {
      units += point.flow / levels[index];
      cumulativeFlow += point.flow;
    }
    const gain = units * levels[index] - base.totalValue - cumulativeFlow;
    return {
      date: point.date,
      gain,
      returnOnCapital: point.capital > 0 ? gain / point.capital : null,
      priceIndex: levels[index] / levels[0],
    };
  });
}
