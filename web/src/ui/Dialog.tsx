// Dialog (DESIGN.md §11.7; static/ui.js HB.open / HB.close): the native
// <dialog> opened with showModal(), so the page behind is inert and the
// browser keeps focus inside. Focus moves to `initialFocus` (else the first
// control of the body) and returns to what had it before, when the dialog
// closes.
//
// Closing: Esc, the header's Close button, a click that starts and ends on
// the backdrop, and Back. Each asks `guard()` first: when it says there is
// unsaved input, the footer turns into "Discard your changes? Keep editing /
// Discard" (Esc there means "keep editing"). `onClose` gets why.
//
// Back while open: the dialog does not add a history entry of its own (a
// page whose URL is the dialog, `?edit=…`, already has one). When Back
// leaves the dialog's URL and `guard()` holds input, the dialog puts its URL
// back and asks; otherwise it calls onClose("cancel") after the URL has
// already changed, so a handler that removes `?edit` must replace, never go
// back again.
//
// Toasts wait while any dialog is open (layers.ts). Ctrl/⌘ Enter submits the
// first <form> inside the dialog. Footer buttons that submit a form in the
// body use the `form="<form id>"` attribute.
import { XIcon } from "lucide-react";
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";

import { Button } from "./Button.tsx";
import { dialogOpened } from "./layers.ts";

export type DialogCloseReason = "escape" | "cancel" | "backdrop" | "done";

export interface DialogProps {
  open: boolean;
  title: string;
  description?: ReactNode;
  onClose: (reason: DialogCloseReason) => void;
  /** True while the dialog holds unsaved input: closing asks first. */
  guard?: () => boolean;
  footer?: ReactNode;
  children?: ReactNode;
  initialFocus?: RefObject<HTMLElement | null>;
  testId?: string;
  /** 760 px instead of 560 px. */
  wide?: boolean;
  className?: string;
}

const FOCUSABLE =
  'input:not([type=hidden]):not([disabled]),select:not([disabled]),textarea:not([disabled]),button:not([disabled]),a[href],[tabindex]:not([tabindex="-1"])';

function outside(el: HTMLElement, x: number, y: number): boolean {
  const r = el.getBoundingClientRect();
  return x < r.left || x > r.right || y < r.top || y > r.bottom;
}

