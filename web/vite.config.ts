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
import type { Plugin } from "postcss";

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

// Keep the pinned SDK stylesheet import and its global tokens. Rules scoped
// exclusively to unused primitives can be omitted from the shipped layer.
const sdkComponents = new Set(["button", "dialog", "field", "input", "label", "menu", "select", "switch", "toast", "listbox"]);

const sdkStyles: Plugin = {
  postcssPlugin: "hobnail-sdk-styles",
  Once(root) {
    const file = root.source?.input.file ?? "";
    if (!file.endsWith("/@ui-sdk/core/dist/web.css")) return;
    root.walkRules((rule) => {
      if (rule.parent?.type === "rule") return;
      const slots = [...rule.selector.matchAll(/\[data-slot="([a-z-]+)"\]|\bui-([a-z-]+)\b/g)];
      if (slots.length && slots.every((slot) => !sdkComponents.has((slot[1] ?? slot[2] ?? "").split("-")[0] ?? ""))) {
        rule.remove();
      }
    });
  },
};

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
    css: { postcss: { plugins: [sdkStyles] } },
    build: {
      outDir: "dist",
      assetsDir: "assets",
      emptyOutDir: true,
      sourcemap: false,
      // Split the chart implementation itself; its former single lazy chunk
      // exceeded M2b's 60 KB gzip cap. React and SDK imports keep their normal
      // interaction boundaries instead of joining a global vendor bundle.
      rolldownOptions: { output: { strictExecutionOrder: true, codeSplitting: { groups: [
        { name: "charts-vendor", test: /node_modules\/recharts\//, maxSize: 200_000 },
      ] } } },
      // Inline nothing as data: URLs; every asset is a hashed file the
      // server caches for a year.
      assetsInlineLimit: 0,
    },
    server: proxy ? { proxy } : {},
  };
});
