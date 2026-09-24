import { BenchmarkSymbolSelector } from "@/components/benchmark-symbol-selector";
import { BenchmarkSymbolSelectorMobile } from "@/components/benchmark-symbol-selector-mobile";
import { useIsMobileViewport } from "@/hooks/use-platform";
import { MAX_COMPARED_BENCHMARKS, type BenchmarkSeries } from "@/hooks/use-benchmark-comparison";
import { cn } from "@/lib/utils";
import { Icons, useNumberFormatting } from "@wealthfolio/ui";
import { Tooltip, TooltipContent, TooltipTrigger } from "@wealthfolio/ui/components/ui/tooltip";
import { useTranslation } from "react-i18next";

interface BenchmarkCompareBarProps {
  series: BenchmarkSeries[];
  onAdd: (symbol: { id: string; name: string }) => void;
  onRemove: (id: string) => void;
  /** Show each index's return on capital next to its name. */
  showReturns?: boolean;
  className?: string;
}

export function BenchmarkCompareBar({
  series,
  onAdd,
  onRemove,
  showReturns = false,
  className,
}: BenchmarkCompareBarProps) {
  const { t } = useTranslation();
  const isMobile = useIsMobileViewport();
  const { formatPercent } = useNumberFormatting();
  const canAdd = series.length < MAX_COMPARED_BENCHMARKS;

  return (
    <div className={cn("flex flex-wrap items-center gap-1.5", className)}>
      <span className="text-muted-foreground inline-flex items-center gap-1.5 pr-1 text-xs">
        <span className="bg-success block h-0.5 w-3.5 rounded-full" />
        {t("common:benchmark_compare.your_portfolio")}
      </span>
      <span className="text-muted-foreground text-xs">{t("common:benchmark_compare.vs")}</span>
      {series.map(({ benchmark, color, status, reason, shadow }) => {
        const last = shadow?.[shadow.length - 1]?.returnOnCapital;
        const chip = (
          <span
            key={benchmark.id}
            className="bg-background/60 inline-flex h-7 items-center gap-1.5 rounded-full border pl-2.5 pr-1 text-xs"
          >
            <span className="block size-2 rounded-full" style={{ backgroundColor: color }} />
            <span className="font-medium">{benchmark.name}</span>
            {status === "loading" && (
              <Icons.Spinner className="text-muted-foreground size-3 animate-spin" />
            )}
            {status === "unavailable" && <Icons.AlertTriangle className="text-warning size-3" />}
            {status === "ready" && showReturns && last != null && (
              <span className="text-muted-foreground tabular-nums">
                {formatPercent(last, { signDisplay: "exceptZero" })}
              </span>
            )}
            <button
              type="button"
              onClick={() => onRemove(benchmark.id)}
              aria-label={t("common:benchmark_compare.remove", { name: benchmark.name })}
              className="text-muted-foreground hover:text-foreground hover:bg-muted rounded-full p-0.5"
            >
              <Icons.X className="size-3" />
            </button>
          </span>
        );
        return status === "unavailable" ? (
          <Tooltip key={benchmark.id}>
            <TooltipTrigger asChild>{chip}</TooltipTrigger>
            <TooltipContent className="max-w-xs text-xs">
              {reason ?? t("common:benchmark_compare.no_data")}
            </TooltipContent>
          </Tooltip>
        ) : (
          chip
        );
      })}
      {canAdd &&
        (isMobile ? (
          <BenchmarkSymbolSelectorMobile
            onSelect={onAdd}
            iconOnly
            className="size-7 rounded-full"
          />
        ) : (
          <BenchmarkSymbolSelector onSelect={onAdd} iconOnly className="size-7 rounded-full" />
        ))}
    </div>
  );
}
