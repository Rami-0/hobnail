import { createElement as h, type ReactNode } from "react";
import type { BodyRow } from "../api/gen/BodyRow.ts";
import type { Health } from "../api/gen/Health.ts";
import type { MeterRow } from "../api/gen/MeterRow.ts";
import type { StatTile } from "../api/gen/StatTile.ts";
import type { StructuredBody } from "../api/gen/StructuredBody.ts";
import { agendaDays, agendaTime } from "./agenda.ts";
import { RichText } from "./RichText.ts";
import { Clock } from "./Clock.ts";
import { level, sparkPaths } from "./svg.ts";
import { GLYPHS } from "./widgetGlyphs.ts";
import { widgetLink } from "./widgetLink.ts";

const TONE: Record<Health, string> = { up: "ok", warn: "warn", down: "bad" };
const WORD: Record<Health, string> = { up: "up", warn: "warning", down: "down" };

function dot(health: Health) {
  return h("span", { className: `dot ${TONE[health]}`, role: "img", "aria-label": WORD[health] });
}

/** A labelled bar in the brand colour. A usage bar turns amber from 80 % and red from 90 %. */
function bar(percent: number, label: string, usage: boolean, className = "sw-bar") {
  const pct = Math.max(0, Math.min(100, Math.round(percent)));
  return h("span", { className: `${className} ${usage ? level(pct) : ""}`.trim(), role: "meter", "aria-label": label,
    "aria-valuemin": 0, "aria-valuemax": 100, "aria-valuenow": pct, "aria-valuetext": `${pct}%` },
  h("span", { style: { inlineSize: `${pct}%` } }));
}

function meterRow(meter: MeterRow, key: number) {
  return h("li", { key, className: "sw-meter-row" },
    h("div", { className: "sw-meter-head" }, h("span", { className: "sw-label" }, meter.label),
      h("span", { className: "sw-meter-value" }, meter.detail && h("span", { className: "sw-secondary" }, meter.detail), h("strong", null, `${meter.percent}%`))),
    bar(meter.percent, meter.label, true));
}

function spark(values: readonly number[], label: string) {
  const p = sparkPaths(values);
  if (!p) return null;
  return h("svg", { className: "spark sw-spark", viewBox: "0 0 100 24", preserveAspectRatio: "none", role: "img", "aria-label": label, focusable: "false" },
    h("path", { className: "area", d: p.area }), h("path", { className: "line", d: p.line }));
}

function tile(t: StatTile, key: number) {
  const Glyph = t.glyph ? GLYPHS[t.glyph] : null;
  return h("div", { key, className: "sw-tile", "data-tone": t.tone ? TONE[t.tone] : undefined },
    h("dt", null, Glyph && h("span", { className: "sw-glyph", "aria-hidden": true }, h(Glyph)), h("span", { className: "sw-tile-label" }, t.label)),
    h("dd", null, h("span", { className: "sw-tile-value" }, t.value),
      t.delta && h("span", { className: "sw-delta" }, t.delta)),
    t.series?.length ? h("dd", { className: "sw-tile-trend" }, spark(t.series, `${t.label} trend`)) : null);
}

function bars(states: readonly Health[], label: string) {
  const up = states.filter((s) => s === "up").length;
  return h("span", { className: "sw-bars", role: "img", "aria-label": `${label}: ${up} of ${states.length} recent checks up` },
    states.map((state, key) => h("span", { key, className: TONE[state] })));
}

function richRow(r: BodyRow, key: number) {
  return h("li", { key, className: "sw-rich-row" },
    h("div", { className: "sw-rich-main" },
      r.status && dot(r.status),
      h("span", { className: "sw-rich-text" },
        h("span", { className: "sw-label" }, r.title),
        r.secondary && h("span", { className: "sw-sub" }, r.secondary)),
      r.value && h("span", { className: "sw-value" }, r.value)),
    r.progress !== undefined && r.progress !== null && bar(r.progress, `${r.title} progress`, false, "sw-bar sw-bar-thin"),
    r.bars?.length ? bars(r.bars, r.title) : null);
}

export function StructuredWidget({ body, renderIcon }: {
  body: StructuredBody; renderIcon?: (icon: string, title: string) => ReactNode;
}) {
  // The rows are read-only snapshots. Position keeps keys distinct when
  // display labels or values repeat.
  switch (body.kind) {
    case "stat":
      return h("p", { className: "sw-stat wrap" }, body.value);
    case "stat_grid":
      return h("dl", { className: "sw-grid" }, body.rows.map(([label, value], key) =>
        h("div", { key, className: "sw-cell" }, h("dt", null, label), h("dd", null, value))));
    case "status_list":
      return h("ul", { className: "sw-list" }, body.rows.map(([label, health], key) =>
        h("li", { key, className: "sw-row" },
          h("span", { className: `dot ${health === "up" ? "ok" : health === "warn" ? "warn" : "bad"}`, "aria-hidden": true }),
          h("span", { className: "sw-label" }, label),
          h("span", { className: `sw-health sw-${health}` }, health))));
    case "item_list":
      return h("ul", { className: "sw-list" }, body.items.map((item, key) => {
        const link = widgetLink(item.link);
        const date = item.time === null ? null : new Date(item.time * 1000);
        return h("li", { key, className: "sw-row" },
          item.icon && renderIcon?.(item.icon, item.title),
          h("span", { className: "sw-label" }, link ? h("a", { href: link }, item.title) : item.title),
          item.secondary && h("span", { className: "sw-secondary" }, item.secondary),
          date && Number.isFinite(date.getTime()) && h("time", { dateTime: date.toISOString(), className: "sw-secondary" }, date.toLocaleString()));
      }));
    case "rich_text":
      return h(RichText, body);
    case "clock":
      return h(Clock, body);
    case "meter":
      return h("div", { className: "sw-meter" },
        h("div", { className: "sw-meter-head" }, h("span", null, body.label), h("strong", null, `${Math.round(body.percent)}%`)),
        h("progress", { "aria-label": body.label, value: body.percent, max: 100 }));
    case "table":
      return h("div", { className: "provider-table" },
        h("table", { className: "tbl" },
          h("thead", null, h("tr", null, body.headings.map((label, key) => h("th", { key, scope: "col" }, label)))),
          h("tbody", null, body.rows.map((row, key) => h("tr", { key },
            row.map((value, cell) => h("td", { key: cell }, value)))))));
    case "unavailable":
      return h("p", { className: "muted" }, body.message);
    case "meters":
      return h("ul", { className: "sw-meters" }, body.rows.map(meterRow));
    case "tiles":
      return h("dl", { className: "sw-tiles" }, body.tiles.map(tile));
    case "rows":
      return h("ul", { className: "sw-rich" }, body.rows.map(richRow));
    case "hobnail":
      // Drawn live on boards (boards/HobnailWidget.tsx); elsewhere a note.
      return h("p", { className: "muted" }, "Live Hobnail widget: open it on its board.");
    case "agenda":
      return h("div", { className: "sw-agenda" }, agendaDays(body.entries, Date.now()).map((day) =>
        h("section", { key: day.key }, h("h4", null, day.heading),
          h("ul", null, day.entries.map((entry, key) => h("li", { key },
            h("time", { dateTime: new Date(entry.time * 1000).toISOString() }, agendaTime(entry)),
            h("span", { className: "sw-rich-text" }, h("span", { className: "sw-label" }, entry.status && dot(entry.status), entry.title),
              entry.secondary && h("span", { className: "sw-sub" }, entry.secondary))))))));
  }
}
