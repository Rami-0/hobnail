import { Badge } from "../ui/Badge.tsx";
// The live indicator in the sidebar foot (its dot stays in the rail) and the
// "Live updates stopped at HH:MM" pill (ROB-13): shown after the stream has
// been reconnecting for 4 s, so quick reconnects show nothing; it goes when
// the stream is back. A paused hidden tab is not stale.
import { useEffect, useState } from "react";

import { STALE_PILL_MS } from "../live/backoff.ts";
import { useLiveInfo } from "../live/index.ts";
import type { LiveStatus } from "../live/index.ts";
import { clock } from "./fmt.ts";

const WORD: Record<LiveStatus, string> = {
  connecting: "connecting",
  live: "live",
  reconnecting: "reconnecting",
  stopped: "paused",
};

export function LiveIndicator() {
  const { status } = useLiveInfo();
  const word = WORD[status];
  return (
    <Badge className="live" variant={status === "live" ? "success" : status === "reconnecting" ? "warning" : "secondary"} data-testid="live-status" data-state={status} title={`Live updates: ${word}`}>
      <span className={status === "live" ? "dot ok" : status === "reconnecting" ? "dot warn" : "dot off"} aria-hidden="true" />
      <span className="live-t">{word}</span>
      <span className="sr"> (live updates)</span>
    </Badge>
  );
}

export function StalePill() {
  const { status, troubleSince, lastAt } = useLiveInfo();
  // The reconnecting spell (by its start) that has lasted long enough.
  const [lateFor, setLateFor] = useState(0);
  useEffect(() => {
    if (status !== "reconnecting" || !troubleSince) return;
    const wait = Math.max(0, troubleSince + STALE_PILL_MS - Date.now());
    const t = window.setTimeout(() => setLateFor(troubleSince), wait);
    return () => window.clearTimeout(t);
  }, [status, troubleSince]);
  const late = status === "reconnecting" && troubleSince > 0 && lateFor === troubleSince;
  useEffect(() => {
    document.documentElement.classList.toggle("stale", late);
  }, [late]);
  if (!late) return null;
  return (
    <output className="live-off" data-testid="live-stopped">
      <span className="dot" aria-hidden="true" />
      <span>Live updates stopped at {clock(lastAt || troubleSince)}. Trying to reconnect; this page may be out of date.</span>
    </output>
  );
}
