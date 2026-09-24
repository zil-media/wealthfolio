import type { BenchmarkSeries } from "@/hooks/use-benchmark-comparison";
import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import { useIsMobileViewport } from "@/hooks/use-platform";
import type { ContributionNeutralPoint } from "@/lib/benchmark-compare";
import { AmountDisplay, useDateFormatting, useNumberFormatting } from "@wealthfolio/ui";
import { ChartContainer, type ChartConfig } from "@wealthfolio/ui/components/ui/chart";
import { useId, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Area, ComposedChart, Line, ReferenceLine, Tooltip, XAxis, YAxis } from "recharts";

interface ContributionNeutralChartProps {
  neutral: ContributionNeutralPoint[];
  series: BenchmarkSeries[];
  currency: string;
}

type ChartRow = Record<string, number | string | null> & { date: string };

const portfolioKey = "portfolio";
const benchmarkKey = (slot: number) => `benchmark${slot}`;

interface TooltipProps {
  active?: boolean;
  payload?: { payload?: ChartRow }[];
  series: BenchmarkSeries[];
  neutral: ContributionNeutralPoint[];
  currency: string;
}

function NeutralTooltip({ active, payload, series, neutral, currency }: TooltipProps) {
  const { t } = useTranslation();
  const { formatPercent } = useNumberFormatting();
  const dateFormatting = useDateFormatting();
  const { isBalanceHidden } = useBalancePrivacy();
  const row = payload?.[0]?.payload;
  if (!active || !row) return null;
  const point = neutral[row.index as number];
  if (!point) return null;

  return (
    <div className="bg-popover pointer-events-none grid min-w-52 grid-cols-1 gap-1.5 rounded-md border p-2 text-xs shadow-md">
      <p className="text-muted-foreground">
        {dateFormatting.formatCalendarDate(point.date, {
          year: "numeric",
          month: "short",
          day: "numeric",
        })}
      </p>
      <div className="flex items-center justify-between gap-3">
        <span className="text-muted-foreground flex items-center gap-1.5">
          <span className="bg-success block h-0.5 w-3" />
          {t("common:benchmark_compare.your_portfolio")}
        </span>
        <span className="font-semibold tabular-nums">
          {formatPercent(point.returnOnCapital, { signDisplay: "exceptZero" })}
        </span>
      </div>
      <div className="flex items-center justify-between gap-3">
        <span className="text-muted-foreground pl-[18px]">
          {t("common:benchmark_compare.gain")}
        </span>
        <AmountDisplay
          value={point.gain}
          currency={currency}
          isHidden={isBalanceHidden}
          className="font-semibold"
        />
      </div>
      {point.flow !== 0 && (
        <div className="flex items-center justify-between gap-3">
          <span className="text-muted-foreground flex items-center gap-1.5">
            <span className="block h-0 w-3 border-b-2 border-dashed border-[var(--muted-foreground)]" />
            {point.flow > 0
              ? t("common:benchmark_compare.deposit_excluded")
              : t("common:benchmark_compare.withdrawal_excluded")}
          </span>
          <AmountDisplay
            value={point.flow}
            currency={currency}
            isHidden={isBalanceHidden}
            className="font-semibold"
          />
        </div>
      )}
      {series.map(({ benchmark, color, shadow }) => {
        const value = shadow?.[row.index as number]?.returnOnCapital;
        if (value == null) return null;
        return (
          <div key={benchmark.id} className="flex items-center justify-between gap-3">
            <span className="text-muted-foreground flex items-center gap-1.5">
              <span className="block h-0.5 w-3" style={{ backgroundColor: color }} />
              {benchmark.name}
            </span>
            <span className="font-semibold tabular-nums">
              {formatPercent(value, { signDisplay: "exceptZero" })}
            </span>
          </div>
        );
      })}
    </div>
  );
}

/**
 * Return on capital over time with deposits and withdrawals removed, plus each selected
 * index replaying the same cash flows.
 */
