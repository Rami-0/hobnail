import { useEffect } from "react";
import { useBootstrapState } from "../live/index.ts";
import { navigate } from "../app/router.ts";
import { homeHref, landingHref } from "../app/home.ts";

/** `/app` (the default board) and `/app/overview` (the Home board) only redirect. */
export function Home({ home = false }: { home?: boolean }) {
  const { data, error } = useBootstrapState();
  useEffect(() => {
    if (!data) return;
    const href = home ? homeHref(data) : "/app";
    navigate(href === "/app" ? landingHref(data) : href, { replace: true });
  }, [data, home]);
  return <p className="page" role="status">{error ?? "Loading home…"}</p>;
}
