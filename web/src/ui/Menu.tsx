import { lazy, useState, type ReactNode } from "react";
import { Button } from "./Button.tsx";
import { LoadBoundary } from "./LoadBoundary.tsx";
const MenuImpl = lazy(() => import("./MenuImpl.tsx"));

export interface MenuItem {
  label: ReactNode;
  onSelect: () => void;
  icon?: ReactNode;
  testId?: string;
  danger?: boolean;
  separator?: boolean;
  shortcut?: string;
}

export interface MenuProps {
  /** The button's accessible name ("Account: admin, AD"). */
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

export function Menu(props: MenuProps) {
  const [loaded, setLoaded] = useState(false);
  const trigger = <Button variant="ghost" className={props.triggerClassName ?? "icon-btn"} aria-haspopup="menu" aria-expanded={false}
    aria-label={props.label} title={props.label} data-testid={props.testId}
    onClick={() => setLoaded(true)} onKeyDown={(e) => { if (e.key === "ArrowDown") { e.preventDefault(); setLoaded(true); } }}>
    {props.trigger}
  </Button>;
  // A missing menu chunk leaves the trigger; the page stays.
  return loaded ? <LoadBoundary fallback={trigger} failed={trigger}><MenuImpl {...props} /></LoadBoundary> : trigger;
}