export function ContributionNeutralChart({
  neutral,
  series,
  currency,
}: ContributionNeutralChartProps) {
  const { t } = useTranslation();
  const { formatPercent } = useNumberFormatting();
  const isMobile = useIsMobileViewport();
  const fillId = `neutralFill-${useId()}`;
  const readySeries = series.filter((item) => item.shadow);

  const rows = useMemo<ChartRow[]>(
    () =>
      neutral.map((point, index) => {
        const row: ChartRow = { date: point.date, index, [portfolioKey]: point.returnOnCapital };
        for (const item of readySeries) {
          row[benchmarkKey(item.benchmark.slot)] = item.shadow?.[index]?.returnOnCapital ?? null;
        }
        return row;
      }),
    [neutral, readySeries],
  );

  const { domain, ticks, tickDigits } = useMemo(() => {
    let lo = 0;
    let hi = 0;
    for (const row of rows) {
      for (const [key, value] of Object.entries(row)) {
        if (key === "date" || key === "index" || typeof value !== "number") continue;
        if (value < lo) lo = value;
        if (value > hi) hi = value;
      }
    }
    // Round ticks to 1/2/5 steps so the axis reads 5%, 10%, 15% rather than 8.9%.
    const raw = Math.max(hi - lo, 0.01) / 4;
    const magnitude = 10 ** Math.floor(Math.log10(raw));
    const step = [1, 2, 5, 10].map((m) => m * magnitude).find((s) => s >= raw) ?? raw;
    const pad = step * 0.25;
    const values: number[] = [];
    for (let v = Math.ceil(lo / step) * step; v <= hi + 1e-9; v += step) values.push(v);
    return {
      domain: [lo - pad, hi + pad] as [number, number],
      ticks: values,
      tickDigits: step >= 0.01 ? 0 : 1,
    };
  }, [rows]);

  const chartConfig = useMemo(() => {
    const config: ChartConfig = {
      [portfolioKey]: { label: t("common:benchmark_compare.your_portfolio") },
    };
    for (const item of readySeries) {
      config[benchmarkKey(item.benchmark.slot)] = { label: item.benchmark.name, color: item.color };
    }
    return config;
  }, [readySeries, t]);

  if (neutral.length === 0) return null;

  return (
    <ChartContainer config={chartConfig} className="h-full w-full" data-no-swipe-drag>
      <ComposedChart data={rows} margin={{ top: 8, right: 0, left: 0, bottom: 0 }}>
        <defs>
          <linearGradient id={fillId} x1="0" y1="0" x2="0" y2="1">
            <stop offset="5%" stopColor="var(--success)" stopOpacity={0.2} />
            <stop offset="95%" stopColor="var(--success)" stopOpacity={0.02} />
          </linearGradient>
        </defs>
        <XAxis hide dataKey="date" type="category" />
        <YAxis
          orientation="right"
          mirror
          type="number"
          domain={domain}
          ticks={ticks}
          tickLine={false}
          axisLine={false}
          tick={{ fontSize: 11, fill: "var(--muted-foreground)" }}
          tickFormatter={(value: number) => formatPercent(value, { digits: tickDigits })}
        />
        <ReferenceLine y={0} stroke="var(--border)" strokeDasharray="4 4" />
        <Tooltip
          position={isMobile ? { y: 60 } : { y: -20 }}
          cursor={{ stroke: "var(--border)", strokeWidth: 1, pointerEvents: "none" }}
          wrapperStyle={{ pointerEvents: "none", zIndex: 30 }}
          content={(props) => (
            <NeutralTooltip
              {...(props as unknown as Pick<TooltipProps, "active" | "payload">)}
              series={readySeries}
              neutral={neutral}
              currency={currency}
            />
          )}
        />
        <Area
          type="monotone"
          dataKey={portfolioKey}
          stroke="var(--success)"
          strokeWidth={2}
          fill={`url(#${fillId})`}
          baseValue={0}
          connectNulls
          isAnimationActive
          animationDuration={300}
          dot={false}
          activeDot={{ r: 4, fill: "var(--success)", stroke: "var(--background)", strokeWidth: 2 }}
        />
        {readySeries.map((item) => (
          <Line
            key={item.benchmark.id}
            type="monotone"
            dataKey={benchmarkKey(item.benchmark.slot)}
            stroke={item.color}
            strokeWidth={1.5}
            dot={false}
            connectNulls
            isAnimationActive
            animationDuration={300}
            activeDot={{ r: 3.5, fill: item.color, stroke: "var(--background)", strokeWidth: 2 }}
          />
        ))}
      </ComposedChart>
    </ChartContainer>
  );
}
