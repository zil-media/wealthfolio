import { HoldingType, QuoteMode } from "@/lib/constants";
import type { Holding, IntradayQuote, MonetaryValue } from "@/lib/types";

/** Assets worth asking a provider for live prices: open, market-priced, not cash. */
export function liveQuoteAssetIds(holdings: readonly Holding[]): string[] {
  const ids = new Set<string>();
  for (const holding of holdings) {
    const instrument = holding.instrument;
    if (!instrument?.id || holding.isClosed || holding.holdingType === HoldingType.CASH) continue;
    if (instrument.quoteMode === QuoteMode.MANUAL) continue;
    ids.add(instrument.id);
  }
  return [...ids].sort();
}

const scale = (value: MonetaryValue, factor: number): MonetaryValue => ({
  local: value.local * factor,
  base: value.base * factor,
});

const shift = (value: MonetaryValue | null | undefined, delta: MonetaryValue) =>
  value ? { local: value.local + delta.local, base: value.base + delta.base } : value;

const ratio = (
  numerator: number | undefined,
  basis: number | undefined,
  fallback?: number | null,
) => (numerator != null && basis ? numerator / basis : fallback);

/**
 * Reprices one holding at the live price. Every amount that depends on price moves by the
 * same factor (FX stays as last calculated); cost bases don't move, so return percentages are
 * recomputed against them. Returns the holding unchanged when the quote can't apply.
 */
export function applyIntradayQuote(holding: Holding, quote: IntradayQuote): Holding {
  const oldPrice = holding.price;
  if (!oldPrice || oldPrice <= 0 || !(quote.lastPrice > 0) || holding.isClosed) return holding;
  // A quote in another unit (GBp vs GBP) would misprice by 100x; leave those alone.
  if (holding.instrument?.currency && holding.instrument.currency !== quote.currency) {
    return holding;
  }

  const factor = quote.lastPrice / oldPrice;
  const marketValue = scale(holding.marketValue, factor);
  const delta = {
    local: marketValue.local - holding.marketValue.local,
    base: marketValue.base - holding.marketValue.base,
  };
  const unrealizedGain = shift(holding.unrealizedGain, delta);
  const totalGain = shift(holding.totalGain, delta);
  const totalReturn = shift(holding.totalReturn, delta);

  let prevCloseValue = holding.prevCloseValue;
  let dayChange = shift(holding.dayChange, delta);
  let dayChangePct = ratio(dayChange?.local, prevCloseValue?.local, holding.dayChangePct);
  if (quote.previousClose && quote.previousClose > 0) {
    prevCloseValue = scale(marketValue, quote.previousClose / quote.lastPrice);
    dayChange = {
      local: marketValue.local - prevCloseValue.local,
      base: marketValue.base - prevCloseValue.base,
    };
    dayChangePct = quote.lastPrice / quote.previousClose - 1;
  }

  return {
    ...holding,
    price: quote.lastPrice,
    marketValue,
    unrealizedGain,
    unrealizedGainPct: ratio(
      unrealizedGain?.local,
      holding.costBasis?.local,
      holding.unrealizedGainPct,
    ),
    totalGain,
    totalGainPct: ratio(totalGain?.local, holding.costBasis?.local, holding.totalGainPct),
    totalReturn,
    totalReturnPct: ratio(totalReturn?.base, holding.returnBasis?.base, holding.totalReturnPct),
    dayChange,
    dayChangePct,
    prevCloseValue,
  };
}

/**
 * Applies live quotes to a holdings list. Weights are rescaled among the listed holdings so
 * their combined share of the portfolio stays what the last calculation said.
 */
export function applyIntradayQuotes(
  holdings: readonly Holding[],
  quotes: readonly IntradayQuote[],
): Holding[] {
  if (quotes.length === 0) return [...holdings];
  const byAsset = new Map(quotes.map((quote) => [quote.assetId, quote]));
  const repriced = holdings.map((holding) => {
    const quote = holding.instrument?.id ? byAsset.get(holding.instrument.id) : undefined;
    return quote ? applyIntradayQuote(holding, quote) : holding;
  });

  const weightBefore = holdings.reduce((sum, holding) => sum + (holding.weight ?? 0), 0);
  const weighted = repriced.map((holding, index) => {
    const before = holdings[index].marketValue.base;
    const factor = before ? holding.marketValue.base / before : 1;
    return (holding.weight ?? 0) * factor;
  });
  const weightAfter = weighted.reduce((sum, weight) => sum + weight, 0);
  if (!weightAfter) return repriced;

  return repriced.map((holding, index) => ({
    ...holding,
    weight: (weighted[index] * weightBefore) / weightAfter,
  }));
}
