import { useEffect, useState } from "react";
import { api } from "../api/session.ts";
import { onConfigChange, watchReadTopic, type ReadTopic } from "./runtime.ts";

export function useRead<T>(path: string | null, topic?: ReadTopic, pollMs?: number) {
  const [result, setResult] = useState<{ path: string | null; data: T | null; error: string }>({ path, data: null, error: "" });
  if (result.path !== path) setResult({ path, data: null, error: "" });
  useEffect(() => {
    if (!path) return;
    const lifetime = new AbortController();
    let revision = 0;
    const load = () => {
      const current = ++revision;
      void api.get<T>(path, { signal: lifetime.signal }).then((res) => {
        if (lifetime.signal.aborted || revision !== current) return;
        if (res.kind === "ok") setResult({ path, data: res.data, error: "" });
        else setResult((previous) => ({
          path, data: previous.path === path ? previous.data : null, error: res.message,
        }));
      });
    };
    load();
    const timer = pollMs ? window.setInterval(() => { if (!document.hidden) load(); }, pollMs) : undefined;
    const config = onConfigChange(load);
    let pending: number | undefined;
    const live = topic ? watchReadTopic(topic, () => {
      if (pending !== undefined) return;
      pending = window.setTimeout(() => { pending = undefined; load(); }, 500);
    }) : null;
    return () => { lifetime.abort(); config(); live?.(); window.clearInterval(timer); window.clearTimeout(pending); };
  }, [path, topic, pollMs]);
  if (result.path !== path) return { data: null, error: "", stale: false };
  const stale = result.data !== null && !!result.error;
  return { data: result.data, error: stale ? `${result.error} Showing the last loaded data.` : result.error, stale };
}
