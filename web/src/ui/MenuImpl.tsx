// Menu (DESIGN.md §11.9; the old account menu, static/app.js): a button
// that opens a list of actions under it. Opening focuses the first item;
// Up/Down/Home/End move; Esc closes and returns focus to the button; Tab, a
// click outside or choosing an item closes it.
import { Menu as SdkMenu, MenuContent, MenuItem as SdkMenuItem, MenuTrigger } from "@ui-sdk/react/menu";

import type { MenuProps } from "./Menu.tsx";

export default function MenuImpl({ label, trigger, items, header, testId, triggerClassName, align = "end" }: MenuProps) {
  return (
    <div className="menu-wrap">
      <SdkMenu defaultOpen>
        <MenuTrigger type="button" className={triggerClassName ?? "icon-btn"}
          aria-label={label} title={label} data-testid={testId}>
          {trigger}
        </MenuTrigger>
        <MenuContent className={`menu-body ${align}`} aria-label={label} align={align} sideOffset={6}>
          {header && <div className="menu-who">{header}</div>}
          {items.map((it, i) => (
            // Items are a fixed list for each menu.
            // eslint-disable-next-line react/no-array-index-key
            <SdkMenuItem key={i} value={String(i)} className={it.danger ? "menu-item danger" : "menu-item"}
              render={<button type="button" aria-label={typeof it.label === "string" ? it.label : undefined} />} data-testid={it.testId} onSelect={() => { it.onSelect(); }}>
              {it.icon}<span>{it.label}</span>
            </SdkMenuItem>
          ))}
        </MenuContent>
      </SdkMenu>
    </div>
  );
}
