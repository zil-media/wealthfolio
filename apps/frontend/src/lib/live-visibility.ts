/**
 * Tracks which assets currently have a row on screen, so live mode only asks for those.
 * Rows report through `LiveVisibilityProbe`; readers subscribe via `useSyncExternalStore`.
 *
 * Table cells remount on every render (their column definitions are rebuilt), so a probe
 * reports "hidden" and then "visible" again within a frame. Hides only count after a short
 * grace period, which keeps that churn from ever reaching subscribers.
 */
export class LiveVisibilityRegistry {
  private counts = new Map<string, number>();
  private pendingHides = new Map<string, number>();
  private listeners = new Set<() => void>();
  private snapshot = "";

  constructor(private readonly hideGraceMs = 1000) {}

  /** Several rows (desktop and mobile, or one asset in two accounts) can show the same asset. */
  setVisible(assetId: string, visible: boolean) {
    if (visible) {
      const pending = this.pendingHides.get(assetId) ?? 0;
      if (pending > 0) {
        this.pendingHides.set(assetId, pending - 1);
        return;
      }
      this.counts.set(assetId, (this.counts.get(assetId) ?? 0) + 1);
      this.publish();
      return;
    }

    this.pendingHides.set(assetId, (this.pendingHides.get(assetId) ?? 0) + 1);
    setTimeout(() => {
      const pending = this.pendingHides.get(assetId) ?? 0;
      if (pending === 0) return;
      this.pendingHides.set(assetId, pending - 1);
      const count = (this.counts.get(assetId) ?? 0) - 1;
      if (count > 0) this.counts.set(assetId, count);
      else this.counts.delete(assetId);
      this.publish();
    }, this.hideGraceMs);
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  /** Sorted visible asset ids joined by newlines (a stable string for React to compare). */
  getSnapshot = () => this.snapshot;

  private publish() {
    const next = [...this.counts.keys()].sort().join("\n");
    if (next === this.snapshot) return;
    this.snapshot = next;
    this.listeners.forEach((listener) => listener());
  }
}

export function parseVisibleAssetIds(snapshot: string): Set<string> {
  return new Set(snapshot ? snapshot.split("\n") : []);
}
