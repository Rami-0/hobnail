// Menu (DESIGN.md §11.9; the old account menu, static/app.js): a button
// that opens a list of actions under it. Opening focuses the first item;
// Up/Down/Home/End move; Esc closes and returns focus to the button; Tab, a
// click outside or choosing an item closes it.
import { useEffect, useId, useRef, useState, type ReactNode } from "react";

export interface MenuItem {
  label: ReactNode;
  onSelect: () => void;
  icon?: ReactNode;
  testId?: string;
  danger?: boolean;
}

export interface MenuProps {
  /** The button's accessible name ("Account: admin"). */
  label: string;
  /** What the button shows. */
  trigger: ReactNode;
  items: MenuItem[];
  /** A line above the items ("Signed in as admin"). */
  header?: ReactNode;
  /** data-testid of the button. */
  testId?: string;
  triggerClassName?: string;
  /** Which edge of the button the list lines up with (default "end"). */
  align?: "start" | "end";
}

export function Menu({ label, trigger, items, header, testId, triggerClassName, align = "end" }: MenuProps) {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const btn = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const id = useId();

  const close = (refocus: boolean) => {
    setOpen(false);
    if (refocus) btn.current?.focus({ preventScroll: true });
  };

  useEffect(() => {
    const wrap = box.current;
    if (!open || !wrap) return;
    const entries = () => Array.from(list.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? []);
    entries()[0]?.focus();
    const down = (e: PointerEvent) => {
      if (e.target instanceof Node && !wrap.contains(e.target)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setOpen(false);
        btn.current?.focus({ preventScroll: true });
        return;
      }
      if (e.key === "Tab") {
        setOpen(false);
        return;
      }
      if (!/^(ArrowDown|ArrowUp|Home|End)$/.test(e.key)) return;
      e.preventDefault();
      const l = entries();
      if (!l.length) return;
      const i = l.indexOf(document.activeElement as HTMLButtonElement);
      const j =
        e.key === "Home" ? 0 : e.key === "End" ? l.length - 1 : (i + (e.key === "ArrowDown" ? 1 : -1) + l.length) % l.length;
      l[j]?.focus();
    };
    document.addEventListener("pointerdown", down);
    wrap.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", down);
      wrap.removeEventListener("keydown", key);
    };
  }, [open, box, list, btn]);

  return (
    <div className="menu-wrap" ref={box}>
      <button
        ref={btn}
        type="button"
        className={triggerClassName ?? "icon-btn"}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={`${id}-m`}
        aria-label={label}
        title={label}
        data-testid={testId}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" && !open) {
            e.preventDefault();
            setOpen(true);
          }
        }}
      >
        {trigger}
      </button>
      <div ref={list} id={`${id}-m`} className={`menu-body ${align}`} role="menu" aria-label={label} hidden={!open}>
        {header && <div className="menu-who">{header}</div>}
        {items.map((it, i) => (
          <button
            // Items are a fixed list per render; the index is their identity.
            // eslint-disable-next-line react/no-array-index-key
            key={i}
            type="button"
            role="menuitem"
            className={it.danger ? "menu-item danger" : "menu-item"}
            data-testid={it.testId}
            onClick={() => {
              close(true);
              it.onSelect();
            }}
          >
            {it.icon}
            <span>{it.label}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
