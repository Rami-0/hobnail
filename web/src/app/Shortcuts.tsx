// The `?` sheet (src/ui/shell.rs keys_dialog): every binding this client
// has, in the old sheet's order. Pages of the current interface say so.
// "/" names this page's filter (NAV-62). ⌘ instead of Ctrl on a Mac.
import type { ReactNode } from "react";

import { useBootstrap } from "../live/index.ts";
import { Dialog } from "../ui/Dialog.tsx";
import { pageFilter } from "./page-filter.ts";
import { modName } from "./keys.ts";

function Row({ k, children }: { k: string[]; children: ReactNode }) {
  return (
    <div className="kr">
      <dt>
        {k.map((x, i) => (
          // A key may repeat in a chord; position is its identity.
          // eslint-disable-next-line react/no-array-index-key
          <kbd key={i}>{x}</kbd>
        ))}
      </dt>
      <dd>{children}</dd>
    </div>
  );
}

const OLD = <span className="muted"> (current interface)</span>;

export function Shortcuts({ open, onClose }: { open: boolean; onClose: () => void }) {
  const boot = useBootstrap();
  const scoped = boot?.scoped;
  const mod = modName(navigator.platform || navigator.userAgent);
  const f = open ? pageFilter() : null;
  return (
    <Dialog open={open} title="Keyboard shortcuts" onClose={onClose} testId="shortcuts" className="keys">
      <dl className="keys-l">
        {!scoped && <Row k={["g", "o"]}>Overview</Row>}
        <Row k={["g", "c"]}>Containers</Row>
        {!scoped && <>
        <Row k={["g", "h"]}>Host</Row>
        <Row k={["g", "l"]}>Logs{OLD}</Row>
        <Row k={["g", "y"]}>History</Row>
        <Row k={["g", "a"]}>Alerts</Row>
        </>}
        {(!scoped || boot.role === "admin") && <Row k={["g", "s"]}>Settings</Row>}
        <Row k={[mod, "K"]}>Search and commands</Row>
        <Row k={["↑ / ↓"]}>Move through palette results</Row>
        <Row k={["Home / End"]}>First or last palette result</Row>
        <Row k={["Page Up / Down"]}>Move eight results in the palette</Row>
        <Row k={["↵"]}>Run the selected palette result</Row>
        <Row k={["j"]}>Next row that opens something</Row>
        <Row k={["k"]}>Previous row</Row>
        <Row k={["↵"]}>Open the row</Row>
        {!scoped && <Row k={["l"]}>Logs of the focused or open container{OLD}</Row>}
        <Row k={["/"]}>
          {f ? `This page's filter: “${f.placeholder || f.getAttribute("aria-label") || "Filter"}”` : "Search and commands"}
        </Row>
        <Row k={["Alt", "↑ / ↓"]}>Board edit: move an item within its section</Row>
        <Row k={["Alt", "← / →"]}>Board edit: move an item between sections</Row>
        <Row k={["Shift", "arrows"]}>Board edit: resize width or rows on a focused Move or Resize control</Row>
        <Row k={[mod, "↵"]}>Save, in a dialog</Row>
        <Row k={["?"]}>This list</Row>
        <Row k={["Esc"]}>Close</Row>
      </dl>
    </Dialog>
  );
}