export function Dialog({
  open,
  title,
  description,
  onClose,
  guard,
  footer,
  children,
  initialFocus,
  testId,
  wide,
  className,
}: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null);
  const keepRef = useRef<HTMLButtonElement>(null);
  const [asking, setAsking] = useState(false);
  const id = useId();
  // The latest props and state, for the listeners added once per opening.
  const live = useRef({ onClose, guard, open, asking });
  useLayoutEffect(() => {
    live.current = { onClose, guard, open, asking };
  });
  const pending = useRef<DialogCloseReason>("cancel");
  const lastField = useRef<HTMLElement | null>(null);

  /** A close request: asks first when the guard says there is unsaved input. */
  const request = useCallback((reason: DialogCloseReason) => {
    if (live.current.guard?.()) {
      pending.current = reason;
      setAsking(true);
      return;
    }
    setAsking(false);
    live.current.onClose(reason);
  }, []);

  const keepEditing = useCallback(() => {
    setAsking(false);
    const back = lastField.current;
    window.setTimeout(() => (back?.isConnected ? back : ref.current)?.focus(), 0);
  }, []);

  // Open and close the native dialog with the prop; its events while open.
  useEffect(() => {
    const el = ref.current;
    if (!el || !open) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const openUrl = window.location.href;
    if (!el.open) el.showModal();
    const release = dialogOpened();
    const first =
      initialFocus?.current ?? el.querySelector<HTMLElement>(`.dlg-b :is(${FOCUSABLE})`) ?? el.querySelector<HTMLElement>(FOCUSABLE);
    (first ?? el).focus();

    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        if (live.current.asking) keepEditing();
        else request("escape");
      } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
        const form = el.querySelector("form");
        if (form) {
          e.preventDefault();
          form.requestSubmit();
        }
      }
    };
    // Backdrop: a click that both starts and ends outside the box.
    let downOut = false;
    const onDown = (e: PointerEvent) => {
      downOut = e.target === el && outside(el, e.clientX, e.clientY);
    };
    const onClick = (e: MouseEvent) => {
      if (downOut && e.target === el && outside(el, e.clientX, e.clientY)) request("backdrop");
      downOut = false;
    };
    // Esc is handled on keydown; a cancel from elsewhere (a phone's back
    // gesture) is the same request.
    const onCancel = (e: Event) => {
      e.preventDefault();
      if (!live.current.asking) request("escape");
    };
    // The browser closed it anyway (a forced close request): keep the input
    // if there is any.
    const onNativeClose = () => {
      if (!live.current.open) return;
      if (live.current.guard?.()) {
        el.showModal();
        pending.current = "escape";
        setAsking(true);
      } else live.current.onClose("escape");
    };
    const onFocusIn = (e: FocusEvent) => {
      if (e.target instanceof HTMLElement && e.target.closest(".dlg-b")) lastField.current = e.target;
    };
    // Back: see the header comment.
    let restoring = false;
    const onPop = () => {
      if (restoring || !live.current.open) return;
      if (window.location.href === openUrl) return;
      if (live.current.guard?.()) {
        restoring = true;
        window.history.pushState(null, "", openUrl);
        window.dispatchEvent(new PopStateEvent("popstate"));
        restoring = false;
        pending.current = "cancel";
        setAsking(true);
        return;
      }
      live.current.onClose("cancel");
    };
    el.addEventListener("keydown", onKey);
    el.addEventListener("pointerdown", onDown);
    el.addEventListener("click", onClick);
    el.addEventListener("cancel", onCancel);
    el.addEventListener("close", onNativeClose);
    el.addEventListener("focusin", onFocusIn);
    window.addEventListener("popstate", onPop);
    return () => {
      el.removeEventListener("keydown", onKey);
      el.removeEventListener("pointerdown", onDown);
      el.removeEventListener("click", onClick);
      el.removeEventListener("cancel", onCancel);
      el.removeEventListener("close", onNativeClose);
      el.removeEventListener("focusin", onFocusIn);
      window.removeEventListener("popstate", onPop);
      release();
      if (el.open) el.close();
      setAsking(false);
      if (opener?.isConnected && !opener.closest("[inert]")) opener.focus({ preventScroll: true });
    };
  }, [open, initialFocus, request, keepEditing]);

  // The guard bar takes focus on its safe choice.
  useEffect(() => {
    if (asking) keepRef.current?.focus();
  }, [asking]);

  const cls = ["dlg", wide ? "wide" : "", className ?? ""].filter(Boolean).join(" ");
  return (
    <dialog
      ref={ref}
      className={cls}
      aria-labelledby={`${id}-t`}
      aria-describedby={description ? `${id}-d` : undefined}
      data-testid={testId}
    >
      {open && (
        <>
          <div className="dlg-h">
            <div className="dlg-t">
              <h2 id={`${id}-t`}>{title}</h2>
              {description && <p id={`${id}-d`}>{description}</p>}
            </div>
            <button type="button" className="btn icon ghost" aria-label="Close" onClick={() => request("cancel")}>
              <XIcon aria-hidden="true" />
            </button>
          </div>
          <div className="dlg-b">{children}</div>
          {(footer || asking) && (
            <div className="dlg-ft">
              {asking ? (
                <div className="dlg-guard" role="group" aria-label="Unsaved changes">
                  <span>Discard your changes?</span>
                  <Button ref={keepRef} onClick={keepEditing}>
                    Keep editing
                  </Button>
                  <Button
                    className="danger"
                    onClick={() => {
                      setAsking(false);
                      live.current.onClose(pending.current);
                    }}
                  >
                    Discard
                  </Button>
                </div>
              ) : (
                footer
              )}
            </div>
          )}
        </>
      )}
    </dialog>
  );
}
