import { test } from "node:test";
import assert from "node:assert/strict";
import { agendaDays, agendaTime } from "./agenda.ts";

const at = (day: number, hour: number) => Math.floor(new Date(2026, 10, day, hour, 30).getTime() / 1000);

test("agenda groups by local day in time order with relative headings", () => {
  const now = new Date(2026, 10, 23, 9, 0).getTime();
  const days = agendaDays([
    { time: at(24, 20), title: "Later tomorrow" },
    { time: at(23, 21), title: "Tonight" },
    { time: at(24, 8), title: "Tomorrow morning", all_day: true },
    { time: at(27, 12), title: "Friday" },
  ], now, "en-GB");
  assert.deepEqual(days.map((d) => d.heading), ["Today", "Tomorrow", "Friday 27 Nov"]);
  const [today, tomorrow] = days;
  assert.deepEqual(tomorrow?.entries.map((e) => e.title), ["Tomorrow morning", "Later tomorrow"]);
  assert.deepEqual(tomorrow?.entries.map((e) => agendaTime(e, "en-GB")), ["All day", "20:30"]);
  assert.deepEqual(today?.entries.map((e) => agendaTime(e, "en-GB")), ["21:30"]);
});
