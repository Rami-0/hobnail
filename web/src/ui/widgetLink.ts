// Provider links allow HTTP(S) navigation. Rich text may also link to email.
export function widgetLink(input: string | null, mail = false): string | null {
  if (!input) return null;
  try {
    const url = new URL(input);
    if (mail && url.protocol === "mailto:" && url.pathname) return url.href;
    return ["http:", "https:"].includes(url.protocol) && !url.username && !url.password ? url.href : null;
  } catch {
    return null;
  }
}
