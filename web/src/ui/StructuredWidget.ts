import { createElement as h } from "react";
import type { StructuredBody } from "../api/gen/StructuredBody.ts";
import { Clock } from "./Clock.ts";
import { widgetLink } from "./widgetLink.ts";

export function StructuredWidget({ body }: { body: StructuredBody }) {
  // The rows are read-only snapshots. Position keeps keys distinct when
  // display labels or values repeat.
  switch (body.kind) {
    case "stat":
      return h("p", { className: "tile-value wrap" }, body.value);
    case "stat_grid":
      return h("dl", null, body.rows.map(([label, value], key) =>
        h("div", { key }, h("dt", null, label), h("dd", null, value))));
    case "status_list":
      return h("ul", null, body.rows.map(([label, health], key) =>
        h("li", { key }, label, " · ", health)));
    case "item_list":
      return h("ul", null, body.items.map((item, key) => {
        const link = widgetLink(item.link);
        const date = item.time === null ? null : new Date(item.time * 1000);
        return h("li", { key },
          link ? h("a", { href: link }, item.title) : item.title,
          item.secondary && ` · ${item.secondary}`,
          date && Number.isFinite(date.getTime()) && h("time", { dateTime: date.toISOString() }, ` · ${date.toLocaleString()}`));
      }));
    case "clock":
      return h(Clock, body);
    case "meter":
      return h("div", null, body.label, " ", h("progress", { "aria-label": body.label, value: body.percent, max: 100 }));
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
