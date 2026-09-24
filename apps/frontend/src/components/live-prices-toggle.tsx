import { LIVE_REFRESH_MS } from "@/hooks/use-live-holdings";
import { cn } from "@/lib/utils";
import { Button } from "@wealthfolio/ui/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@wealthfolio/ui/components/ui/tooltip";
import { useTranslation } from "react-i18next";

interface LivePricesToggleProps {
  enabled: boolean;
  onToggle: () => void;
  updatedAt: Date | null;
  isFetching: boolean;
  hasError: boolean;
  className?: string;
}

export function LivePricesToggle({
  enabled,
  onToggle,
  updatedAt,
  isFetching,
  hasError,
  className,
}: LivePricesToggleProps) {
  const { t, i18n } = useTranslation();
  const seconds = LIVE_REFRESH_MS / 1000;
  const hint = !enabled
    ? t("holdings:live_hint", { seconds })
    : hasError
      ? t("holdings:live_error")
      : updatedAt
        ? t("holdings:live_updated", {
            time: updatedAt.toLocaleTimeString(i18n.resolvedLanguage, {
              hour: "2-digit",
              minute: "2-digit",
              second: "2-digit",
            }),
            seconds,
          })
        : t("holdings:live_loading");

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant={enabled ? "secondary" : "outline"}
          size="sm"
          aria-pressed={enabled}
          onClick={onToggle}
          className={cn("h-8 gap-2 rounded-lg px-3", className)}
        >
          <span className="relative flex size-2">
            {enabled && !hasError && (
              <span className="bg-success absolute inline-flex size-full animate-ping rounded-full opacity-60 motion-reduce:hidden" />
            )}
            <span
              className={cn(
                "relative inline-flex size-2 rounded-full",
                !enabled ? "bg-muted-foreground/50" : hasError ? "bg-warning" : "bg-success",
                enabled && isFetching && "opacity-60",
              )}
            />
          </span>
          {t("holdings:live")}
        </Button>
      </TooltipTrigger>
      <TooltipContent>
        <p>{hint}</p>
      </TooltipContent>
    </Tooltip>
  );
}
