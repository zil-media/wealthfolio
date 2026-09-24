import { AnimatedToggleGroup } from "@wealthfolio/ui";
import { useTranslation } from "react-i18next";

export type ValuationChartMode = "value" | "return";

export function ChartModeToggle({
  value,
  onChange,
  className,
}: {
  value: ValuationChartMode;
  onChange: (value: ValuationChartMode) => void;
  className?: string;
}) {
  const { t } = useTranslation();
  return (
    <AnimatedToggleGroup<ValuationChartMode>
      value={value}
      onValueChange={onChange}
      aria-label={t("common:benchmark_compare.mode_label")}
      items={[
        { value: "value", label: t("common:benchmark_compare.mode_value") },
        {
          value: "return",
          label: t("common:benchmark_compare.mode_return"),
          title: t("common:benchmark_compare.mode_return_hint"),
        },
      ]}
      size="xs"
      rounded="full"
      className={className ?? "bg-muted/30 border"}
    />
  );
}
