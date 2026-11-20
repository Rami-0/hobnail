// Reconnect waits for the live stream: a stream the
// browser gave up on (a proxy's 502 while Hobnail restarts, a refused
// request) is opened again after 5 s, then twice as long each time up to a
// minute; one that opens resets the wait.

export const FIRST_RETRY_MS = 5000;
export const MAX_RETRY_MS = 60000;

/** The wait after `ms` failed to bring the stream back. */
export function nextRetryMs(ms: number): number {
  if (!Number.isFinite(ms) || ms < FIRST_RETRY_MS) return FIRST_RETRY_MS * 2;
  return Math.min(MAX_RETRY_MS, ms * 2);
}

/** Time a tab may stay hidden before its stream is closed (plan 4.2). */
export const HIDDEN_CLOSE_MS = 10000;

/** Hidden longer than this: everything is fetched again on return. */
export const STALE_AFTER_MS = 60000;

/** The "live updates stopped" pill waits this long, so quick reconnects show nothing (ROB-13). */
export const STALE_PILL_MS = 4000;
