// The `?` sheet (src/ui/shell.rs keys_dialog): every binding this client
// has, in the old sheet's order. Pages of the current interface say so.
// "/" names this page's filter (NAV-62). ⌘ instead of Ctrl on a Mac.
import type { ReactNode } from "react";

import { Dialog } from "../ui/index.ts";
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

/** The placeholder of this page's filter, if it has one on screen. */
export function pageFilter(): HTMLInputElement | null {
  const f = document.querySelector<HTMLInputElement>('main input[data-testid="filter"]');
  return f && f.offsetParent !== null && !f.closest("[hidden]") ? f : null;
}

export function Shortcuts({ open, onClose }: { open: boolean; onClose: () => void }) {
  const mod = modName(navigator.platform || navigator.userAgent);
  const f = open ? pageFilter() : null;
  return (
    <Dialog open={open} title="Keyboard shortcuts" onClose={onClose} testId="shortcuts" className="keys">
      <dl className="keys-l">
        <Row k={["g", "o"]}>Overview{OLD}</Row>
        <Row k={["g", "c"]}>Containers</Row>
        <Row k={["g", "h"]}>Host{OLD}</Row>
        <Row k={["g", "l"]}>Logs{OLD}</Row>
        <Row k={["g", "y"]}>History{OLD}</Row>
        <Row k={["g", "a"]}>Alerts{OLD}</Row>
        <Row k={["g", "s"]}>Settings{OLD}</Row>
        <Row k={["j"]}>Next row that opens something</Row>
        <Row k={["k"]}>Previous row</Row>
        <Row k={["↵"]}>Open the row</Row>
        <Row k={["l"]}>Logs of the focused or open container{OLD}</Row>
        <Row k={["/"]}>
          {f ? `This page's filter: “${f.placeholder || f.getAttribute("aria-label") || "Filter"}”` : "This page's filter (this page has none)"}
        </Row>
        <Row k={[mod, "↵"]}>Save, in a dialog</Row>
        <Row k={["?"]}>This list</Row>
        <Row k={["Esc"]}>Close</Row>
      </dl>
    </Dialog>
  );
}
