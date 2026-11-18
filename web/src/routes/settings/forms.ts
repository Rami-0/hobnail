export type Values = Record<string, unknown>;

export function changedFields(base: Values, input: Values): Values {
  return Object.fromEntries(Object.entries(input).filter(([key, value]) => JSON.stringify(base[key]) !== JSON.stringify(value)));
}

export function rebaseFields(base: Values, input: Values, latest: Values): Values {
  return { ...latest, ...changedFields(base, input) };
}

export function dialogState(search: URLSearchParams, kind: string): { id: string | null } | null {
  if (search.get("add") === kind) return { id: null };
  const edit = search.get("edit");
  const prefix = kind + ":";
  return edit?.startsWith(prefix) && edit.length > prefix.length ? { id: edit.slice(prefix.length) } : null;
}

export function fieldErrors(error: { message: string; field?: string; line?: number; col?: number }): Record<string, string> {
  return { [error.field ?? "form"]: error.line === undefined ? error.message : `${error.message} (line ${error.line}, column ${error.col ?? 1})` };
}
