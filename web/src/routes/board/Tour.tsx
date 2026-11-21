import { useLayoutEffect, useRef, useState } from "react";
import { api, refreshSession } from "../../api/session.ts";
import { Button } from "../../ui/Button.tsx";
import { Card } from "../../ui/Card.tsx";

const edit = ['[data-testid="board-edit"]', "Edit board lets you add services, move and resize tiles, and save your layout."];
const add = ['[data-testid="board-empty-add"]', "Add opens the board editor and picker. Use Discover to browse available services."];
const palette = ['[data-testid="palette-open"]', "Search and commands opens the command palette. Press Ctrl+K or Command+K to open it from anywhere."];
const account = ['button[data-testid="user"]', "Open your account menu to manage your password or sign out."];
const settings = ['nav [data-rtip="Settings"]', "Open Settings to manage boards and integrations."];
const shortcuts = ['button[aria-label="Keyboard shortcuts"]', "Open Keyboard shortcuts to see what you can do from the keyboard."];
const more = ['[data-testid="drawer-open"]', "More opens navigation to the rest of Hobnail."];
const mobileSettings = [more[0], "Tap More, then Settings, to manage boards and integrations."];

export default function Tour({ admin, empty, onClose }: { admin: boolean; empty: boolean; onClose: () => void }) {
  const phone = window.matchMedia("(max-width:699px)").matches;
  const steps = admin ? [edit, ...(empty ? [add] : []), palette, phone ? mobileSettings : settings]
    : [palette, account, phone ? more : shortcuts];
  const [index, setIndex] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const panel = useRef<HTMLElement>(null);
  const next = useRef<HTMLButtonElement>(null);
  const anchor = useRef<HTMLElement | null>(null);
  const [selector, description] = steps[index] ?? palette;
  const last = index === steps.length - 1;

  async function finish() {
    if (busy) return;
    setBusy(true);
    setError("");
    const result = await api.post("/api/v1/account/board-tour");
    if (result.kind === "ok") {
      anchor.current?.focus({ preventScroll: true });
      onClose();
      void refreshSession();
    } else {
      setError(result.message);
      setBusy(false);
    }
  }

  useLayoutEffect(() => {
    const card = panel.current;
    const target = selector ? document.querySelector<HTMLElement>(selector) : null;
    if (!card) return;
    anchor.current = target;
    const describedBy = target?.getAttribute("aria-describedby");
    target?.setAttribute("aria-describedby", [describedBy, "board-tour-description"].filter(Boolean).join(" "));
    target?.setAttribute("data-board-tour-anchor", "");
    const position = () => {
      const rect = target?.getBoundingClientRect();
      const gap = 12;
      const below = (rect?.bottom ?? gap) + gap;
      const above = (rect?.top ?? gap) - card.offsetHeight - gap;
      const top = below + card.offsetHeight <= window.innerHeight - gap ? below : above;
      card.style.top = `${Math.max(gap, Math.min(top, window.innerHeight - card.offsetHeight - gap))}px`;
      const left = rect?.left ?? gap;
      card.style.left = `${Math.max(gap, Math.min(left, document.documentElement.clientWidth - card.offsetWidth - gap))}px`;
    };
    target?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" });
    position();
    next.current?.focus({ preventScroll: true });
    const observer = new ResizeObserver(position);
    observer.observe(card);
    if (target) observer.observe(target);
    window.addEventListener("resize", position);
    window.addEventListener("scroll", position, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", position, true);
      target?.removeAttribute("data-board-tour-anchor");
      if (describedBy) target?.setAttribute("aria-describedby", describedBy);
      else target?.removeAttribute("aria-describedby");
    };
  }, [selector]);

  useLayoutEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented ||
          document.querySelector('[role="dialog"][data-open], [role="menu"][data-open]') ||
          document.documentElement.classList.contains("side-open")) return;
      event.preventDefault();
      void finish();
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  });

  return <Card ref={panel} className="board-tour" aria-labelledby="board-tour-title" data-testid="board-tour">
    <h2 id="board-tour-title">Board tour · {index + 1} of {steps.length}</h2>
    <p id="board-tour-description">{description}</p>
    {error && <p role="alert" className="bad-fg">{error}</p>}
    <div className="acts">
      <Button ref={next} variant="default" busy={busy} data-testid="tour-next" onClick={() => last ? void finish() : setIndex(index + 1)}>{last ? "Finish" : "Next"}</Button>
      <Button disabled={busy} data-testid="tour-skip" onClick={() => void finish()}>Skip tour</Button>
    </div>
  </Card>;
}
