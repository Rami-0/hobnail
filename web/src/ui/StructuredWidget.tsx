import type { StructuredBody } from "../api/gen/StructuredBody.ts";
import { RelativeTime } from "./RelativeTime.tsx";
import { widgetLink } from "./widgetLink.ts";

export function StructuredWidget({ body }: { body: StructuredBody }) {
  switch (body.kind) {
    case "stat": return <p className="tile-value wrap">{body.value}</p>;
    case "stat_grid": return <dl>{body.rows.map(([label, value], i) => <div key={i}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>;
    case "status_list": return <ul>{body.rows.map(([label, health], i) => <li key={i}>{label} · {health}</li>)}</ul>;
    case "item_list": return <ul>{body.items.map((item, i) => {
      const link = widgetLink(item.link);
      return <li key={i}>{link ? <a href={link}>{item.title}</a> : item.title}{item.secondary && <> · {item.secondary}</>}
        {item.time !== null && <> · <RelativeTime at={item.time} /></>}</li>;
    })}</ul>;
    case "meter": return <div>{body.label} <progress aria-label={body.label} value={body.percent} max={100} /></div>;
    case "table": return <div className="table-scroll"><table><thead><tr>{body.headings.map((h, i) => <th key={i} scope="col">{h}</th>)}</tr></thead>
      <tbody>{body.rows.map((row, i) => <tr key={i}>{row.map((value, j) => <td key={j}>{value}</td>)}</tr>)}</tbody></table></div>;
    case "unavailable": return <p className="muted">{body.message}</p>;
  }
}
