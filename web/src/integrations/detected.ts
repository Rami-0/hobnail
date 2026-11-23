// Detected services (GET /api/v1/providers/detected): local containers that
// look like a provider, with the URL the server proposes. Pure helpers; the
// hook and cards live in Detected.tsx.
import type { DetectedService } from "../api/gen/DetectedService.ts";
import type { Provider } from "../api/gen/Provider.ts";

export interface KV {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** Suggestions the user dismissed in this browser (`provider:container`). */
export const DISMISSED_KEY = "rh-detect-dismissed";
const MAX_DISMISSED = 200;

export const detectionKey = (service: Pick<DetectedService, "provider" | "container">): string =>
  `${service.provider}:${service.container}`;

export function readDismissed(store: KV | null): Set<string> {
  try {
    const value: unknown = JSON.parse(store?.getItem(DISMISSED_KEY) ?? "[]");
    return new Set(Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === "string") : []);
  } catch {
    return new Set();
  }
}

export function writeDismissed(store: KV | null, keys: Set<string>): void {
  try {
    store?.setItem(DISMISSED_KEY, JSON.stringify([...keys].slice(-MAX_DISMISSED)));
  } catch {
    // Blocked storage: the dismissal lasts for this page.
  }
}

/** Services still worth showing: not dismissed and with a known provider. */
export function visibleDetected(services: readonly DetectedService[], dismissed: ReadonlySet<string>, providers: readonly Provider[]): DetectedService[] {
  const known = new Set(providers.map((provider) => provider.id));
  return services.filter((service) => known.has(service.provider) && !dismissed.has(detectionKey(service)));
}

/** `lscr.io/linuxserver/sonarr:4` → `linuxserver/sonarr`: the image as people say it. */
export function imageName(image: string): string {
  const [path = ""] = image.split("@");
  const slash = path.lastIndexOf("/");
  const last = path.slice(slash + 1).split(":")[0] ?? "";
  const parts = path.slice(0, Math.max(slash, 0)).split("/").filter(Boolean);
  if (parts[0] && (/[.:]/.test(parts[0]) || parts[0] === "localhost")) parts.shift();
  return [...parts, last].join("/") || image;
}

/** "linuxserver/sonarr, port 8989". */
export function detectedSummary(service: DetectedService): string {
  return `${imageName(service.image)}, port ${service.port}`;
}

/** The URL Connect fills in, or "" when Hobnail found none it can reach. */
export function proposedUrl(service: DetectedService): string {
  return service.candidates[0]?.url ?? "";
}

const VIA: Record<string, string> = {
  published: "published port",
  network: "container IP on a shared network",
  name: "container name on a shared network",
};

/** How Hobnail would reach it, for the card's hint. */
export function viaLabel(service: DetectedService): string {
  const via = service.candidates[0]?.via;
  return via ? `via ${VIA[via] ?? via}` : "No address Hobnail can reach. Enter the URL when you connect.";
}

/** What the connect step starts from. */
export interface Prefill {
  provider: string;
  url: string;
  name: string;
}

export function prefillFor(service: DetectedService, provider: Pick<Provider, "name">): Prefill {
  return { provider: service.provider, url: proposedUrl(service), name: provider.name };
}
