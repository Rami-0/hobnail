import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { StructuredBody } from "../api/gen/StructuredBody.ts";
import { StructuredWidget } from "./StructuredWidget.ts";

const render = (body: StructuredBody) => renderToStaticMarkup(createElement(StructuredWidget, { body }));

test("renders the shared provider body vocabulary as text and semantic elements", () => {
  assert.match(render({ kind: "stat", value: "<42>" }), /&lt;42&gt;/);
  assert.match(render({ kind: "stat_grid", rows: [["Reading", "42"]] }), /<dt>Reading<\/dt><dd>42<\/dd>/);
  assert.match(render({ kind: "status_list", rows: [["Router", "up"], ["Store", "down"], ["Disk", "warn"]] }), /Router<\/span><span class="sw-health sw-up">up<\/span>.*Store.*sw-down.*Disk.*sw-warn/);
  assert.match(render({ kind: "meter", label: "Used", percent: 42 }), /<progress aria-label="Used" value="42" max="100">/);
  assert.match(render({ kind: "table", headings: ["Disk", "Used"], rows: [["Data", "42%"]] }), /<th scope="col">Disk<\/th>.*<td>Data<\/td><td>42%<\/td>/);
  assert.match(render({ kind: "unavailable", message: "No reading" }), /No reading/);
});

test("item list preserves safe navigation and text while refusing unsafe links", () => {
  const markup = render({ kind: "item_list", items: [
    { title: "Reading", secondary: "42 units", link: "https://fixture.test/updates", time: 1 },
    { title: "<unsafe>", secondary: null, link: "javascript:void(0)", time: null },
  ] });
  assert.match(markup, /<a href="https:\/\/fixture.test\/updates">Reading<\/a><\/span><span class="sw-secondary">42 units<\/span><time dateTime="1970-01-01T00:00:01.000Z"/);
  assert.match(markup, /<span class="sw-label">&lt;unsafe&gt;<\/span><\/li>/);
  assert.equal(markup.includes("javascript:"), false);
});

test("clock handles unknown zones and renders a calendar for the first valid zone", () => {
  const markup = render({ kind: "clock", zones: [["Imaginary/Zone", "Unknown"], ["UTC", "Universal"]], hour12: false, calendar: true });
  assert.match(markup, /Unknown time zone/);
  assert.match(markup, /<dt title="UTC">Universal<\/dt><dd>\d{2}:\d{2}:\d{2}<\/dd>/);
  assert.match(markup, /<caption>.+<\/caption>.*aria-current="date"/);
});

test("rich text escapes HTML and permits only HTTP(S) and mailto navigation", () => {
  const markup = render({ kind: "rich_text", blocks: [
    { kind: "heading", level: 2, spans: [{ kind: "text", text: "Title" }] },
    { kind: "paragraph", spans: [
      { kind: "text", text: '<img src="bad" onerror="evil()">' },
      { kind: "link", text: "Unsafe", link: "javascript:alert(1)" },
      { kind: "link", text: "Email", link: "mailto:help@example.test" },
      { kind: "link", text: 'Unsafe attribute "onclick"', link: "https://example.test/" },
    ] },
    { kind: "list", start: 4, items: [[{ kind: "italic", text: "Item" }]] },
  ] });
  assert.match(markup, /<h2>Title<\/h2>/);
  assert.ok(markup.includes("&lt;img src=&quot;bad&quot; onerror=&quot;evil()&quot;&gt;Unsafe"));
  assert.match(markup, /<a href="mailto:help@example.test">Email<\/a>/);
  assert.match(markup, /<ol start="4"><li><em>Item<\/em><\/li><\/ol>/);
  assert.ok(!/<img|javascript:| onclick=/.test(markup));
});
