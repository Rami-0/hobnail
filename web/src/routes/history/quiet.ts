export const MAX_DEFER_MS = 500;
export const QUIET_MS = 100;

export function quietDelay(pressed: boolean, until: number, deadline: number, now: number): number {
  return Math.min(pressed ? QUIET_MS : until - now, deadline - now);
}
