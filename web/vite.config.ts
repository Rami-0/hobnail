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
import { defineConfig, type CSSOptions } from "vite";
import react from "@vitejs/plugin-react";


// No @types/node in this project: declare the one global the config reads.
declare const process: { env: Record<string, string | undefined> };

/** Server routes the client calls or links to; everything else is Vite's. */
const PROXIED = [
  "/api",
  "/icons",
  "/custom.css",
  "/manifest.webmanifest",
  // The PWA icons the document head links to (src/http/pwa.rs), and the
  // health check the noscript text names.
  "/icon.svg",
  "/icon-192.png",
  "/icon-512.png",
  "/apple-touch-icon.png",
  "/healthz",
];

// Keep the pinned SDK stylesheet import and its global tokens. Rules scoped
// exclusively to unused primitives can be omitted from the shipped layer.
// Controls with complete Hobnail styles do not need the optional SDK defaults.
// Keep popup structure and SDK-managed visibility for the parts in use.
const sdkParts = new Set([
  "checkbox-indicator", "dialog-backdrop", "dialog-content", "dialog-description", "dialog-title",
  "field", "field-description", "field-error", "field-label", "label",
  "menu-content", "menu-item", "menu-separator", "select-content", "select-item", "select-item-indicator",
  "toast", "toast-action", "toast-close", "toast-region", "toast-title", "tooltip-content",
]);
const sdkSlots = (selector: string) =>
  [...selector.matchAll(/\[data-slot="([a-z-]+)"\]|\bui-([a-z-]+)\b/g)].map((slot) => slot[1] ?? slot[2] ?? "");
const keepSdkSelector = (selector: string) => {
  const slots = sdkSlots(selector);
  return slots.length === 0 || slots.some((slot) => sdkParts.has(slot));
};

type PostcssPlugin = NonNullable<Exclude<CSSOptions["postcss"], string | undefined>["plugins"]>[number];

const sdkStyles: PostcssPlugin = {
  postcssPlugin: "hobnail-sdk-styles",
  Once(root) {
    const file = root.source?.input.file ?? "";
    if (!file.endsWith("/@ui-sdk/core/dist/web.css")) return;
    root.walkRules((rule) => {
      const selectors = rule.selectors.filter(keepSdkSelector).map((selector) =>
        selector.replace(/:(is|where)\(([^()]*)\)/g, (_, kind: string, choices: string) => {
          const live = choices.split(",").filter(keepSdkSelector);
          return live.length ? ":" + kind + "(" + live.join(",") + ")" : ":not(*)";
        }),
      );
      if (selectors.length === 0) rule.remove();
      else if (selectors.join(",") !== rule.selectors.join(",")) rule.selectors = selectors;
    });
    const usedIcons = new Set<string>();
    root.walkDecls((decl) => {
      for (const match of decl.value.matchAll(/var\((--ui-icon-[a-z-]+)\b/g)) usedIcons.add(match[1] ?? "");
    });
    root.walkDecls(/^--ui-icon-/, (decl) => {
      if (decl.value.startsWith("url(") && !usedIcons.has(decl.prop)) decl.remove();
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
      target: "esnext",
      cssCodeSplit: false,
      modulePreload: { polyfill: false },
      assetsDir: "assets",
      emptyOutDir: true,
      sourcemap: false,
      // Keep Recharts' mutually dependent selectors in one lazy chunk.
      // Keep its dependencies in a separate lazy chunk so a chart vendor
      // never has to import an initializer from the chart route itself.
      rolldownOptions: { output: {
        chunkFileNames: ({ name }) => /^(?:charts-vendor|RechartsUsage)/.test(name) ? "assets/[name]-[hash].js" : "assets/[hash].js",
        codeSplitting: { groups: [
        { name: "app-runtime", test: /\/src\/app\/(?:settings-tabs|router|paths|prefs|keys|glyphs|theme|fmt)\.tsx?$/, includeDependenciesRecursively: false },
        { name: "settings-pages", test: /\/src\/routes\/settings\/[^/]+\.tsx$/, includeDependenciesRecursively: false },
        { name: "icons", test: /node_modules\/lucide-react\/dist\/[^/]+\/icons\//, includeDependenciesRecursively: false },
        { name: "charts-vendor-deps", test: /node_modules\/(?:d3-[^/]+|internmap|clsx|eventemitter3|reselect|redux(?:-thunk)?|react-redux|immer|use-sync-external-store|es-toolkit|decimal\.js-light|victory-vendor|@reduxjs\/toolkit|@standard-schema\/[^/]+)\//, includeDependenciesRecursively: false },
        { name: "charts-vendor", test: /node_modules\/recharts\//, includeDependenciesRecursively: false },
      ] } } },
      // Inline nothing as data: URLs; every asset is a hashed file the
      // server caches for a year.
      assetsInlineLimit: 0,
    },
    server: proxy ? { proxy } : {},
  };
});
