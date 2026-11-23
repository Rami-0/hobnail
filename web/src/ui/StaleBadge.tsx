import { useNow } from "../live/index.ts";
import { duration } from "../format/index.ts";
import { Badge } from "./Badge.tsx";

export function StaleBadge({ at, error }: { at: number; error: string }) {
  const age = duration(useNow() - at);
  // Only the error is a live region: the age ticks and must not be
  // announced again every 30 s; it stays readable in place.
  return <p><Badge variant="warning" data-testid="stale-badge" title={error}>stale · {age}</Badge>{" "}<span role="status">{error}</span></p>;
}
