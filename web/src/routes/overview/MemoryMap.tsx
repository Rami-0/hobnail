import { ScrollTextIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { logsHref } from "../../app/paths.ts";
import { containerHref } from "../../app/router.ts";
import { bytes } from "../../format/index.ts";
import { treemap } from "./treemap.ts";

export type MemoryRow = { name: string; title: string; stack: string; mem: number };

/** One of six muted colours; a stack keeps its colour across reloads because the pick is a hash of its name. */
const hue = (stack: string) => {
  let h = 0;
  for (const c of stack) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `oklch(.72 .09 ${[140, 10, 95, 230, 300, 185][h % 6]})`;
};

const at = (n: number, of: number) => n / of * 100 + "%";

/** Tiles below this can't hold a name over a size, so they fold into "others". */
const MIN: [number, number] = [96, 52];

export function MemoryMap({ rows }: { rows: MemoryRow[] }) {
  const ref = useRef<HTMLDivElement>(null);
  const [[W, H], setSize] = useState([0, 0]);
  // ResizeObserver reports once on observe, then on every size change.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const w = Math.round(entry.contentRect.width), h = Math.round(entry.contentRect.height);
      setSize((size) => size[0] === w && size[1] === h ? size : [w, h]);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  const total = rows.reduce((n, r) => n + r.mem, 0);
  const map = W > 0 && H > 0 ? treemap(rows.map((r) => r.mem), W, H, MIN) : null;
  const others = map ? rows.slice(map.shown) : [];
  return <>
    <p className="mm-sum"><b>{bytes(total)}</b> in {rows.length} containers</p>
    <div className="mm" ref={ref}>
      {map?.boxes.map(([x, y, w, h], i) => {
        const row = i < map.shown ? rows[i] : null;
        const mem = row ? row.mem : others.reduce((n, r) => n + r.mem, 0);
        const label = row ? row.title : `+${others.length} others`;
        const share = total ? Math.round(mem / total * 100) : 0;
        const roomy = h >= 84 && w >= 120;
        return <div key={row?.name ?? "others"} className="mm-t" style={{ ...(row && { "--c": hue(row.stack) }), left: at(x, W), top: at(y, H), width: at(w, W), height: at(h, H) }}>
          <a className="mm-a" href={row ? containerHref(row.name) : "/app/containers?sort=mem"} title={label}>
            <span className="mm-n">{label}</span>
            <span className="num">{bytes(mem)}{roomy && ` · ${share}%`}</span>
          </a>
          {row && roomy && <a className="mm-l" href={logsHref(row.name, null)} aria-label={"Logs of " + label}><ScrollTextIcon size={15} aria-hidden="true" /></a>}
        </div>;
      })}
    </div>
  </>;
}
