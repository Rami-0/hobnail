import { local } from "../../app/prefs.ts";

export const SEEN = "rh-seen";

export function readSeen(): number {
  try {
    const value = Number(local()?.getItem(SEEN));
    return Number.isSafeInteger(value) && value > 0 ? value : 0;
  } catch {
    return 0;
  }
}

export function overviewPath(seen: number, admin: boolean): string {
  return `/api/v1/overview?since=${seen}${admin ? "&probes=1" : ""}`;
}
