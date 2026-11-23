import type { Bootstrap } from "../api/gen/Bootstrap.ts";

export function tourBoard(boot: Bootstrap | null | undefined) {
  return boot?.default_board ?? boot?.boards[0]?.id;
}

export function tourHref(boot: Bootstrap | null | undefined) {
  const id = tourBoard(boot);
  return id ? `/app/boards/${id}?tour=1` : undefined;
}
