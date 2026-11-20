import { useEffect, useState } from "react";
import { useBootstrap, watchReadTopic } from "../live/index.ts";

export function useAlertCount() {
  const boot = useBootstrap();
  const scoped = boot?.scoped;
  const [live, setLive] = useState<number>();
  useEffect(() => {
    if (scoped !== false) return;
    return watchReadTopic("alerts", (data) => setLive((data as { firing: number }).firing));
  }, [scoped]);
  return scoped ? 0 : live ?? boot?.alerts_firing ?? 0;
}
