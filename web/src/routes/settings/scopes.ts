import type { Scopes } from "../../api/gen/Scopes.ts";

export const emptyScopes = (): Scopes => ({ boards: [], integrations: [] });

export function toggleScope(scopes: Scopes, kind: keyof Scopes, id: string, checked: boolean): Scopes {
  const remaining = scopes[kind].filter((value) => value !== id);
  return { ...scopes, [kind]: checked ? [...remaining, id] : remaining };
}
