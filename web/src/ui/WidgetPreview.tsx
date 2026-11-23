import type { CSSProperties, ReactNode } from "react";
import type { Widget } from "../api/gen/Widget.ts";
import { Badge } from "./Badge.tsx";
import { brandColor } from "./brand.ts";
import { ServiceIcon } from "./ServiceIcon.tsx";
import { StructuredWidget } from "./StructuredWidget.ts";

/** "4 × 2": a widget's default board size in grid units. */
export function sizeLabel(widget: Pick<Widget, "default_size">): string | null {
  const size = widget.default_size;
  return size ? `${size.w} × ${size.h}` : null;
}

/** A gallery card: the widget drawn by the real renderer from the server's
 * example data, framed like a board tile and marked as an example. */
export function WidgetPreview({ widget, icon, color, title, children, compact = false }: {
  widget: Widget; icon: string; color: string; title: string; children?: ReactNode; compact?: boolean;
}) {
  const brand = brandColor(color);
  const size = sizeLabel(widget);
  return <article className="widget-preview" data-compact={compact ? "" : undefined} style={brand ? { "--brand": brand } as CSSProperties : undefined}>
    <div className="widget-preview-frame" aria-hidden="true" inert>
      <header className="widget-preview-head">
        <ServiceIcon className="service-icon widget-preview-icon" name={icon} title={title} enabled={false} tint />
        <strong>{widget.name}</strong>
        <span className="widget-preview-example">Example</span>
      </header>
      <div className="widget-content" data-brand={brand ? "" : undefined}><StructuredWidget body={widget.sample} /></div>
    </div>
    <div className="widget-preview-meta">
      <div className="widget-preview-title">
        <h4>{widget.name}</h4>
        {widget.recommended && <Badge variant="secondary">Recommended</Badge>}
        {size && <span className="widget-preview-size" title="Default size on a board (columns × rows)">{size}</span>}
      </div>
      <p>{widget.description}</p>
      {children}
    </div>
  </article>;
}
