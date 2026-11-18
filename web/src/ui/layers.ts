// Which modal layers are open: Dialog registers itself here while it is
// shown, and the Toaster holds its toasts back while any is open (a toast
// never sits over a dialog; it shows once the dialog closes). Plain store,
// no DOM, so the toast queue logic stays testable.

let open = 0;
const listeners = new Set<() => void>();

export function dialogOpened(): () => void {
  open++;
  for (const l of listeners) l();
  let done = false;
  return () => {
    if (done) return;
    done = true;
    open = Math.max(0, open - 1);
    for (const l of listeners) l();
  };
}

export function openDialogs(): number {
  return open;
}

export function subscribeDialogs(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}
