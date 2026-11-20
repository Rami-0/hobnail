import { Collection, type Entry } from "./Collection.tsx";
import { FailureNote, useSettings, type Control } from "./shared.tsx";

const fields: Control[] = [
  { key: "name", label: "Name", required: true, max: 80 },
  { key: "url", label: "URL", required: true, format: "link" },
  { key: "group", label: "Group" },
  { key: "icon", label: "Icon" },
  { key: "description", label: "Description", max: 200 },
];

function bookmarkRow(entry: Entry) { return <p className="muted">{String(entry.item.group ?? "")}</p>; }

export default function Bookmarks({ editorOnly }: { editorOnly?: boolean }) {
  const { data, failure, load } = useSettings<{ hash: string; bookmarks: Entry[] }>("bookmarks");
  return <>
    <FailureNote failure={failure} retry={() => { void load(); }} />
    {data && <Collection kind="bookmark" title="Bookmarks" path="bookmarks" fields={fields} hash={data.hash}
      entries={data.bookmarks} reorder editorOnly={editorOnly} row={bookmarkRow} body={(patch, input) => ({ name: input.name, url: input.url, ...patch })}
      refresh={async () => {
        const result = await load();
        if (result.kind === "ok") return { hash: result.data.hash, entries: result.data.bookmarks };
      }} />}
  </>;
}
