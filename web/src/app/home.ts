import type { Bootstrap } from "../api/gen/Bootstrap.ts";

// The Home page is the Home board (`home_board`); the default board is what
// `/app` opens. Either may be unset or hidden from a scoped account.
type Boot = Pick<Bootstrap, "home_board" | "default_board" | "scoped" | "boards">;

const board = (id: string) => `/app/boards/${id}`;

/** Where the Home entry (sidebar, bottom bar, `g o`, /app/overview) goes. */
export function homeHref(boot: Boot | null | undefined): string {
  return boot?.home_board ? board(boot.home_board) : "/app";
}

/** Where `/app` lands: the default board, else Home, else (scoped) the first board. */
export function landingHref(boot: Boot): string {
  const id = boot.default_board ?? boot.home_board ?? (boot.scoped ? boot.boards[0]?.id : undefined);
  return id ? board(id) : "/app/containers";
}
