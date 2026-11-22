import { createElement as h, Fragment, useEffect, useMemo, useState } from "react";
import type { StructuredBody } from "../api/gen/StructuredBody.ts";

function month(now: number, timeZone: string) {
  const date = new Intl.DateTimeFormat("en-US", { timeZone, year: "numeric", month: "numeric", day: "numeric" });
  const parts = date.formatToParts(now);
  const get = (type: string) => Number(parts.find((part) => part.type === type)?.value);
  const year = get("year"), index = get("month") - 1, today = get("day");
  const first = new Date(Date.UTC(year, index, 1)).getUTCDay();
  const days = new Date(Date.UTC(year, index + 1, 0)).getUTCDate();
  const caption = new Intl.DateTimeFormat(undefined, { timeZone, calendar: "gregory", month: "long", year: "numeric" }).format(now);
  return h("table", { className: "widget-month" },
    h("caption", null, caption),
    h("thead", null, h("tr", null,
      "Sun Mon Tue Wed Thu Fri Sat".split(" ").map((day) => h("th", { key: day, scope: "col", "aria-label": day }, day.slice(0, 2))))),
    h("tbody", null, Array.from({ length: Math.ceil((first + days) / 7) }, (_week, week) =>
      h("tr", { key: week }, Array.from({ length: 7 }, (_, column) => {
        const day = week * 7 + column - first + 1;
        return h("td", { key: column, "aria-current": day === today ? "date" : undefined }, day > 0 && day <= days ? day : "");
      })))));
}

export function Clock({ zones, hour12, calendar }: Extract<StructuredBody, { kind: "clock" }>) {
  const [now, setNow] = useState(Date.now);
  const [reduced, setReduced] = useState(() => globalThis.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false);
  useEffect(() => {
    const media = matchMedia("(prefers-reduced-motion: reduce)");
    const change = () => setReduced(media.matches);
    media.addEventListener("change", change);
    return () => media.removeEventListener("change", change);
  }, []);
  useEffect(() => {
    const period = reduced ? 60000 : 1000;
    let timer: number;
    const tick = () => {
      clearTimeout(timer);
      if (document.hidden) return;
      const current = Date.now();
      setNow(current);
      timer = window.setTimeout(tick, period - current % period);
    };
    tick();
    document.addEventListener("visibilitychange", tick);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", tick);
    };
  }, [reduced]);
  const clocks = useMemo(() => zones.map(([timeZone]) => {
    try {
      return new Intl.DateTimeFormat(undefined, {
        timeZone, hourCycle: hour12 ? "h12" : "h23", hour: "2-digit", minute: "2-digit", second: reduced ? undefined : "2-digit",
      });
    } catch {
      return null;
    }
  }), [zones, hour12, reduced]);
  const firstZone = zones[clocks.findIndex(Boolean)]?.[0];
  return h(Fragment, null, h("dl", null, zones.map(([zone, label], key) =>
    h("div", { key }, h("dt", { title: zone }, label), h("dd", null, clocks[key]?.format(now) ?? "Unknown time zone")))),
    calendar && firstZone && month(now, firstZone));
}
