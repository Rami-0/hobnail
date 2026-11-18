import { useEffect, useState } from "react";
import { api } from "../api/session.ts";
import { onConfigChange, watchReadTopic, type ReadTopic } from "./runtime.ts";

export function useRead<T>(path: string | null, topic?: ReadTopic, pollMs?: number) {
  const [result, setResult] = useState<{ data: T | null; error: string }>({ data: null, error: "" });
  useEffect(() => {
    if (!path) return;
    const lifetime = new AbortController();
    let revision = 0;
    const load = () => {
      const current = ++revision;
      void api.get<T>(path, { signal: lifetime.signal }).then((res) => {
        if (lifetime.signal.aborted || revision !== current) return;
        if (res.kind === "ok") setResult({ data: res.data, error: "" });
        else setResult((previous) => ({ ...previous, error: res.message }));
      });
    };
    load();
    const timer = pollMs ? window.setInterval(() => { if (!document.hidden) load(); }, pollMs) : undefined;
    const config = onConfigChange(load);
    const live = topic && path ? watchReadTopic(topic, load) : null;
    return () => { lifetime.abort(); config(); live?.(); window.clearInterval(timer); };
  }, [path, topic, pollMs]);
  return result;
}
