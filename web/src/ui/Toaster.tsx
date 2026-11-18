// The toast stack (see toast.ts). Rendered once, by the shell. The region is
// always in the document so screen readers announce what joins it; an
// error toast is its own alert.
import { CircleAlertIcon, CircleCheckIcon, InfoIcon, TriangleAlertIcon, XIcon } from "lucide-react";
import { useEffect, useState, useSyncExternalStore } from "react";

import { Toast as SdkToast, ToastRegion, ToastTitle, ToastAction, ToastClose, createToaster } from "@ui-sdk/react/toast";
import { dismiss, getToasts, subscribeToasts, type ToastItem } from "./toast.ts";

const ICON = {
  ok: <CircleCheckIcon aria-hidden="true" />,
  info: <InfoIcon aria-hidden="true" />,
  warn: <TriangleAlertIcon aria-hidden="true" />,
  bad: <CircleAlertIcon aria-hidden="true" />,
};

function Toast({ t }: { t: ToastItem }) {
  const [toaster] = useState(() => createToaster());
  useEffect(() => {
    toaster(t.text, { id: String(t.id), duration: t.ms ?? Infinity,
      onDismiss: () => dismiss(t.id),
      ...(t.action ? { action: { label: t.action.label, onAction: t.action.run } } : {}),
    });
  }, [t, toaster]);
  return (
    <ToastRegion toaster={toaster}>
      {() => <SdkToast className={`toast ${t.kind}`} data-testid="toast">
        {ICON[t.kind]}<ToastTitle />
        {t.action && <ToastAction className="btn sm ghost" />}
        <ToastClose className="btn sm icon ghost" aria-label="Dismiss"><XIcon aria-hidden="true" /></ToastClose>
      </SdkToast>}
    </ToastRegion>
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
