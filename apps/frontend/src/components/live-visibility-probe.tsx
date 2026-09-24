import type { LiveVisibilityRegistry } from "@/lib/live-visibility";
import { cn } from "@/lib/utils";
import { useEffect, useRef, type ReactNode } from "react";

/** Reports whether its row is on screen (clipped by scroll containers, hidden by display:none). */
export function LiveVisibilityProbe({
  assetId,
  registry,
  className,
  children,
}: {
  assetId: string;
  registry: LiveVisibilityRegistry;
  className?: string;
  children?: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    let visible = false;
    const observer = new IntersectionObserver(([entry]) => {
      if (entry.isIntersecting === visible) return;
      visible = entry.isIntersecting;
      registry.setVisible(assetId, visible);
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
      if (visible) registry.setVisible(assetId, false);
    };
  }, [assetId, registry]);

  return (
    <div ref={ref} className={cn("min-h-px min-w-px", className)}>
      {children}
    </div>
  );
}
