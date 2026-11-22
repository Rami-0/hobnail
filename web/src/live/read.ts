import { useEffect, useState } from "react";
import { api } from "../api/session.ts";
import { onConfigChange, watchReadTopic, type ReadTopic } from "./runtime.ts";

type ScheduleCommit = (commit: () => void) => () => void;

export function useRead<T>(path: string | null, topic?: ReadTopic, pollMs?: number, schedule?: ScheduleCommit) {
  const [result, setResult] = useState<{ path: string | null; data: T | null; error: string }>({ path, data: null, error: "" });
  if (result.path !== path) setResult({ path, data: null, error: "" });
  useEffect(() => {
    if (!path) return;
    const lifetime = new AbortController();
    let revision = 0;
    let cancelCommit: (() => void) | undefined;
    const load = (refresh: boolean) => {
      const current = ++revision;
      cancelCommit?.();
      void api.get<T>(path, { signal: lifetime.signal }).then((res) => {
        if (lifetime.signal.aborted || revision !== current) return;
        const commit = () => {
          if (res.kind === "ok") setResult({ path, data: res.data, error: "" });
          else setResult((previous) => ({
            path, data: previous.path === path ? previous.data : null, error: res.message,
          }));
        };
        if (refresh && schedule) cancelCommit = schedule(commit);
        else commit();
      });
    };
    const refetch = () => load(true);
    load(false);
    const timer = pollMs ? window.setInterval(() => { if (!document.hidden) refetch(); }, pollMs) : undefined;
    const config = onConfigChange(refetch);
    let pending: number | undefined;
    const live = topic ? watchReadTopic(topic, () => {
      if (pending !== undefined) return;
      pending = window.setTimeout(() => { pending = undefined; refetch(); }, 500);
    }) : null;
    return () => { lifetime.abort(); cancelCommit?.(); config(); live?.(); window.clearInterval(timer); window.clearTimeout(pending); };
  }, [path, topic, pollMs, schedule]);
  if (result.path !== path) return { data: null, error: "", stale: false };
  const stale = result.data !== null && !!result.error;
  return { data: result.data, error: stale ? `${result.error} Showing the last loaded data.` : result.error, stale };
}
