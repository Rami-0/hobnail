import { useNow } from "../live/index.ts";
import { duration } from "../format/index.ts";
import { Badge } from "./Badge.tsx";

export function StaleBadge({ at, error }: { at: number; error: string }) {
  const age = duration(useNow() - at);
  return <p role="status"><Badge variant="warning" data-testid="stale-badge" title={error}>stale · {age}</Badge>{" "}{error}</p>;
}
