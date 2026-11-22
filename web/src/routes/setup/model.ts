import type { SetupDocument } from "../../api/gen/SetupDocument.ts";
import type { SetupItem } from "../../api/gen/SetupItem.ts";
import type { SetupStep } from "../../api/gen/SetupStep.ts";

export const STEPS = {
  appearance: "Appearance",
  discover: "Discover services",
  select: "Select what to include",
  connections: "Configure connections",
  review: "Review dashboard",
} satisfies Record<SetupStep, string>;
export const stepOrder = Object.keys(STEPS) as SetupStep[];

export function move(document: SetupDocument, action: "back" | "continue" | "skip"): SetupDocument {
  const index = stepOrder.indexOf(document.step);
  const step = stepOrder[index + (action === "back" ? -1 : 1)];
  return step ? { ...document, step } : document;
}

export function selectedGroups(document: SetupDocument) {
  const groups = new Map<string, typeof document.items>();
  for (const item of document.items) {
    if (!item.included) continue;
    const group = item.group.trim() || "Services";
    const items = groups.get(group);
    if (items) items.push(item);
    else groups.set(group, [item]);
  }
  return groups;
}


export function incompleteField({ name, target }: SetupItem, endpoint = true): "name" | "url" | undefined {
  if (!name.trim()) return "name";
  if (target.kind === "provider" && endpoint && !target.base_url.trim()) return "url";
  if (target.kind === "board" && target.payload.kind === "link" && !target.payload.url.trim()) return "url";
}
