// Provider-controlled links are allowed only as HTTP(S) navigation.
export function widgetLink(input: string | null): string | null {
  if (!input) return null;
  try {
    const url = new URL(input);
    return ["http:", "https:"].includes(url.protocol) && !url.username && !url.password ? url.href : null;
  } catch {
    return null;
  }
}
