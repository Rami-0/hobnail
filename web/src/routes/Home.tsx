import { useEffect } from "react";
import { useBootstrapState } from "../live/index.ts";
import { navigate } from "../app/router.ts";

export function Home() {
  const { data, error } = useBootstrapState();
  useEffect(() => {
    if (!data) return;
    const board = data.default_board ?? (data.scoped ? data.boards[0]?.id : undefined);
    navigate(board ? `/app/boards/${board}` : data.scoped ? "/app/containers" : "/app/overview", { replace: true });
  }, [data]);
  return <p className="page" role="status">{error ?? "Loading home…"}</p>;
}
