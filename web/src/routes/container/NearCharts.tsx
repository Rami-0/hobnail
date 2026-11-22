import { lazy, memo, startTransition, Suspense, useEffect, useRef, useState } from "react";
import { ChartPlaceholder } from "../../ui/Sparkline.tsx";
import type { UsageChartsProps } from "../../charts/RechartsUsage.tsx";

const Charts = lazy(() => import("../../charts/RechartsUsage.tsx"));

export const NearCharts = memo(function NearCharts(props: UsageChartsProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [near, setNear] = useState(typeof IntersectionObserver === "undefined");
  useEffect(() => {
    const grid = ref.current;
    if (near || !grid) return;
    const observer = new IntersectionObserver(([entry]) => {
      if (entry?.isIntersecting) {
        observer.disconnect();
        startTransition(() => setNear(true));
      }
    }, { rootMargin: "160px 0px" });
    observer.observe(grid);
    return () => observer.disconnect();
  }, [near]);
  const placeholder = <ChartPlaceholder count={props.metrics?.length ?? 2} ref={ref} />;
  return <Suspense fallback={placeholder}>{near ? <Charts {...props} /> : placeholder}</Suspense>;
});
