import { createElement as h, type ReactNode } from "react";
import type { StructuredBody } from "../api/gen/StructuredBody.ts";
import { RichText } from "./RichText.ts";
import { Clock } from "./Clock.ts";
import { widgetLink } from "./widgetLink.ts";

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
  }
}
