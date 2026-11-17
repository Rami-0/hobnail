// Route -> page, inside the shell. Marks #root[data-ready="1"] once the first
// render with a known session is committed (the browser checks wait for it).
import { useEffect } from "react";

import { useSession } from "../api/session.ts";
import { Button } from "../ui/index.ts";
import { Home } from "../routes/Home.tsx";
import { Login } from "../routes/Login.tsx";
import { NotFound } from "../routes/NotFound.tsx";
import { useRoute } from "./router.ts";
import { Shell } from "./Shell.tsx";

const TITLES = { home: "Home", login: "Sign in", not_found: "Not found" } as const;

export function App() {
  const { route } = useRoute();
  const s = useSession();
  const settled = s.status !== "loading";

  useEffect(() => {
    document.title = `${TITLES[route.name]} · Hobnail`;
  }, [route.name]);

  useEffect(() => {
    if (settled) document.getElementById("root")?.setAttribute("data-ready", "1");
  }, [settled]);

  let page;
  switch (route.name) {
    case "home":
      page = <Home />;
      break;
    case "login":
      page = <Login />;
      break;
    default:
      page = <NotFound path={route.path} />;
  }
  return (
    <Shell title={TITLES[route.name]}>
      {s.status === "error" && !s.session ? (
        <div className="page narrow">
          <div className="banner bad" role="alert">
            <div>
              Could not read the session: {s.message}.{" "}
              <Button className="sm" onClick={() => window.location.reload()}>
                Reload
              </Button>
            </div>
          </div>
        </div>
      ) : null}
      {page}
    </Shell>
  );
}
