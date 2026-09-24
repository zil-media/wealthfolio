import { getIntradayQuotes } from "@/adapters";
import { applyIntradayQuotes, liveQuoteAssetIds } from "@/lib/live-holdings";
import { parseVisibleAssetIds, type LiveVisibilityRegistry } from "@/lib/live-visibility";
import { QueryKeys } from "@/lib/query-keys";
import type { Holding, IntradayQuote } from "@/lib/types";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

/** How often live mode asks for new prices while the page is visible. */
export const LIVE_REFRESH_MS = 30_000;
/**
 * Wait for scrolling to settle before asking for the rows that came into view. Longer than the
 * registry's hide grace, so rows that just left are already gone from the request.
 */
const VISIBILITY_SETTLE_MS = 1200;

const NO_QUOTES = new Map<string, IntradayQuote>();

/**
 * While `enabled`, polls intraday prices for the holdings whose rows are on screen and returns
 * the list repriced. Rows that scroll away keep their last live price. Display only: nothing is
 * written back, the next portfolio calculation stays authoritative.
 *
 * Visibility is read outside React state on purpose: re-rendering on every scroll would rebuild
 * the table cells, whose probes would then report visibility again, and so on.
 */
export function useLiveHoldings(
  holdings: Holding[],
  enabled: boolean,
  visibility: LiveVisibilityRegistry,
) {
  const candidates = useMemo(() => liveQuoteAssetIds(holdings), [holdings]);
  const visibleCandidates = useCallback(() => {
    const visible = parseVisibleAssetIds(visibility.getSnapshot());
    return candidates.filter((id) => visible.has(id));
  }, [candidates, visibility]);
  const lastRequested = useRef("");

  // The visible rows are read when the request runs, not keyed: keying on them would start a
  // new query (and a request) for every scroll position.
  const query = useQuery({
    // eslint-disable-next-line @tanstack/query/exhaustive-deps
    queryKey: [QueryKeys.INTRADAY_QUOTES, candidates],
    queryFn: () => {
      const ids = visibleCandidates();
      lastRequested.current = ids.join("\n");
      return ids.length > 0 ? getIntradayQuotes(ids) : Promise.resolve([]);
    },
    enabled: enabled && candidates.length > 0,
    refetchInterval: enabled ? LIVE_REFRESH_MS : false,
    refetchIntervalInBackground: false,
    staleTime: LIVE_REFRESH_MS / 2,
    placeholderData: keepPreviousData,
    retry: false,
  });
  const { refetch } = query;

  // Ask again once scrolling settles on a different set of rows.
  useEffect(() => {
    if (!enabled) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const scheduleCheck = () => {
      clearTimeout(timer);
      timer = setTimeout(() => {
        // Rows that left keep their last price until the next poll; only new rows need a request.
        const requested = new Set(lastRequested.current.split("\n"));
        if (visibleCandidates().some((id) => !requested.has(id))) void refetch();
      }, VISIBILITY_SETTLE_MS);
    };
    // Also check on (re)subscribe: a change seen by a previous subscription may still be pending.
    scheduleCheck();
    const unsubscribe = visibility.subscribe(scheduleCheck);
    return () => {
      clearTimeout(timer);
      unsubscribe();
    };
  }, [enabled, visibility, visibleCandidates, refetch]);

  // Latest live answer per asset, kept for rows that are no longer on screen.
  const [known, setKnown] = useState(NO_QUOTES);
  useEffect(() => {
    if (!enabled) {
      setKnown(NO_QUOTES);
      return;
    }
    const fresh = query.data;
    if (!fresh?.length) return;
    setKnown((previous) => {
      const next = new Map(previous);
      for (const quote of fresh) next.set(quote.assetId, quote);
      return next;
    });
  }, [enabled, query.data]);

  const liveHoldings = useMemo(
    () => (known.size > 0 ? applyIntradayQuotes(holdings, [...known.values()]) : holdings),
    [holdings, known],
  );

  return {
    holdings: liveHoldings,
    quotesByAssetId: enabled ? known : undefined,
    updatedAt: enabled && query.dataUpdatedAt ? new Date(query.dataUpdatedAt) : null,
    isFetching: enabled && query.isFetching,
    error: enabled && query.isError ? query.error : null,
  };
}
