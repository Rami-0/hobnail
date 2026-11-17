// Entry module. Order matters: styles, then the stored theme on <html>
// (before the first render, see app/theme.ts for the first-paint limit),
// then the session request, then React.
//
// When UI SDK is installed, its stylesheet is imported here, once:
//   import "@ui-sdk/core/web.css";
import "./styles/global.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { refreshSession } from "./api/session.ts";
import { App } from "./app/App.tsx";
import { interceptLinks } from "./app/router.ts";
import { applyStoredTheme } from "./app/theme.ts";

applyStoredTheme();
void refreshSession();
interceptLinks();

const el = document.getElementById("root");
if (!el) throw new Error("index.html has no #root");
createRoot(el).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
