import { describe, expect, it } from "vitest";
import { HoldingType, QuoteMode } from "@/lib/constants";
import type { Holding, IntradayQuote } from "@/lib/types";
import { applyIntradayQuote, applyIntradayQuotes, liveQuoteAssetIds } from "./live-holdings";

function holding(overrides: Partial<Holding> = {}): Holding {
  return {
    id: "h-aapl",
    holdingType: HoldingType.SECURITY,
    accountId: "acc",
    instrument: {
      id: "AAPL:XNAS",
      symbol: "AAPL",
      currency: "USD",
      quoteMode: QuoteMode.MARKET,
    },
    quantity: 100,
    localCurrency: "USD",
    baseCurrency: "USD",
    price: 300,
    marketValue: { local: 30000, base: 30000 },
    costBasis: { local: 25000, base: 25000 },
    unrealizedGain: { local: 5000, base: 5000 },
    unrealizedGainPct: 0.2,
    totalGain: { local: 5000, base: 5000 },
    totalGainPct: 0.2,
    dayChange: { local: 0, base: 0 },
    dayChangePct: 0,
    prevCloseValue: { local: 30000, base: 30000 },
    weight: 0.5,
    asOfDate: "2026-09-24",
    ...overrides,
  } as Holding;
}

const quote = (overrides: Partial<IntradayQuote> = {}): IntradayQuote => ({
  assetId: "AAPL:XNAS",
  currency: "USD",
  lastPrice: 330,
  previousClose: 320,
  points: [],
  ...overrides,
});

describe("applyIntradayQuote", () => {
  it("reprices value, gains and the day change at the live price", () => {
    const live = applyIntradayQuote(holding(), quote());
    expect(live.price).toBe(330);
    expect(live.marketValue).toEqual({ local: 33000, base: 33000 });
    expect(live.unrealizedGain?.local).toBeCloseTo(8000);
    expect(live.totalGainPct).toBeCloseTo(8000 / 25000);
    expect(live.dayChange?.local).toBeCloseTo(33000 - 32000);
    expect(live.dayChangePct).toBeCloseTo(330 / 320 - 1);
    expect(live.prevCloseValue?.local).toBeCloseTo(32000);
  });

  it("keeps FX from the last calculation for base amounts", () => {
    const live = applyIntradayQuote(
      holding({ marketValue: { local: 30000, base: 27000 }, localCurrency: "USD" }),
      quote(),
    );
    expect(live.marketValue.base).toBeCloseTo(29700);
  });

  it("leaves holdings alone when the quote is in another unit", () => {
    const original = holding();
    expect(applyIntradayQuote(original, quote({ currency: "GBp" }))).toBe(original);
  });

  it("leaves closed or unpriced holdings alone", () => {
    const closed = holding({ isClosed: true });
    const unpriced = holding({ price: null });
    expect(applyIntradayQuote(closed, quote())).toBe(closed);
    expect(applyIntradayQuote(unpriced, quote())).toBe(unpriced);
  });
});

describe("applyIntradayQuotes", () => {
  it("rescales weights among listed holdings without changing their combined share", () => {
    const other = holding({
      id: "h-cash",
      holdingType: HoldingType.CASH,
      instrument: null,
      price: null,
      marketValue: { local: 30000, base: 30000 },
      weight: 0.5,
    });
    const [aapl, cash] = applyIntradayQuotes([holding(), other], [quote()]);
    expect(aapl.weight + cash.weight).toBeCloseTo(1);
    expect(aapl.weight).toBeCloseTo(33000 / 63000);
  });
});

describe("liveQuoteAssetIds", () => {
  it("skips cash, closed and manually priced holdings", () => {
    const ids = liveQuoteAssetIds([
      holding(),
      holding({ id: "dup" }),
      holding({ id: "closed", isClosed: true, instrument: { ...holding().instrument!, id: "X" } }),
      holding({
        id: "manual",
        instrument: { ...holding().instrument!, id: "M", quoteMode: QuoteMode.MANUAL },
      }),
      holding({ id: "cash", holdingType: HoldingType.CASH, instrument: null }),
    ]);
    expect(ids).toEqual(["AAPL:XNAS"]);
  });
});
