import { createElement as h } from "react";
import type { RichSpan } from "../api/gen/RichSpan.ts";
import type { StructuredBody } from "../api/gen/StructuredBody.ts";
import { widgetLink } from "./widgetLink.ts";

function spans(items: RichSpan[]) {
  return items.map(({ kind, text, link }, key) => {
    switch (kind) {
      case "break": return h("br", { key });
      case "bold": return h("strong", { key }, text);
      case "italic": return h("em", { key }, text);
      case "code": return h("code", { key }, text);
      case "link": {
        const href = widgetLink(link ?? null, true);
        return href ? h("a", { key, href }, text) : text;
      }
      default: return text;
    }
  });
}

export function RichText({ blocks }: Extract<StructuredBody, { kind: "rich_text" }>) {
  return h("div", { className: "widget-rich" }, blocks.map((block, key) => {
    switch (block.kind) {
      case "heading": return h(block.level === 1 ? "h1" : block.level === 2 ? "h2" : "h3", { key }, spans(block.spans));
      case "paragraph": return h("p", { key }, spans(block.spans));
      case "list": return h(block.start === null ? "ul" : "ol", { key, start: block.start ?? undefined },
        block.items.map((item, index) => h("li", { key: index }, spans(item))));
      case "code": return h("pre", { key }, h("code", null, block.text));
    }
  }));
}
