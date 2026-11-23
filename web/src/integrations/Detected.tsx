// "Detected on this host": cards for local containers that look like a
// provider, shown in the board Add picker, the Add integration dialog and
// Settings → Integrations. Connect starts the provider's connect step with
// the proposed URL; nothing is contacted until the user tests it.
import { useEffect, useState, type CSSProperties } from "react";
import { PlugZapIcon, RadarIcon, XIcon } from "lucide-react";
import type { DetectedService } from "../api/gen/DetectedService.ts";
import type { Provider } from "../api/gen/Provider.ts";
import { api } from "../api/session.ts";
import { Button } from "../ui/Button.tsx";
import { ServiceIcon } from "../ui/ServiceIcon.tsx";
import { brandColor } from "../ui/brand.ts";
import { detectedSummary, detectionKey, prefillFor, proposedUrl, readDismissed, viaLabel, visibleDetected, writeDismissed, type KV, type Prefill } from "./detected.ts";

const storage = (): KV | null => { try { return globalThis.localStorage ?? null; } catch { return null; } };

export interface Detection {
  services: DetectedService[] | undefined;
  dismissed: ReadonlySet<string>;
  dismiss: (service: DetectedService) => void;
}

/** Detected services, fetched while `enabled` (unscoped admins only) and
 * again whenever `refresh` changes (after a connection is saved). */
export function useDetected(enabled: boolean, refresh = 0): Detection {
  const [services, setServices] = useState<DetectedService[]>();
  const [dismissed, setDismissed] = useState<ReadonlySet<string>>(() => readDismissed(storage()));
  useEffect(() => {
    // A negative refresh never happens; reading it ties the fetch to it.
    if (!enabled || refresh < 0) return;
    const controller = new AbortController();
    void api.get<DetectedService[]>("/api/v1/providers/detected", { signal: controller.signal }).then((result) => {
      if (!controller.signal.aborted && result.kind === "ok") setServices(result.data);
    });
    return () => controller.abort();
  }, [enabled, refresh]);
  const dismiss = (service: DetectedService) => {
    const next = new Set(dismissed);
    next.add(detectionKey(service));
    writeDismissed(storage(), next);
    setDismissed(next);
  };
  return { services, dismissed, dismiss };
}

export function DetectedCards({ detection, providers, onConnect, compact = false }: {
  detection: Detection; providers: readonly Provider[]; onConnect: (prefill: Prefill) => void; compact?: boolean;
}) {
  const shown = visibleDetected(detection.services ?? [], detection.dismissed, providers);
  if (!shown.length) return null;
  return <section className="detected" data-compact={compact ? "" : undefined} data-testid="detected" aria-labelledby="detected-title">
    <div className="detected-head"><RadarIcon aria-hidden="true" /><div>
      <h3 id="detected-title">Detected on this host</h3>
      <p>Containers that look like services Hobnail can connect to. Nothing is contacted until you test the connection.</p>
    </div></div>
    <ul className="detected-grid">{shown.map((service) => {
      const provider = providers.find((entry) => entry.id === service.provider);
      if (!provider) return null;
      const url = proposedUrl(service);
      return <li key={detectionKey(service)} className="detected-card" data-testid="detected-service" data-provider={service.provider}
        style={{ "--brand": brandColor(provider.color) } as CSSProperties}>
        <ServiceIcon tint name={provider.icon} title={provider.name} enabled={false} />
        <div className="detected-copy">
          <strong>{provider.name}</strong>
          <span className="detected-image" title={service.image}>{detectedSummary(service)}{service.container !== service.provider && <> · {service.container}</>}</span>
          {url && <code className="detected-url">{url}</code>}
          <small>{viaLabel(service)}</small>
        </div>
        <div className="detected-actions">
          <Button size="sm" variant="default" data-testid="detected-connect" icon={<PlugZapIcon aria-hidden="true" />}
            aria-label={`Connect ${provider.name} (${service.container})`} onClick={() => onConnect(prefillFor(service, provider))}>Connect</Button>
          <Button size="icon" variant="ghost" data-testid="detected-dismiss" aria-label={`Dismiss ${provider.name} (${service.container})`} title="Dismiss"
            onClick={() => detection.dismiss(service)}><XIcon aria-hidden="true" /></Button>
        </div>
      </li>;
    })}</ul>
  </section>;
}
