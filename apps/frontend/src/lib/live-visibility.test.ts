import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LiveVisibilityRegistry, parseVisibleAssetIds } from "./live-visibility";

describe("LiveVisibilityRegistry", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("keeps an asset visible while any of its rows is on screen", () => {
    const registry = new LiveVisibilityRegistry(1000);
    registry.setVisible("AAPL", true);
    registry.setVisible("AAPL", true);
    registry.setVisible("AAPL", false);
    vi.advanceTimersByTime(1000);
    expect(parseVisibleAssetIds(registry.getSnapshot())).toEqual(new Set(["AAPL"]));
    registry.setVisible("AAPL", false);
    vi.advanceTimersByTime(1000);
    expect(parseVisibleAssetIds(registry.getSnapshot())).toEqual(new Set());
  });

  it("ignores a row that remounts (hidden then visible again) within the grace period", () => {
    const registry = new LiveVisibilityRegistry(1000);
    const listener = vi.fn();
    registry.setVisible("BTC", true);
    registry.subscribe(listener);
    registry.setVisible("BTC", false);
    registry.setVisible("BTC", true);
    vi.advanceTimersByTime(5000);
    expect(listener).not.toHaveBeenCalled();
    expect(registry.getSnapshot()).toBe("BTC");
  });

  it("notifies only when the visible set changes", () => {
    const registry = new LiveVisibilityRegistry(1000);
    const listener = vi.fn();
    registry.subscribe(listener);
    registry.setVisible("BTC", true);
    registry.setVisible("BTC", true);
    registry.setVisible("AAPL", true);
    expect(listener).toHaveBeenCalledTimes(2);
    expect(registry.getSnapshot()).toBe("AAPL\nBTC");
  });
});
