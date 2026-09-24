import type { IntradayQuote } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useId } from "react";

const HOUR_MS = 60 * 60 * 1000;
/** A regular equity session; shorter paths are drawn against it so the line grows through the day. */
const EQUITY_SESSION_MS = 6.5 * HOUR_MS;

interface IntradaySparklineProps {
  quote: IntradayQuote;
  width?: number;
  height?: number;
  className?: string;
}

/**
 * Today's price path, green above the previous close and red below, with the previous close as
 * a dotted baseline (the Stocks-app convention).
 */
export function IntradaySparkline({
  quote,
  width = 88,
  height = 30,
  className,
}: IntradaySparklineProps) {
  const gradientId = `intraday-${useId()}`;
  const points = quote.points
    .map((point) => ({ t: Date.parse(point.timestamp), price: point.price }))
    .filter((point) => Number.isFinite(point.t) && point.price > 0);

  if (points.length < 2) {
    return <span className={cn("block", className)} style={{ width, height }} aria-hidden />;
  }

  const baseline =
    quote.previousClose && quote.previousClose > 0 ? quote.previousClose : points[0].price;
  const start = points[0].t;
  const last = points[points.length - 1].t;
  // Equity sessions span hours, crypto spans the whole day; only pad the former.
  const end = last - start < 8 * HOUR_MS ? Math.max(last, start + EQUITY_SESSION_MS) : last;
  let lo = baseline;
  let hi = baseline;
  for (const point of points) {
    if (point.price < lo) lo = point.price;
    if (point.price > hi) hi = point.price;
  }

  const pad = 1.5;
  const x = (t: number) => pad + ((t - start) / (end - start || 1)) * (width - 2 * pad);
  const y = (price: number) => pad + ((hi - price) / (hi - lo || 1)) * (height - 2 * pad);
  const line = points
    .map(
      (point, index) => `${index ? "L" : "M"}${x(point.t).toFixed(1)} ${y(point.price).toFixed(1)}`,
    )
    .join("");
  const baseY = y(baseline).toFixed(1);
  const area = `${line}L${x(last).toFixed(1)} ${baseY}L${x(start).toFixed(1)} ${baseY}Z`;
  const color = quote.lastPrice >= baseline ? "var(--success)" : "var(--destructive)";

  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      className={cn("block shrink-0 overflow-visible", className)}
      aria-hidden
    >
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity={0.28} />
          <stop offset="100%" stopColor={color} stopOpacity={0.02} />
        </linearGradient>
      </defs>
      <line
        x1={pad}
        x2={width - pad}
        y1={baseY}
        y2={baseY}
        stroke="var(--muted-foreground)"
        strokeOpacity={0.6}
        strokeWidth={1}
        strokeDasharray="1.5 2.5"
      />
      <path d={area} fill={`url(#${gradientId})`} />
      <path d={line} fill="none" stroke={color} strokeWidth={1.5} strokeLinejoin="round" />
    </svg>
  );
}
