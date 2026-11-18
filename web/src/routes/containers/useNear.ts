// "Is this element on or near the screen?" with one shared
// IntersectionObserver for the whole table. `seen`: it has been near the
// screen once (rows then keep their heavy parts: strip, sparklines);
// `visible`: it is near the screen now (the live cells follow their numbers
// only then, so a stats tick re-renders the rows on screen, not all 150; a
// row that comes back into view reads the current numbers at once).
import { useEffect, useState, type RefObject } from "react";

type Cb = (near: boolean) => void;

let io: IntersectionObserver | null = null;
const cbs = new WeakMap<Element, Cb>();

function observer(): IntersectionObserver | null {
  if (io || typeof IntersectionObserver === "undefined") return io;
  io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) cbs.get(e.target)?.(e.isIntersecting);
    },
    { rootMargin: "100px 0px" },
  );
  return io;
}

export function useNear(ref: RefObject<Element | null>): { seen: boolean; visible: boolean } {
  const none = typeof IntersectionObserver === "undefined";
  const [st, setSt] = useState({ seen: none, visible: none });
  useEffect(() => {
    const el = ref.current;
    const o = observer();
    if (!el || !o) return;
    cbs.set(el, (n) => setSt((s) => (s.visible === n && (s.seen || !n) ? s : { seen: s.seen || n, visible: n })));
    o.observe(el);
    return () => {
      o.unobserve(el);
      cbs.delete(el);
    };
  }, [ref]);
  return st;
}
