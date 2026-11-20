// Generic hit testing ported from scripts/a11y-check.mjs.
export const MEASURE = `((scope) => {
  const root = scope ? document.querySelector(scope) : document;
  const vis = (el) => { const r = el.getBoundingClientRect(); if (r.width < 2 || r.height < 2) return false; const s = getComputedStyle(el); if (s.visibility !== "visible") return false;
    for (let e = el; e; e = e.parentElement) { if (e.hidden || e.inert || getComputedStyle(e).display === "none") return false; if (e.tagName === "DETAILS" && !e.open && e !== el && !e.querySelector(":scope>summary").contains(el)) return false; } return true; };
  const inline = (el) => el.tagName === "A" && getComputedStyle(el).display === "inline" && el.parentElement && el.parentElement.textContent.trim().length > el.textContent.trim().length + 20;
  const name = (el) => (el.getAttribute("aria-label") || el.title || el.textContent || el.placeholder || el.name || "").trim().replace(/\\s+/g, " ").slice(0, 40);
  const sel = (el) => { let s = el.tagName.toLowerCase(); const c = (el.getAttribute("class") || "").trim().split(/\\s+/).filter(Boolean).slice(0, 2).join("."); if (c) s += "." + c; const p = el.parentElement; if (p) { let ps = p.tagName.toLowerCase(); const pc = (p.getAttribute("class") || "").trim().split(/\\s+/).filter(Boolean).slice(0, 1).join("."); if (pc) ps += "." + pc; s = ps + ">" + s; } return s; };
  const mine = (el, x, y) => { if (x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) return false; const h = document.elementFromPoint(x, y); if (!h) return false; if (h === el || el.contains(h)) return true; const l = h.closest("label"); return !!(l && l.control === el); };
  const fits = (el, cx, cy, d) => mine(el, cx - d, cy) && mine(el, cx + d, cy) && mine(el, cx, cy - d) && mine(el, cx, cy + d);
  const x0 = scrollX, y0 = scrollY, out = [];
  for (const el of root.querySelectorAll('a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=tab],[role=switch],[tabindex]:not([tabindex="-1"])')) {
    // Chart markers (.mk) are 24 px by design: a row of them sits on the
    // chart's top edge, one per event, and a 44 px target would cover its
    // neighbours and the plot; each also has its timeline row (a 44 px
    // target) and the crosshair read-out. Left out of the count.
    if (el.classList.contains("skip") || el.classList.contains("mk") || !vis(el) || inline(el)) continue;
    el.scrollIntoView({ block: "center", inline: "center" });
    // A stretched row link's target is its row (.rl).
    let r = (el.classList.contains("row-link") && el.closest(".rl") || el).getBoundingClientRect();
    if ((el.type === "checkbox" || el.type === "radio") && el.closest("label")) { const lr = el.closest("label").getBoundingClientRect(); if (lr.width * lr.height > r.width * r.height) r = lr; }
    const cx = r.left + r.width / 2, cy = r.top + r.height / 2;
    // A stretched row link is a target when some 44 px (24 px) square of
    // its row is its own: rows also hold their own 44 px secondary links (a
    // rule, "alert firing", logs), which take part of the row. Every point
    // of the square must resolve to the row's own link (a link that is not
    // stretched, or text laid over it, does not count).
    if (el.classList.contains("row-link") && el.closest(".rl")) { const hit = (x, y) => mine(el, x, y);
      // (The row's middle is always tried: a 45 px row with a 1 px border
      // has exactly 44 px of link, centred, which a 6 px step can miss.)
      const square = (d) => { const ys = [r.top + r.height / 2]; for (let y = r.top + d; y <= r.bottom - d; y += 6) ys.push(y); for (const y of ys) for (let x = r.left + d; x <= r.right - d; x += 8) if ([[0, 0], [-d, 0], [d, 0], [0, -d], [0, d]].every(([dx, dy]) => hit(x + dx, y + dy))) return true; return false; };
      out.push({ sel: sel(el), name: name(el), w: Math.round(r.width), h: Math.round(r.height), ok44: square(21), ok24: square(11), shell: false }); continue; }
    out.push({ sel: sel(el), name: name(el), w: Math.round(r.width), h: Math.round(r.height), ok44: fits(el, cx, cy, 21), ok24: fits(el, cx, cy, 11),
      shell: !!el.closest("header.top, #bbar, #side, #palette") });
  }
  scrollTo(x0, y0);
  return out;
})`;
