// The toast stack (see toast.ts). Rendered once, by the shell. The region is
// always in the document so screen readers announce what joins it; an
// error toast is its own alert.
import { CircleAlertIcon, CircleCheckIcon, InfoIcon, TriangleAlertIcon, XIcon } from "lucide-react";
import { useEffect, useRef, useSyncExternalStore } from "react";

import { dismiss, getToasts, subscribeToasts, type ToastItem } from "./toast.ts";

const ICON = {
  ok: <CircleCheckIcon aria-hidden="true" />,
  info: <InfoIcon aria-hidden="true" />,
  warn: <TriangleAlertIcon aria-hidden="true" />,
  bad: <CircleAlertIcon aria-hidden="true" />,
};

function Toast({ t }: { t: ToastItem }) {
  // The timer pauses while the pointer or focus is on the toast.
  const ctl = useRef({ arm: () => {}, hold: () => {} });
  useEffect(() => {
    let timer: number | null = null;
    const hold = () => {
      if (timer !== null) window.clearTimeout(timer);
      timer = null;
    };
    const arm = () => {
      if (t.ms === null || timer !== null) return;
      timer = window.setTimeout(() => dismiss(t.id), t.ms);
    };
    ctl.current = { arm, hold };
    arm();
    return hold;
  }, [t.id, t.ms]);
  const arm = () => ctl.current.arm();
  const hold = () => ctl.current.hold();
  return (
    <div
      className={`toast ${t.kind}`}
      data-testid="toast"
      role={t.kind === "bad" ? "alert" : undefined}
      onPointerEnter={hold}
      onPointerLeave={arm}
      onFocus={hold}
      onBlur={arm}
    >
      {ICON[t.kind]}
      <span>{t.text}</span>
      {t.action && (
        <button
          type="button"
          className="btn sm ghost"
          onClick={() => {
            t.action?.run();
            dismiss(t.id);
          }}
        >
          {t.action.label}
        </button>
      )}
      <button type="button" className="btn sm icon ghost" aria-label="Dismiss" onClick={() => dismiss(t.id)}>
        <XIcon aria-hidden="true" />
      </button>
    </div>
  );
}

export function Toaster() {
  const s = useSyncExternalStore(subscribeToasts, getToasts, getToasts);
  return (
    <output className="toasts" aria-live="polite">
      {s.shown.map((t) => (
        <Toast key={t.id} t={t} />
      ))}
    </output>
  );
}
