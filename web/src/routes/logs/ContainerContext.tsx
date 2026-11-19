import { useBootstrap, useContainers } from "../../live/index.ts";
import { rowLink, viewerHost } from "../../format/index.ts";
import { TabNav } from "../../ui/TabNav.tsx";
import { ServiceIcon, StatusDot } from "../containers/parts.tsx";
import { paneHref } from "./model.ts";
import { tabs } from "../container/range.ts";

export function ContainerContext({ name, query }: { name: string; query: string }) {
  const bootstrap = useBootstrap(), live = useContainers();
  const row = live.data?.groups.flatMap((group) => group.containers).find((container) => container.name === name);
  const admin = !!bootstrap && !bootstrap.scoped && (bootstrap.role === "admin" || bootstrap.role === "owner");
  const open = row ? rowLink(row, viewerHost(location.hostname)) : null;
  const root = `/app/containers/${encodeURIComponent(name)}`;
  return <div className="log-container-context">
    <header className="log-container-head" data-testid="log-container-head">
      <ServiceIcon name={row?.icon ?? null} label={row?.title ?? name} />
      <div><h1>{row && <StatusDot dot={row.dot} />}{row?.title ?? name}</h1><p className="mono muted">{row?.title !== name && `${name} · `}{row?.image}{row && ` · ${row.label}`}</p></div>
      {open && <a className="btn" href={open} target="_blank" rel="noopener noreferrer">Open</a>}
      {admin && <a className="btn ghost" href={`${root}?edit=override`}>Edit</a>}
      <a href={paneHref(query, [name], location.hash, true)}>Side by side</a>
    </header>
    {live.data?.docker_error && <p className="banner warn" role="alert">Docker is unreachable. Logs cannot be read right now. {live.data.docker_error}</p>}
    <TabNav layout="container" label="Container sections" items={tabs(name, query).filter((tab) => tab.id !== "processes" || admin)} current="logs" testId="ctr-tabs" />
  </div>;
}
