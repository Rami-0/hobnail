// Prepare heavy cells ahead of scrolling; subscribe to stats only on screen.
import { useEffect, useState, type RefObject } from "react";

type Cb = (near: boolean, visible: boolean) => void;
const callbacks = new WeakMap<Element, Cb>();
let ahead: IntersectionObserver | null = null;
let viewport: IntersectionObserver | null = null;

export function useNear(ref: RefObject<Element | null>): { seen: boolean; visible: boolean } {
  const none = typeof IntersectionObserver === "undefined";
  const [state, setState] = useState({ seen: none, visible: none });
  useEffect(() => {
    const element = ref.current;
    if (!element || none) return;
    ahead ??= new IntersectionObserver((entries) => {
      for (const entry of entries) if (entry.isIntersecting) {
        ahead?.unobserve(entry.target);
        callbacks.get(entry.target)?.(true, false);
      }
    }, { rootMargin: "300px 0px" });
    viewport ??= new IntersectionObserver((entries) => {
      for (const entry of entries) callbacks.get(entry.target)?.(false, entry.isIntersecting);
    });
    callbacks.set(element, (near, visible) => setState((previous) => {
      const next = { seen: previous.seen || near || visible, visible: near ? previous.visible : visible };
      return next.seen === previous.seen && next.visible === previous.visible ? previous : next;
    }));
    ahead.observe(element); viewport.observe(element);
    return () => { ahead?.unobserve(element); viewport?.unobserve(element); callbacks.delete(element); };
  }, [ref, none]);
  return state;
}
