// Vite config for the Hobnail React client (docs/plans/react-port.md 4.3, 4.4).
//
// Build: hashed files under dist/assets/ (Vite's default `[name]-[hash]`),
// base "/" because the Rust server serves /assets/* at the root and the
// document under /app. No source maps in the build.
//
// Dev: `vite` serves the client and proxies the server's routes to a running
// Hobnail named by RH_DEV_BACKEND (e.g. http://127.0.0.1:3210). There is no
// default: a dev server pointed at the wrong backend would sign in to it.
// `changeOrigin: false` keeps `Host: localhost:5173` and the browser's
// `Origin`, so the server's Host guard and same-origin write check pass
// without any dev-only allowance (plan 4.4).
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// No @types/node in this project: declare the one global the config reads.
declare const process: { env: Record<string, string | undefined> };

/** Server routes the client calls or links to; everything else is Vite's. */
const PROXIED = [
  "/api",
  "/icons",
  "/custom.css",
  "/manifest.webmanifest",
  // The PWA icons the document head links to (src/ui/pwa.rs), and the
  // health check the noscript text names.
  "/icon.svg",
  "/icon-192.png",
  "/icon-512.png",
  "/apple-touch-icon.png",
  "/healthz",
];

export default defineConfig(({ command }) => {
  let proxy: Record<string, { target: string; changeOrigin: false; ws: boolean }> | undefined;
  if (command === "serve") {
    const target = process.env.RH_DEV_BACKEND;
    if (!target) {
      throw new Error(
        "RH_DEV_BACKEND is not set: point it at a running Hobnail, for example " +
          "RH_DEV_BACKEND=http://127.0.0.1:3210. The dev server proxies /api, /icons, " +
          "/custom.css, the manifest and the icons to it; there is no default.",
      );
    }
    proxy = Object.fromEntries(PROXIED.map((p) => [p, { target, changeOrigin: false, ws: false }]));
  }
  return {
    base: "/",
    plugins: [react()],
    build: {
      outDir: "dist",
      assetsDir: "assets",
      emptyOutDir: true,
      sourcemap: false,
      // Inline nothing as data: URLs; every asset is a hashed file the
      // server caches for a year.
      assetsInlineLimit: 0,
    },
    server: proxy ? { proxy } : {},
  };
});
