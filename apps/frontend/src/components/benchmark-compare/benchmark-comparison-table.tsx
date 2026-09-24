import type { BenchmarkSeries } from "@/hooks/use-benchmark-comparison";
import type { ContributionNeutralPoint } from "@/lib/benchmark-compare";
import type { PerformanceResult } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useNumberFormatting } from "@wealthfolio/ui";
import { useTranslation } from "react-i18next";
import { Fragment, type ReactNode } from "react";

interface BenchmarkComparisonTableProps {
  neutral: ContributionNeutralPoint[];
  series: BenchmarkSeries[];
  /** The account's own performance for the same period (TWR, volatility, drawdown). */
  performance?: PerformanceResult | null;
  /** Index picker rendered above the table. */
  controls?: ReactNode;
  className?: string;
}

type Tone = "signed" | "neutral" | "drawdown";

export function BenchmarkComparisonTable({
  neutral,
  series,
  performance,
  controls,
  className,
}: BenchmarkComparisonTableProps) {
  const { t } = useTranslation();
  const { formatPercent } = useNumberFormatting();
  const portfolioReturn = neutral[neutral.length - 1]?.returnOnCapital ?? null;

  const cell = (value: number | null | undefined, tone: Tone, suffix?: string) => {
    if (value == null) {
      return <td className="text-muted-foreground px-1.5 py-1.5 text-right">—</td>;
    }
    const color =
      tone === "neutral"
        ? ""
        : tone === "drawdown"
          ? value < 0
            ? "text-destructive"
            : ""
          : value < 0
            ? "text-destructive"
            : value > 0
              ? "text-success"
              : "";
    const formatted = formatPercent(
      value,
      tone === "signed" ? { signDisplay: "exceptZero" } : undefined,
    );
    // Percentage points: a difference between two returns, not a return itself.
    const text = suffix === "pp" ? formatted.replace(/\s?%/, " pp") : formatted;
    return (
      <td
        className={cn("whitespace-nowrap px-1.5 py-1.5 text-right font-medium tabular-nums", color)}
      >
        {text}
      </td>
    );
  };

  const pending = (item: BenchmarkSeries) => (
    <td key={item.benchmark.id} className="text-muted-foreground px-1.5 py-1.5 text-right">
      {item.status === "loading" ? "…" : t("common:benchmark_compare.no_data")}
    </td>
  );

  const rows: {
    label: string;
    portfolio: number | null | undefined;
    benchmark: (item: BenchmarkSeries) => number | null | undefined;
    tone: Tone;
  }[] = [
    {
      label: t("common:benchmark_compare.return_on_capital"),
      portfolio: portfolioReturn,
      benchmark: (item) => item.shadow?.[item.shadow.length - 1]?.returnOnCapital,
      tone: "signed",
    },
    {
      label: t("common:benchmark_compare.twr_or_index"),
      portfolio: performance?.returns.twr,
      benchmark: (item) => item.performance?.returns.valueReturn,
      tone: "signed",
    },
    {
      label: t("common:benchmark_compare.volatility"),
      portfolio: performance?.risk.volatility,
      benchmark: (item) => item.performance?.risk.volatility,
      tone: "neutral",
    },
    {
      label: t("common:benchmark_compare.max_drawdown"),
      portfolio: performance?.risk.maxDrawdown,
      benchmark: (item) => item.performance?.risk.maxDrawdown,
      tone: "drawdown",
    },
  ];

  return (
    <div className={cn("space-y-2", className)}>
      {controls}
      {series.length > 0 && (
        <div className="border-muted/30 bg-muted/30 overflow-x-auto rounded-md border">
          <table className="w-full min-w-[320px] text-xs">
            <thead>
              <tr className="text-muted-foreground border-b">
                <th className="px-1.5 py-1.5 text-left font-normal" />
                <th className="whitespace-nowrap px-1.5 py-1.5 text-right font-medium">
                  <span className="inline-flex items-center gap-1.5">
                    <span className="bg-success block h-0.5 w-3" />
                    {t("common:benchmark_compare.this_scope")}
                  </span>
                </th>
                {series.map((item) => (
                  <th
                    key={item.benchmark.id}
                    className="whitespace-nowrap px-1.5 py-1.5 text-right font-medium"
                  >
                    <span className="inline-flex items-center gap-1.5">
                      <span
                        className="block size-2 rounded-full"
                        style={{ backgroundColor: item.color }}
                      />
                      {item.benchmark.name}
                    </span>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.label} className="border-b last:border-0">
                  <td className="text-muted-foreground px-1.5 py-1.5">{row.label}</td>
                  {cell(row.portfolio, row.tone)}
                  {series.map((item) =>
                    item.status === "ready" ? (
                      <Fragment key={item.benchmark.id}>
                        {cell(row.benchmark(item), row.tone)}
                      </Fragment>
                    ) : (
                      pending(item)
                    ),
                  )}
                </tr>
              ))}
              <tr className="bg-muted/40">
                <td className="text-muted-foreground px-1.5 py-1.5">
                  {t("common:benchmark_compare.difference")}
                </td>
                <td className="text-muted-foreground px-1.5 py-1.5 text-right">—</td>
                {series.map((item) => {
                  const benchmarkReturn = item.shadow?.[item.shadow.length - 1]?.returnOnCapital;
                  if (item.status !== "ready") return pending(item);
                  return (
                    <Fragment key={item.benchmark.id}>
                      {cell(
                        portfolioReturn != null && benchmarkReturn != null
                          ? portfolioReturn - benchmarkReturn
                          : null,
                        "signed",
                        "pp",
                      )}
                    </Fragment>
                  );
                })}
              </tr>
            </tbody>
          </table>
        </div>
      )}
      <p className="text-muted-foreground text-[11px] leading-snug">
        {t("common:benchmark_compare.method_note")}
      </p>
    </div>
  );
}
