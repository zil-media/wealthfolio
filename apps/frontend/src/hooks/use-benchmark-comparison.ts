import { calculatePerformanceHistory } from "@/adapters";
import { PERFORMANCE_CHART_COLORS } from "@/components/performance-chart-colors";
import {
  buildBenchmarkShadowSeries,
  buildContributionNeutralSeries,
  type BenchmarkShadowPoint,
  type ContributionNeutralPoint,
  type ValuationPoint,
} from "@/lib/benchmark-compare";
import { QueryKeys } from "@/lib/query-keys";
import type { PerformanceResult } from "@/lib/types";
import { keepPreviousData, useQueries } from "@tanstack/react-query";
import { usePersistentState } from "@wealthfolio/ui";
import { useCallback, useMemo } from "react";

export interface BenchmarkItem {
  id: string;
  name: string;
  /** Color slot, kept for the life of the selection so survivors never repaint. */
  slot: number;
}

export const MAX_COMPARED_BENCHMARKS = 3;
// Blue, magenta, orange from the performance palette: distinct from each other and from
// the portfolio's green area in both themes.
const BENCHMARK_SLOT_COLORS = [
  PERFORMANCE_CHART_COLORS[0],
  PERFORMANCE_CHART_COLORS[1],
  PERFORMANCE_CHART_COLORS[6],
] as const;
const DEFAULT_BENCHMARKS: BenchmarkItem[] = [{ id: "^GSPC", name: "S&P 500", slot: 0 }];
const SELECTION_STORAGE_KEY = "benchmark-compare-selection";

export function benchmarkColor(slot: number): string {
  return BENCHMARK_SLOT_COLORS[slot] ?? BENCHMARK_SLOT_COLORS[0];
}

function isBenchmarkItem(value: unknown): value is BenchmarkItem {
  const item = value as BenchmarkItem;
  return (
    !!item &&
    typeof item.id === "string" &&
    item.id.length > 0 &&
    typeof item.name === "string" &&
    Number.isInteger(item.slot) &&
    item.slot >= 0 &&
    item.slot < MAX_COMPARED_BENCHMARKS
  );
}

export function useBenchmarkSelection() {
  const [stored, setStored] = usePersistentState<BenchmarkItem[]>(
    SELECTION_STORAGE_KEY,
    DEFAULT_BENCHMARKS,
  );
  const benchmarks = useMemo(
    () =>
      Array.isArray(stored)
        ? stored.filter(isBenchmarkItem).slice(0, MAX_COMPARED_BENCHMARKS)
        : DEFAULT_BENCHMARKS,
    [stored],
  );

  const addBenchmark = useCallback(
    (symbol: { id: string; name: string }) => {
      if (benchmarks.length >= MAX_COMPARED_BENCHMARKS) return;
      if (benchmarks.some((item) => item.id === symbol.id)) return;
      const used = new Set(benchmarks.map((item) => item.slot));
      const slot = [0, 1, 2].find((candidate) => !used.has(candidate)) ?? 0;
      setStored([...benchmarks, { id: symbol.id, name: symbol.name, slot }]);
    },
    [benchmarks, setStored],
  );

  const removeBenchmark = useCallback(
    (id: string) => setStored(benchmarks.filter((item) => item.id !== id)),
    [benchmarks, setStored],
  );

  return { benchmarks, addBenchmark, removeBenchmark };
}

export type BenchmarkSeriesStatus = "loading" | "ready" | "unavailable";

export interface BenchmarkSeries {
  benchmark: BenchmarkItem;
  color: string;
  status: BenchmarkSeriesStatus;
  /** Why the index can't be shown, when the backend or the query said so. */
  reason?: string;
  shadow: BenchmarkShadowPoint[] | null;
  performance?: PerformanceResult;
}

export interface BenchmarkComparison {
  neutral: ContributionNeutralPoint[];
  series: BenchmarkSeries[];
}

/**
 * Rebases the valuation history without deposits and replays the same flows into each
 * selected index, over exactly the dates the chart shows.
 */
export function useBenchmarkComparison(
  points: readonly ValuationPoint[],
  benchmarks: readonly BenchmarkItem[],
): BenchmarkComparison {
  const neutral = useMemo(() => buildContributionNeutralSeries(points), [points]);
  const startDate = neutral[0]?.date;
  const endDate = neutral[neutral.length - 1]?.date;

  const queries = useQueries({
    queries: benchmarks.map((benchmark) => ({
      queryKey: [
        QueryKeys.PERFORMANCE_HISTORY,
        "symbol",
        benchmark.id,
        undefined,
        startDate,
        endDate,
        undefined,
      ],
      queryFn: () => calculatePerformanceHistory("symbol", benchmark.id, startDate, endDate),
      enabled: !!startDate && !!endDate,
      staleTime: 5 * 60 * 1000,
      retry: false,
      placeholderData: keepPreviousData,
    })),
  });

  const series = benchmarks.map((benchmark, index): BenchmarkSeries => {
    const query = queries[index];
    const color = benchmarkColor(benchmark.slot);
    if (!query || query.isLoading) {
      return { benchmark, color, status: "loading", shadow: null };
    }
    if (query.isError || !query.data) {
      const reason = query.error instanceof Error ? query.error.message : undefined;
      return { benchmark, color, status: "unavailable", reason, shadow: null };
    }
    const shadow = buildBenchmarkShadowSeries(neutral, query.data.series ?? []);
    return shadow
      ? { benchmark, color, status: "ready", shadow, performance: query.data }
      : {
          benchmark,
          color,
          status: "unavailable",
          reason: query.data.dataQuality?.notApplicableReasons?.[0],
          shadow: null,
        };
  });

  return { neutral, series };
}
