import { lazy, Suspense, useState, type ReactNode } from "react";
import { Button } from "./Button.tsx";
const MenuImpl = lazy(() => import("./MenuImpl.tsx"));

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

export function Menu(props: MenuProps) {
  const [loaded, setLoaded] = useState(false);
  const trigger = <Button className={props.triggerClassName ?? "icon-btn"} aria-haspopup="menu" aria-expanded={false}
    aria-label={props.label} title={props.label} data-testid={props.testId}
    onClick={() => setLoaded(true)} onKeyDown={(e) => { if (e.key === "ArrowDown") { e.preventDefault(); setLoaded(true); } }}>
    {props.trigger}
  </Button>;
  return loaded ? <Suspense fallback={trigger}><MenuImpl {...props} /></Suspense> : trigger;
}
