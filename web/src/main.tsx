// Entry module. Order matters: styles, then the stored theme and sidebar
// rail on <html> (before the first render, see app/theme.ts for the
// first-paint limit), then the session request, then React. The live
// stream starts with the first component that asks for live data.
//
import { refreshSession } from "./api/session.ts";
import "@ui-sdk/core/web.css";
import "./styles/global.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./app/App.tsx";
import { LoadBoundary, LoadError } from "./ui/LoadBoundary.tsx";
import { interceptLinks } from "./app/router.ts";
import { applyStoredRail } from "./app/Shell.tsx";
import { applyStoredTheme } from "./app/theme.ts";

applyStoredTheme();
applyStoredRail();
interceptLinks();
void refreshSession();

const el = document.getElementById("root");
if (!el) throw new Error("index.html has no #root");
createRoot(el).render(
  <StrictMode>
    <LoadBoundary fallback={null} failed={<main className="page narrow" id="main"><LoadError>Could not load Hobnail.</LoadError></main>}>
      <App />
    </LoadBoundary>
  </StrictMode>,
);
