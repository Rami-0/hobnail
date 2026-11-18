import { ago } from "../format/index.ts";
import { useNow } from "../live/index.ts";

export function RelativeTime({ at }: { at: number }) {
  const now = useNow();
  const date = new Date(at * 1000);
  return <time dateTime={date.toISOString()} title={date.toLocaleString()}>{ago(at, now)}</time>;
}
