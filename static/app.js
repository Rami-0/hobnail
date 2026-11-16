// hobnail client: theme, sidebar, stack tree, command palette, live
// fragment swaps over SSE, live sparklines, chart read-out, the containers
// table view, login. No framework, no build step. Never inserts server or
// user text as HTML except the server's own fragments.
(() => {
  "use strict";
  const doc = document, root = doc.documentElement, body = doc.body;
  const $ = (id) => doc.getElementById(id);
  const $$ = (sel, el) => Array.from((el || doc).querySelectorAll(sel));
  const store = {
    get(k, d) { try { const v = localStorage.getItem(k); return v === null ? d : JSON.parse(v); } catch { return d; } },
    set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch {} },
  };
  const skew = Date.now() / 1000 - Number(body.dataset.now || 0);
  const nowS = () => Date.now() / 1000 - skew;
  const typing = () => { const a = doc.activeElement; return a && (a.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName)); };

  // ---- formatting (mirrors src/util.rs and ui/chart.rs) ----
  function bytes(n) {
    const u = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let v = Math.max(0, n), i = 0;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return i === 0 ? `${Math.round(n)} B` : `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${u[i]}`;
  }
  function fmtVal(u, v) {
    if (!isFinite(v)) return "—";
    switch (u) {
      case "pct": return Math.abs(v) < 10 ? v.toFixed(1) + "%" : v.toFixed(0) + "%";
      case "b": return bytes(v);
      case "bps": return bytes(v) + "/s";
      case "c": return v.toFixed(0) + " °C";
      default: return Math.abs(v) < 10 ? v.toFixed(2) : v.toFixed(1);
    }
  }
  function dur(s) {
    s = Math.max(0, Math.floor(s));
    const d = Math.floor(s / 86400), h = Math.floor(s / 3600) % 24, m = Math.floor(s / 60) % 60;
    return d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : m ? `${m}m` : `${s}s`;
  }

  // ---- greeting and clock (overview) ----
  function tick() {
    const now = new Date(), c = $("clock"), g = $("greeting");
    if (c) c.textContent = now.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" }) + "  " + now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
    if (g && !g.dataset.text) {
      const h = now.getHours();
      g.textContent = h < 5 ? "Good night" : h < 12 ? "Good morning" : h < 18 ? "Good afternoon" : "Good evening";
    }
  }
  tick();
  setInterval(tick, 15000);

  // ---- theme: auto -> dark -> light -> ember -> contrast ----
  // One click per step (owner decision Y9); each step says which theme it
  // switched to, in a toast a phone shows too (NAV-41, A11Y-11).
  const themeBtn = $("theme"), THEMES = ["auto", "dark", "light", "ember", "contrast"];
  const THEME_NAME = { auto: "auto (follows the system)", dark: "dark", light: "light", ember: "ember (warm)", contrast: "high contrast" };
  const THEME_LABEL = { auto: "Auto (follows the system)", dark: "Dark", light: "Light", ember: "Ember", contrast: "High contrast" };
  let themeGone = null;
  function showTheme() {
    const t = root.dataset.theme || "auto";
    // The browser UI (phone status bar, installed app title bar) follows the page.
    const m = doc.querySelector('meta[name="theme-color"]');
    if (m) m.content = getComputedStyle(root).getPropertyValue("--bg").trim() || "#1D1210";
    if (!themeBtn) return;
    const next = THEMES[(THEMES.indexOf(t) + 1) % THEMES.length];
    themeBtn.setAttribute("aria-label", `Theme: ${THEME_NAME[t] || t}. Click for ${THEME_NAME[next]}`);
    themeBtn.title = `Theme: ${THEME_NAME[t] || t} (click for ${THEME_NAME[next]})`;
  }
  function setTheme(next, say) {
    root.dataset.theme = next;
    try { localStorage.setItem("rh-theme", next); } catch {}
    showTheme();
    if (say && window.HB && HB.toast) { if (themeGone) themeGone(); themeGone = HB.toast("Theme: " + (THEME_LABEL[next] || next), { kind: "info", ms: 2500 }); }
  }
  function cycleTheme() {
    setTheme(THEMES[(THEMES.indexOf(root.dataset.theme || "auto") + 1) % THEMES.length], true);
  }
  themeBtn && themeBtn.addEventListener("click", cycleTheme);
  showTheme();

  // ---- sidebar: rail (wide screens, remembered) or overlay/drawer ----
  // Over 1100 px the sidebar is in the page (the toggle makes it a rail);
  // from 700 to 1100 px it is a rail that the toggle opens over the page;
  // under 700 px it is a drawer that only "More" opens (Y3). Open over the
  // page, everything behind is inert and Back closes it (A11Y-03, NAV-33);
  // a closed drawer is inert (A11Y-01; CSS hides it from the tab order
  // without the script too).
  const side = $("side"), railBtn = $("rail-toggle"), moreBtn = $$('[data-act="more"]');
  const wide = () => matchMedia("(min-width:1101px)").matches;
  const phone = () => matchMedia("(max-width:699px)").matches;
  let drawerPushed = false, drawerPop = false, drawerFrom = null;
  function sideState() {
    const open = root.classList.contains("side-open");
    if (side) side.inert = phone() && !open;
    $$(".col, #bbar").forEach((x) => { x.inert = open && !wide(); });
    moreBtn.forEach((b) => b.setAttribute("aria-expanded", String(open)));
    if (railBtn) {
      const label = wide() ? (root.dataset.side === "rail" ? "Expand sidebar" : "Collapse sidebar") : open ? "Close sidebar" : "Open sidebar";
      railBtn.setAttribute("aria-label", label); railBtn.title = label;
      railBtn.setAttribute("aria-pressed", String(wide() ? root.dataset.side === "rail" : open));
    }
  }
  // `entry`: the drawer's history entry is there already (Forward onto it).
  function setOpen(open, from, entry) {
    if (open === root.classList.contains("side-open")) return;
    if (open) drawerFrom = from || doc.activeElement; // focus comes back here
    root.classList.toggle("side-open", open);
    sideState();
    if (open) {
      const f = side && side.querySelector(".nav-item"); f && f.focus();
      if (entry) drawerPushed = true;
      else try { history.pushState({ rhDrawer: 1 }, ""); drawerPushed = true; } catch {}
    } else {
      if (drawerPushed) { drawerPushed = false; drawerPop = true; try { history.back(); } catch { drawerPop = false; } }
      if (drawerFrom && drawerFrom.isConnected && !wide()) drawerFrom.focus({ preventScroll: true });
    }
  }
  // Back closes the drawer; Forward onto its entry opens it again (the
  // entry is never a dead step).
  window.addEventListener("popstate", (e) => {
    if (drawerPop) { drawerPop = false; return; }
    if (root.classList.contains("side-open")) { drawerPushed = false; setOpen(false); }
    else if (e.state && e.state.rhDrawer && !wide()) setOpen(true, null, true);
  });
  // Leaving through a link in the open drawer replaces its history entry,
  // so Back from the next page comes straight back here.
  side && side.addEventListener("click", (e) => {
    const a = e.target.closest && e.target.closest("a[href]");
    if (!a || !drawerPushed || e.defaultPrevented || e.button || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || a.target) return;
    e.preventDefault(); drawerPushed = false; location.replace(a.href);
  });
  window.addEventListener("pageshow", (e) => { if (e.persisted && root.classList.contains("side-open")) { drawerPushed = false; root.classList.remove("side-open"); sideState(); } });
  function toggleSide() {
    if (wide()) {
      const rail = root.dataset.side !== "rail";
      if (rail) root.dataset.side = "rail"; else delete root.dataset.side;
      try { localStorage.setItem("rh-side", rail ? "rail" : "open"); } catch {}
      sideState();
    } else setOpen(!root.classList.contains("side-open"));
  }
  railBtn && railBtn.addEventListener("click", toggleSide);
  // The rail's labels are styled tooltips (NAV-27), not the native title.
  $$(".nav-item[title]").forEach((a) => { a.dataset.rtip = a.title; a.title = ""; });
  $("scrim") && $("scrim").addEventListener("click", () => setOpen(false));
  matchMedia("(min-width:1101px)").addEventListener("change", () => { if (wide()) setOpen(false); sideState(); });
  matchMedia("(max-width:699px)").addEventListener("change", sideState);
  sideState();

  // ---- stack tree: remembered open/closed stacks, filter, current container ----
  const TREE = "rh-tree";
  const here = decodeURIComponent((body.dataset.path || "").replace(/^\/containers\//, "").replace(/\/logs$/, ""));
  function treeApply() {
    const closed = store.get(TREE, {});
    const href = "/containers/" + encodeURIComponent(here);
    $$("#tree details.stack").forEach((d) => {
      const a = $$("a", d).find((x) => x.getAttribute("href") === href);
      if (a) a.setAttribute("aria-current", "page");
      d.open = !!a || !closed[d.dataset.k];
    });
    treeFilter();
  }
  function treeFilter() {
    const inp = $("tree-filter"), v = ((inp && inp.value) || "").trim().toLowerCase();
    let any = false;
    $$("#tree details.stack").forEach((d) => {
      let hit = false;
      const stackHit = v && d.dataset.k.toLowerCase().includes(v);
      $$("li.ctr", d).forEach((li) => {
        const ok = !v || stackHit || li.dataset.s.includes(v);
        li.hidden = !ok; hit = hit || ok;
      });
      d.hidden = !hit; any = any || hit;
      if (v && hit) d.open = true;
    });
    const none = $$("#tree .tree-none")[0];
    none && (none.hidden = any || !v);
  }
  doc.addEventListener("toggle", (e) => {
    const d = e.target;
    if (!(d instanceof HTMLDetailsElement) || !d.classList.contains("stack")) return;
    if ($("tree-filter") && $("tree-filter").value) return;
    const c = store.get(TREE, {});
    if (d.open) delete c[d.dataset.k]; else c[d.dataset.k] = true;
    store.set(TREE, c);
  }, true);
  // The filter text survives navigation until it is cleared (NAV-23);
  // Enter opens the first match, Down moves into the results (NAV-22).
  const tf = $("tree-filter"), TF = "rh-tree-q";
  if (tf) {
    try { tf.value = sessionStorage.getItem(TF) || ""; } catch {}
    tf.addEventListener("input", () => { try { sessionStorage.setItem(TF, tf.value); } catch {} treeFilter(); });
    const hits = () => $$("#tree li.ctr:not([hidden]) > a:first-child").filter((a) => !a.closest("details[hidden]"));
    tf.addEventListener("keydown", (e) => {
      if (e.key === "Enter") { const a = hits()[0]; if (a) { e.preventDefault(); a.click(); } }
      else if (e.key === "ArrowDown") { const a = hits()[0]; if (a) { e.preventDefault(); a.focus(); } }
    });
    side.addEventListener("keydown", (e) => {
      if (!e.target.matches || !e.target.matches("#tree li.ctr > a:first-child") || (e.key !== "ArrowDown" && e.key !== "ArrowUp")) return;
      const list = hits(), i = list.indexOf(e.target), next = list[i + (e.key === "ArrowDown" ? 1 : -1)];
      e.preventDefault();
      if (e.key === "ArrowUp" && i === 0) tf.focus(); else if (next) next.focus();
    });
  }
  treeApply();
  // The open container's item is in view (NAV-24).
  const curItem = doc.querySelector('#tree a[aria-current="page"]'), treeBox = $("tree");
  if (curItem && treeBox) { const r = curItem.getBoundingClientRect(), b = treeBox.getBoundingClientRect(); if (r.top < b.top || r.bottom > b.bottom) treeBox.scrollTop += r.top - b.top - b.height / 3; }

  // "Last 5 checks" under "Why is this red?" starts closed on a phone (it
  // makes the page long there); without the script it stays open.
  if (phone()) $$("details.cd-probes[open]").forEach((d) => { d.open = false; });

  // ---- Settings links open the tab used last in this browser (Y17) ----
  // settings.js remembers it; without the script `/settings` is General.
  // Only a tab that exists (routes::SETTINGS; a test keeps the two equal).
  const SET_TABS = ["general", "containers", "bookmarks", "widgets", "notifications", "appearance", "history", "config", "security"];
  const setTab = (() => { try { const t = localStorage.getItem("rh-set-tab"); return SET_TABS.includes(t) ? "/settings/" + t : "/settings"; } catch { return "/settings"; } })();
  $$('a[href="/settings"]').forEach((a) => { if (!a.closest(".crumb")) a.setAttribute("href", setTab); });

  // ---- account menu and sign out ----
  const acct = $("account");
  if (acct && acct.querySelector("button")) { // signed in (signed out it is a link)
    const btn = acct.querySelector("button"), menu = $("account-menu");
    const show = (on) => { menu.hidden = !on; btn.setAttribute("aria-expanded", String(on)); if (on) { const f = menu.querySelector(".menu-item"); f && f.focus(); } };
    btn.addEventListener("click", () => show(menu.hidden));
    doc.addEventListener("click", (e) => { if (!menu.hidden && !acct.contains(e.target)) show(false); });
    acct.addEventListener("keydown", (e) => { if (e.key === "Escape" && !menu.hidden) { show(false); btn.focus(); } });
  }
  async function logout() {
    const m = doc.querySelector('meta[name="rh-csrf"]');
    try {
      await fetch("/api/logout", { method: "POST", credentials: "same-origin", headers: { "X-RH-CSRF": m ? m.content : "" } });
    } catch {}
    location.href = "/";
  }
  doc.addEventListener("click", (e) => { const b = e.target.closest && e.target.closest('[data-act="logout"]'); if (b) { e.preventDefault(); logout(); } });

  // ---- live sparklines: a ring buffer per key, drawn zero based ----
  const series = new Map(), MAXP = 40;
  function sparkPath(v) {
    if (v.length < 2) return null;
    const lo = Math.min(0, ...v), hi = Math.max(...v), span = hi - lo < 1e-9 ? 1 : hi - lo;
    return v.map((x, i) => `${i ? "L" : "M"}${(i / (v.length - 1) * 100).toFixed(1)} ${(23 - (x - lo) / span * 22).toFixed(1)}`).join("");
  }
  function sparks(el) {
    const t = Date.now();
    $$("svg.spark[data-k]", el).forEach((svg) => {
      const k = svg.dataset.k;
      let s = series.get(k);
      if (!s) {
        s = { v: (svg.dataset.h || "").split(",").filter((x) => x !== "").map(Number).filter(isFinite), t: 0 };
        series.set(k, s);
      }
      const v = parseFloat(svg.dataset.v);
      if (isFinite(v) && svg.dataset.seen !== "1") {
        if (t - s.t < 1500 && s.v.length) s.v[s.v.length - 1] = v; else { s.v.push(v); s.t = t; }
        if (s.v.length > MAXP) s.v.splice(0, s.v.length - MAXP);
      }
      svg.dataset.seen = "1";
      const d = sparkPath(s.v), line = svg.querySelector(".line"), area = svg.querySelector(".area");
      if (d && line) { line.setAttribute("d", d); area && area.setAttribute("d", d + "L100 24L0 24Z"); }
    });
  }

  // ---- relative times ----
  function times(el) {
    $$("time[data-t]", el).forEach((t) => {
      const s = nowS() - Number(t.dataset.t), d = new Date(Number(t.dataset.t) * 1000);
      if (t.dataset.abs) { t.textContent = d.toLocaleString([], { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit", hourCycle: "h23" }); return; }
      t.textContent = s < 10 ? "just now" : dur(s) + " ago";
      t.title = d.toLocaleString();
    });
  }
  setInterval(() => times(doc), 30000);

  // ---- chart axis ticks: the server labels them in UTC; show local time ----
  function localTicks(el) {
    $$(".xaxis .tick[data-t]", el).forEach((t) => {
      const d = new Date(Number(t.dataset.t) * 1000);
      t.textContent = /^\d\d:\d\d$/.test(t.textContent.trim())
        ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" })
        : d.toLocaleDateString([], { day: "numeric", month: "short" });
    });
  }
  localTicks(doc);

  // ---- chart hover read-out ----
  function fmtTime(ts, span) {
    const d = new Date(ts * 1000);
    const hm = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
    return span <= 86400 ? hm : d.toLocaleDateString([], { day: "numeric", month: "short" }) + " " + hm;
  }
  // One crosshair for the whole page: hovering any chart shows the same
  // moment on every chart whose window contains it.
  function cursorAt(fig, t) {
    const cv = fig.querySelector(".canvas"), cur = cv.querySelector(".cursor"), tip = cv.querySelector(".tip");
    const t0 = +fig.dataset.t0, t1 = +fig.dataset.t1, step = +fig.dataset.step || 60;
    if (t === null || t < t0 || t > t1) { cur.hidden = tip.hidden = true; return; }
    const r = cv.getBoundingClientRect(), vals = fig.dataset.v.split(",");
    const i = Math.round((t - t0) / step), raw = vals[i];
    const x = Math.min(r.width, (i * step) / (t1 - t0) * r.width);
    cur.hidden = tip.hidden = false;
    cur.style.left = x + "px";
    tip.style.left = Math.min(r.width - 40, Math.max(40, x)) + "px";
    tip.replaceChildren(el("span", "", fmtTime(t0 + i * step, t1 - t0)), raw === undefined || raw === "" ? "no data" : fmtVal(fig.dataset.u, parseFloat(raw)));
  }
  let hoverT = null;
  function chartHover(e) {
    const fig = e.target.closest && e.target.closest("figure.chart[data-v]");
    const figs = $$("figure.chart[data-v]");
    if (!fig || e.type === "pointerleave") { if (hoverT !== null) { hoverT = null; figs.forEach((f) => cursorAt(f, null)); } return; }
    const r = fig.querySelector(".canvas").getBoundingClientRect();
    if (e.clientX < r.left || e.clientX > r.right) return;
    const t0 = +fig.dataset.t0, t1 = +fig.dataset.t1;
    hoverT = t0 + Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * (t1 - t0);
    figs.forEach((f) => cursorAt(f, hoverT));
    if (drag) dragMove(e);
  }
  doc.addEventListener("pointermove", chartHover);
  doc.addEventListener("pointerleave", (e) => { if (e.target.matches && e.target.matches(".canvas")) chartHover(e); }, true);

  // Drag across a zoomable chart to zoom into that window (the page reads
  // ?from=&to=; ranges stay in the URL so a zoom can be shared).
  let drag = null;
  function dragMove(e) {
    const r = drag.cv.getBoundingClientRect();
    const a = Math.max(0, Math.min(drag.x, e.clientX) - r.left), b = Math.min(r.width, Math.max(drag.x, e.clientX) - r.left);
    drag.sel.hidden = b - a < 3; drag.sel.style.left = a + "px"; drag.sel.style.width = b - a + "px";
  }
  doc.addEventListener("pointerdown", (e) => {
    const cv = e.button === 0 && e.pointerType === "mouse" && e.target.closest && e.target.closest("figure.chart[data-zoom] .canvas");
    if (!cv || e.target.closest(".mk")) return;
    e.preventDefault();
    drag = { cv, fig: cv.closest("figure"), x: e.clientX, sel: cv.querySelector(".sel") };
  });
  doc.addEventListener("pointerup", (e) => {
    if (!drag) return;
    const d = drag; drag = null; d.sel.hidden = true;
    const r = d.cv.getBoundingClientRect();
    if (Math.abs(e.clientX - d.x) < 8) return;
    const t0 = +d.fig.dataset.t0, t1 = +d.fig.dataset.t1;
    const at = (x) => Math.round(t0 + Math.min(1, Math.max(0, (x - r.left) / r.width)) * (t1 - t0));
    const p = new URLSearchParams(location.search);
    p.delete("range"); p.set("from", at(Math.min(d.x, e.clientX))); p.set("to", at(Math.max(d.x, e.clientX)));
    location.href = location.pathname + "?" + p.toString();
  });

  // Legend buttons toggle a chart's layers (avg, max band, yesterday).
  doc.addEventListener("click", (e) => {
    const b = e.target.closest && e.target.closest(".lg[data-ser]");
    if (!b) return;
    const on = b.getAttribute("aria-pressed") === "false";
    b.setAttribute("aria-pressed", String(on));
    b.closest("figure").classList.toggle("off-" + b.dataset.ser, !on);
  });

  // ---- container details: header numbers from the live data ----
  function detailLive() {
    const box = doc.querySelector("[data-cd-name]"), j = $("ctr-live");
    if (!box || !j) return;
    let d;
    try { d = JSON.parse(j.dataset.j || "{}").c[box.dataset.cdName]; } catch { return; }
    if (!d) return;
    const set = (k, txt) => { const el = box.querySelector(`[data-cd="${k}"]`); if (el) el.textContent = txt; };
    set("l", d[0]); set("s", d[2]); set("c", d[3]); set("m", d[4]);
    const l = box.querySelector('[data-cd="l"]');
    if (l) l.className = "st-l lg " + d[1];
    const push = (suffix, v) => {
      const svg = box.querySelector(`svg.spark[data-k$="${suffix}"]`);
      if (svg && v !== null && v !== undefined) { svg.dataset.v = String(v); svg.dataset.seen = ""; }
    };
    push(":cpu", d[5]); push(":mem", d[6]);
    sparks(box);
  }

  // ---- containers table: live numbers in place; a fresh table when its shape changed ----
  let tableBusy = false;
  async function tableLive() {
    const wrap = $("ctable"), j = $("ctr-live");
    if (!wrap || !j) return;
    let d;
    try { d = JSON.parse(j.dataset.j || "{}"); } catch { return; }
    if (d.sig && wrap.dataset.sig && d.sig !== wrap.dataset.sig) {
      if (tableBusy || wrap.contains(doc.activeElement)) return;
      tableBusy = true;
      try {
        const r = await fetch("/containers?frag=table", { credentials: "same-origin" });
        if (r.ok) swap(await r.text());
      } catch {}
      tableBusy = false;
      return;
    }
    const upd = (tr, key, text, v) => {
      const td = tr.querySelector(".c-" + key), num = td && td.querySelector(".num"), svg = td && td.querySelector("svg.spark");
      if (num) num.textContent = text;
      if (v === null || v === undefined) delete tr.dataset[key]; else tr.dataset[key] = String(v);
      if (svg && v !== null && v !== undefined) { svg.dataset.v = String(v); svg.dataset.seen = ""; }
    };
    $$("tr.row", wrap).forEach((tr) => {
      const x = d.c && d.c[tr.dataset.c];
      if (x) { upd(tr, "cpu", x[3], x[5]); upd(tr, "mem", x[4], x[6]); }
    });
    sparks(wrap);
    if (view && (view.sort === "cpu" || view.sort === "mem") && !wrap.contains(doc.activeElement)) applyView(false);
  }

  // ---- containers table: filter, sort, group; remembered; re-applied after swaps ----
  const VIEW = "rh-ctr-view", COLL = "rh-collapsed";
  const bar = $("cbar");
  let view = null;
  function readView() {
    const p = new URLSearchParams(location.search);
    const keys = ["q", "state", "stack", "sort", "dir", "group"];
    const fromUrl = keys.some((k) => p.has(k));
    const v = fromUrl ? {} : store.get(VIEW, {});
    keys.forEach((k) => { if (p.has(k)) v[k] = p.get(k); });
    if (!fromUrl) delete v.q;
    return v;
  }
  function grouped() { return view.group ? view.group === "1" : bar.dataset.defaultGroup === "1"; }
  function isDesc() { return view.dir ? view.dir === "desc" : view.sort === "cpu" || view.sort === "mem"; }
  function canonical() {
    const p = new URLSearchParams();
    if (view.q) p.set("q", view.q);
    ["state", "stack", "sort", "dir", "group"].forEach((k) => { if (view[k]) p.set(k, view[k]); });
    const s = p.toString();
    history.replaceState(null, "", "/containers" + (s ? "?" + s : ""));
    const keep = Object.assign({}, view); delete keep.q;
    store.set(VIEW, keep);
  }
  function matches(tr) {
    const st = view.state || "";
    if (st === "running" && tr.dataset.run !== "1") return false;
    if (st === "stopped" && tr.dataset.run === "1") return false;
    if (st === "problem" && tr.dataset.prob !== "1") return false;
    if (view.stack && tr.dataset.k !== view.stack) return false;
    const terms = (view.q || "").toLowerCase().split(/\s+/).filter(Boolean);
    return terms.every((t) => tr.dataset.s.includes(t));
  }
  function cmp(a, b) {
    const s = view.sort || "name", desc = isDesc();
    const name = (x) => x.dataset.n;
    let o = 0;
    if (s === "cpu" || s === "mem") {
      const x = parseFloat(a.dataset[s]), y = parseFloat(b.dataset[s]);
      const xa = isFinite(x), ya = isFinite(y);
      if (xa && ya) o = desc ? y - x : x - y; else if (xa) o = -1; else if (ya) o = 1;
    } else {
      const key = { name: name, stack: (x) => x.dataset.kt.toLowerCase(), image: (x) => x.dataset.img, status: (x) => x.dataset.st }[s] || name;
      const x = key(a), y = key(b);
      o = x < y ? -1 : x > y ? 1 : 0;
      if (desc) o = -o;
    }
    return o || (name(a) < name(b) ? -1 : name(a) > name(b) ? 1 : 0);
  }
  function wrapOf(t) { return t.closest("#ctable") || doc; }
  function el(tag, cls, text) { const e = doc.createElement(tag); if (cls) e.className = cls; if (text !== undefined) e.textContent = text; return e; }
  function svgUse(id, cls) {
    const s = doc.createElementNS("http://www.w3.org/2000/svg", "svg");
    s.setAttribute("class", "i" + (cls ? " " + cls : "")); s.setAttribute("aria-hidden", "true");
    const u = doc.createElementNS("http://www.w3.org/2000/svg", "use"); u.setAttribute("href", "#i-" + id);
    s.append(u); return s;
  }
  function applyView(store_) {
    const table = doc.querySelector("#ctable table.ctbl");
    if (!table || !view) return;
    const rows = $$("tr.row", table);
    const none = table.querySelector("tbody.none"), pool = table.querySelector("tbody.pool");
    $$("tbody.grp, tbody.flat", table).forEach((t) => t.remove());
    const hit = rows.filter(matches).sort(cmp), miss = rows.filter((r) => !matches(r));
    const g = grouped(), coll = store.get(COLL, {});
    const cols = table.querySelectorAll("thead th").length;
    if (g) {
      const groups = new Map();
      hit.forEach((r) => { const k = r.dataset.gi; if (!groups.has(k)) groups.set(k, []); groups.get(k).push(r); });
      [...groups.keys()].sort((a, b) => a - b).forEach((gi) => {
        const rs = groups.get(gi), k = rs[0].dataset.k;
        const all = rows.filter((r) => r.dataset.k === k), run = all.filter((r) => r.dataset.run === "1").length;
        const tb = el("tbody", "grp"); tb.dataset.k = k;
        const closed = k in coll ? coll[k] : rs[0].hasAttribute("data-gc");
        if (closed && !view.q && !view.stack) tb.classList.add("closed");
        const tr = el("tr", "grp-h"), th = el("th"); th.colSpan = cols; th.scope = "rowgroup";
        const b = el("button", "grp-t"); b.type = "button";
        b.setAttribute("aria-expanded", String(!tb.classList.contains("closed")));
        b.append(svgUse("chev-d"), el("span", "grp-n", rs[0].dataset.kt), el("span", "cnt num", `${run}/${all.length}`));
        const gs = $$(".gstrips > span", wrapOf(table)).find((s) => s.dataset.k === k);
        if (gs && gs.firstElementChild) { const sp = el("span", "grp-strip"); sp.append(gs.firstElementChild.cloneNode(true)); b.append(sp); }
        th.append(b); tr.append(th); tb.append(tr, ...rs);
        table.insertBefore(tb, none);
      });
    } else {
      const tb = el("tbody", "flat"); tb.append(...hit); table.insertBefore(tb, none);
    }
    pool.append(...miss);
    none.hidden = hit.length > 0;
    // Nothing matches: the hidden rows stay laid out but collapsed (no
    // height, not focusable, not read), so the columns keep their widths
    // and the header does not re-flow around the empty state.
    pool.hidden = hit.length > 0; pool.classList.toggle("sizer", !hit.length);
    // Nothing shown: good news for an empty state filter on its own, else "no match" (OV-24).
    none.dataset.why = !view.q && !view.stack && (view.state === "stopped" || view.state === "problem") ? view.state : "filter";
    table.classList.toggle("grouped", g);
    const shown = table.querySelector("caption .shown"); shown && (shown.textContent = hit.length);
    // controls
    const desc = isDesc();
    $$("th[data-sort]", table).forEach((th) => {
      const on = (view.sort || "name") === th.dataset.sort;
      th.setAttribute("aria-sort", on ? (desc ? "descending" : "ascending") : "none");
      const u = th.querySelector(".sort-i use"); u && u.setAttribute("href", on && desc ? "#i-arrow-down" : "#i-arrow-up");
    });
    $$("#cstate a").forEach((a) => a.dataset.state === (view.state || "") ? a.setAttribute("aria-current", "true") : a.removeAttribute("aria-current"));
    $$("#cgroup a").forEach((a) => (a.dataset.group === "1") === g ? a.setAttribute("aria-current", "true") : a.removeAttribute("aria-current"));
    const cnt = { "": rows.length, running: 0, stopped: 0, problem: 0 };
    rows.forEach((r) => { r.dataset.run === "1" ? cnt.running++ : cnt.stopped++; if (r.dataset.prob === "1") cnt.problem++; });
    $$("#cstate .n").forEach((n) => { n.textContent = cnt[n.dataset.n]; });
    const sel = $("cstack"); if (sel && sel.value !== (view.stack || "")) sel.value = view.stack || "";
    const q = $("cq"); if (q && doc.activeElement !== q && q.value !== (view.q || "")) q.value = view.q || "";
    if (store_) canonical();
    sparks(table);
  }
  if (bar) {
    view = readView();
    applyView(location.search === "" && Object.keys(view).length > 0);
    bar.addEventListener("submit", (e) => e.preventDefault());
    $("cq").addEventListener("input", (e) => { view.q = e.target.value.trim(); applyView(true); });
    $("cstack").addEventListener("change", (e) => { view.stack = e.target.value; applyView(true); });
    bar.addEventListener("click", (e) => {
      const a = e.target.closest("a[data-state], a[data-group]");
      if (!a) return;
      e.preventDefault();
      if ("state" in a.dataset) view.state = a.dataset.state; else view.group = a.dataset.group;
      applyView(true);
    });
    doc.addEventListener("click", (e) => {
      const th = e.target.closest && e.target.closest("#ctable th[data-sort] a");
      if (th) {
        e.preventDefault();
        const k = th.parentNode.dataset.sort, on = (view.sort || "name") === k;
        const def = k === "cpu" || k === "mem";
        const desc = on ? !isDesc() : def;
        view.sort = k === "name" ? "" : k;
        view.dir = desc === def ? "" : desc ? "desc" : "asc";
        applyView(true);
        return;
      }
      const gt = e.target.closest && e.target.closest("#ctable .grp-t");
      if (gt) {
        const tb = gt.closest("tbody"), closed = !tb.classList.contains("closed");
        tb.classList.toggle("closed", closed); gt.setAttribute("aria-expanded", String(!closed));
        if (!closed) HB.unfold($$("tr.row", tb));
        const c = store.get(COLL, {}); c[tb.dataset.k] = closed; store.set(COLL, c);
        return;
      }
      const clr = e.target.closest && e.target.closest("#ctable [data-clear]");
      if (clr) { e.preventDefault(); view = { group: view.group }; applyView(true); }
    });
  }

  // ---- command palette ----
  const pal = $("palette"), palQ = $("pal-q"), palList = $("pal-list");
  let palData = null, palAt = 0, palItems = [], palSel = 0, palReturn = null;
  const KIND = { p: "Page", s: "Settings", c: "Container", b: "Bookmark", a: "Action", l: "Logs", o: "Open", w: "Search" };
  async function palLoad() {
    if (palData && Date.now() - palAt < 30000) return;
    try {
      const r = await fetch("/api/palette", { credentials: "same-origin" });
      palData = (await r.json()).items || []; palAt = Date.now();
    } catch { palData = palData || []; }
  }
  // A Settings dialog by its URL (NAV-52): opened in place when this page
  // carries it (Alerts: channels and rules; Overview: bookmark, widget),
  // else its page, which opens it on load.
  function dialogAt(href) {
    const want = new URLSearchParams(href.split("?")[1] || "");
    const k = want.has("edit") ? "edit" : "add";
    const t = window.HB && HB.open && $$("[data-dlg][href]").find((a) => new URLSearchParams((a.getAttribute("href").split("?")[1] || "").split("#")[0]).get(k) === want.get(k));
    if (t) t.click(); else location.href = href;
  }
  function actions() {
    const signed = !!doc.querySelector('meta[name="rh-csrf"]');
    const have = new Set((palData || []).filter((x) => x.t === "p").map((x) => x.u));
    const pages = [["Host", "/host"], ["Logs", "/logs"]].filter((p) => !have.has(p[1])).map((p) => ({ t: "p", n: p[0], u: p[1] }));
    // The top tasks, signed in only (they open the Settings dialogs).
    const tasks = signed ? [
      ["Add bookmark", "/settings/bookmarks?add=bookmark"], ["Add widget", "/settings/widgets?add=widget"],
      ["Add channel", "/alerts?add=channel"], ["Add alert rule", "/alerts?add=rule"],
    ].map(([n, u]) => ({ t: "a", n, s: "Settings", k: "new create", run: () => dialogAt(u) })) : [];
    // One action per theme (NAV-55).
    const themes = THEMES.map((t) => ({ t: "a", n: "Theme: " + THEME_LABEL[t], k: "theme colour color " + t, run: () => setTheme(t, true) }));
    return pages.concat(tasks, themes, [
      { t: "a", n: "Keyboard shortcuts", s: "?", run: keysOpen },
      signed ? { t: "a", n: "Sign out", run: logout } : { t: "a", n: "Sign in", u: "/login?next=" + encodeURIComponent(location.pathname + location.search) },
    ]);
  }
  function score(it, q) {
    const n = it.n.toLowerCase();
    if (!q) return 1;
    if (n === q) return 100;
    if (n.startsWith(q)) return 50;
    const i = n.indexOf(q);
    if (i > 0 && /[\s\-_./:]/.test(n[i - 1])) return 30;
    if (i >= 0) return 20;
    if ((it.k || "").toLowerCase().includes(q) || (it.s || "").toLowerCase().includes(q)) return 8;
    return 0;
  }
  // A container brings its own rows, right under it (NAV-50, NAV-51):
  // "Logs: X", "Open service ↗" when it has a link, and, signed in,
  // "Edit X" (its override dialog).
  function withRows(x, signed) {
    if (x.t !== "c") return [x];
    const out = [x];
    if (x.d) out.push({ t: "l", n: "Logs: " + x.n, s: x.s, u: x.d + "/logs" });
    if (x.u) out.push({ t: "o", n: "Open service ↗", s: x.n, u: x.u, ext: true });
    if (signed && x.d) out.push({ t: "a", n: "Edit " + x.n, s: "name, group, icon, link", run: () => dialogAt("/settings/containers?edit=override:" + x.d.slice("/containers/".length)) });
    return out;
  }
  function palRender() {
    const q = palQ.value.trim().toLowerCase();
    const signed = !!doc.querySelector('meta[name="rh-csrf"]');
    const base = (palData || []).concat(actions());
    let list;
    if (!q) {
      // Pages, bookmarks and actions before the long container list (NAV-54).
      const order = { p: 0, b: 1, a: 2, c: 3 };
      list = base.filter((x) => x.t in order).sort((a, b) => order[a.t] - order[b.t]);
    } else {
      // "Logs: X" and "Edit X" are found by typing them too.
      const ctrs = (palData || []).filter((x) => x.t === "c" && x.d);
      const extra = ctrs.flatMap((x) => withRows(x, signed).slice(1).filter((r) => r.t !== "o"));
      const ranked = (xs, min) => xs.map((x) => [score(x, q), x]).filter((x) => x[0] >= min).sort((a, b) => b[0] - a[0]).map((x) => x[1]);
      const seen = new Set();
      const put = (r) => { const id = r.t + r.n + (r.u || ""); if (!seen.has(id)) { seen.add(id); list.push(r); } };
      list = [];
      for (const x of ranked(base, 1)) withRows(x, signed).forEach(put);
      ranked(extra, 20).forEach(put);
      list.push({ t: "w", n: palQ.value.trim(), s: "search the web", web: true });
    }
    palItems = list.slice(0, 60);
    // A query that matches nothing selects nothing: Enter never sends a
    // typo to an outside search engine; arrow down to choose it (NAV-57).
    palSel = q && palItems.length === 1 ? -1 : 0;
    palList.replaceChildren();
    palItems.forEach((it, i) => {
      const li = el("li"); li.id = "pal-" + i; li.setAttribute("role", "option");
      const pt = el("span", "pt" + (it.t === "c" || it.t === "l" ? " mono" : ""));
      const n = it.n, at = q ? n.toLowerCase().indexOf(q) : -1;
      if (at >= 0 && it.t !== "w") pt.append(n.slice(0, at), el("b", "", n.slice(at, at + q.length)), n.slice(at + q.length)); else pt.textContent = n;
      li.append(el("span", "pk", KIND[it.t]), pt);
      if (it.s) li.append(el("span", "ps", it.t === "c" && it.r === false ? it.s + " · stopped" : it.s));
      li.addEventListener("click", () => { palSel = i; palGo(); });
      li.addEventListener("pointermove", () => { if (palSel !== i) { palSel = i; palMark(); } });
      palList.append(li);
    });
    // Nothing in Hobnail matches: say so above the list (the web row stays).
    let none = $("pal-none");
    if (!none) { none = el("p", "pal-none", "No page, container, bookmark or action matches. Arrow down, then Enter, searches the web."); none.id = "pal-none"; none.setAttribute("role", "status"); palList.before(none); }
    none.hidden = !(q && palItems.length === 1);
    palMark();
  }
  function palMark() {
    $$("li[role=option]", palList).forEach((li, i) => li.setAttribute("aria-selected", String(i === palSel)));
    const cur = $("pal-" + palSel);
    palQ.setAttribute("aria-activedescendant", cur ? cur.id : "");
    cur && cur.scrollIntoView({ block: "nearest" });
  }
  function open(u, ext) {
    if (ext || /^[a-z][a-z0-9+.-]*:/i.test(u)) window.open(u, "_blank", "noopener"); else location.href = u;
  }
  function palGo() {
    const it = palItems[palSel];
    if (!it) return;
    palClose();
    if (it.run) return it.run();
    if (it.web) {
      const v = it.n;
      if (/^https?:\/\//i.test(v)) return open(v, true);
      return open((body.dataset.searchUrl || "https://duckduckgo.com/?q={q}").replace("{q}", encodeURIComponent(v)), true);
    }
    // A container opens its page in Hobnail; its service is its own row.
    if (it.t === "c") return open(it.d || it.u, !it.d);
    open(it.u === "/settings" ? setTab : it.u, it.t === "b" || it.ext);
  }
  async function palOpen() {
    if (!pal || !pal.hidden) return;
    palReturn = doc.activeElement;
    pal.hidden = false; palQ.value = ""; palQ.focus();
    $("cmd") && $("cmd").setAttribute("aria-expanded", "true");
    palRender();
    await palLoad();
    if (!pal.hidden) palRender();
  }
  function palClose() {
    if (!pal || pal.hidden) return;
    pal.hidden = true;
    $("cmd") && $("cmd").setAttribute("aria-expanded", "false");
    palReturn && palReturn.focus && palReturn.focus({ preventScroll: true });
  }
  if (pal) {
    $("cmd") && $("cmd").addEventListener("click", palOpen);
    palQ.addEventListener("input", palRender);
    pal.addEventListener("mousedown", (e) => { if (e.target === pal) palClose(); });
    pal.addEventListener("keydown", (e) => {
      const n = palItems.length, to = (i) => { e.preventDefault(); palSel = Math.max(0, Math.min(n - 1, i)); palMark(); };
      if (e.key === "Escape") { e.preventDefault(); palClose(); }
      else if (e.key === "ArrowDown") { e.preventDefault(); palSel = (palSel + 1) % Math.max(n, 1); palMark(); }
      else if (e.key === "ArrowUp") { e.preventDefault(); palSel = palSel <= 0 ? n - 1 : palSel - 1; palMark(); }
      else if (e.key === "Home" && n) to(0); // NAV-58
      else if (e.key === "End" && n) to(n - 1);
      else if (e.key === "PageDown" && n) to(palSel + 8);
      else if (e.key === "PageUp" && n) to(palSel - 8);
      else if (e.key === "Enter") { e.preventDefault(); palGo(); }
      else if (e.key === "Tab") { e.preventDefault(); palQ.focus(); } // focus stays in the dialog
    });
    pal.addEventListener("click", (e) => { if (e.target.closest && e.target.closest("[data-act=pal-close]")) palClose(); });
  }
  // ---- keyboard map: g+key pages, j/k rows, / filter, l logs, ? help ----
  const keys = $("keys");
  function keysOpen() { if (keys && !keys.open) { keys.showModal ? keys.showModal() : (keys.open = true); } }
  doc.addEventListener("click", (e) => {
    const a = e.target.closest && e.target.closest("[data-act]");
    if (!a) return;
    if (a.dataset.act === "keys") keysOpen();
    else if (a.dataset.act === "keys-close") keys.close();
    else if (a.dataset.act === "more" && !(e.ctrlKey || e.metaKey || e.shiftKey || e.button)) { e.preventDefault(); setOpen(true, a); }
    else if (a.dataset.act === "side-close") setOpen(false);
    else if (a.dataset.act === "rail-stacks" && !(e.ctrlKey || e.metaKey || e.shiftKey || e.button)) {
      // NAV-27: from the rail to the stacks: expand, focus the filter.
      e.preventDefault();
      if (wide()) toggleSide(); else setOpen(true, a);
      const f = $("tree-filter"); f && f.focus();
    }
  });
  const GO = { o: "/", c: "/containers", h: "/host", l: "/logs", a: "/alerts", s: setTab };
  let gAt = 0;
  const shown = (x) => x.offsetParent !== null && !x.closest("[hidden]");
  function krows() {
    const t = $("ctable");
    const sel = t ? "#ctable tbody:not(.pool) tr.row" : "main .attn .rows li, main .strip-list li, main ul.events li, main .tbl tbody tr, main .incident";
    return $$(sel).filter(shown);
  }
  function kmove(d) {
    const rows = krows();
    if (!rows.length) return;
    let i = rows.findIndex((r) => r.classList.contains("kfocus"));
    if (i < 0) i = rows.findIndex((r) => r.contains(doc.activeElement));
    rows.forEach((r) => r.classList.remove("kfocus"));
    i = i < 0 ? (d > 0 ? 0 : rows.length - 1) : Math.max(0, Math.min(rows.length - 1, i + d));
    const r = rows[i], a = r.querySelector("a[href]");
    r.classList.add("kfocus");
    a ? a.focus({ preventScroll: true }) : r.setAttribute("tabindex", "-1");
    r.scrollIntoView({ block: "nearest" });
  }
  function logsTarget() {
    const r = doc.querySelector(".kfocus[data-c]") || (doc.activeElement && doc.activeElement.closest && doc.activeElement.closest("[data-c]"));
    if (r) return "/containers/" + encodeURIComponent(r.dataset.c) + "/logs";
    const a = doc.activeElement && doc.activeElement.closest && doc.activeElement.closest('a[href^="/containers/"]');
    const m = (a ? a.getAttribute("href") : body.dataset.path || "").match(/^\/containers\/([^/?#]+)$/);
    return m ? "/containers/" + m[1] + "/logs" : null;
  }
  doc.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") { e.preventDefault(); pal && pal.hidden ? palOpen() : palClose(); return; }
    if (e.key === "Escape" && keys && keys.open) { e.preventDefault(); keys.close(); return; }
    if (e.key === "Escape" && root.classList.contains("side-open")) { setOpen(false); return; }
    if (typing() || e.ctrlKey || e.metaKey || e.altKey || (pal && !pal.hidden) || (keys && keys.open)) return;
    const k = e.key;
    if (Date.now() - gAt < 1200 && GO[k]) { e.preventDefault(); gAt = 0; location.href = GO[k]; return; }
    gAt = 0;
    if (k === "g") { gAt = Date.now(); return; }
    if (k === "/") {
      e.preventDefault();
      const f = [$("cq"), $("lpq"), $("tree-filter")].find((x) => x && shown(x));
      f ? (f.focus(), f.select && f.select()) : palOpen();
    } else if (k === "?") { e.preventDefault(); keysOpen(); }
    else if (k === "j" || k === "k") { e.preventDefault(); kmove(k === "j" ? 1 : -1); }
    else if (k === "l") { const u = logsTarget(); if (u) { e.preventDefault(); location.href = u; } }
  });
  doc.addEventListener("focusin", (e) => { $$(".kfocus").forEach((r) => { if (!r.contains(e.target)) r.classList.remove("kfocus"); }); });
  if (navigator.platform && /Mac/.test(navigator.platform)) $$(".cmd-k").forEach((k) => { k.textContent = "⌘K"; });

  // ---- login: JSON post, then a same-origin redirect only ----
  // Rules from the security review (mirrored by ui::fmt::login_next): no
  // control characters or spaces (browsers strip TAB/CR/LF, so "/\t/evil"
  // would become "//evil"), a leading "/", no leading "//", no backslash,
  // the decoded form must pass too, and the resolved URL must stay on this
  // origin.
  function safeNext(n) {
    const ok = (s) => typeof s === "string" && !/[\u0000-\u001f\u007f-\u009f ]/.test(s) && s[0] === "/" && s[1] !== "/" && s.indexOf("\\") < 0;
    let dec;
    try { dec = decodeURIComponent(n); } catch { return "/"; }
    if (!ok(n) || !ok(dec) || n.length > 512) return "/";
    try { const u = new URL(n, location.origin); return u.origin === location.origin ? u.pathname + u.search + u.hash : "/"; } catch { return "/"; }
  }
  // A sign-in link carries the page's #t= fragment through the login (LG-11).
  if (location.hash) $$("a.login-next, .acct-in").forEach((a) => { if (a.href.includes("?next=")) a.href += encodeURIComponent(location.hash); });
  const login = $("login");
  if (login) {
    login.addEventListener("submit", async (e) => {
      e.preventDefault();
      const msg = $("login-msg"), btn = login.querySelector("button");
      msg.className = "form-msg"; msg.textContent = ""; btn.disabled = true;
      try {
        const r = await fetch("/api/login", {
          method: "POST", credentials: "same-origin",
          headers: { "Content-Type": "application/json", Accept: "application/json" },
          body: JSON.stringify({ password: $("pw").value }),
        });
        let j = {};
        try { j = await r.json(); } catch {}
        if (r.ok && j.ok) {
          msg.className = "form-msg ok"; msg.textContent = "Signed in.";
          location.replace(safeNext(new URLSearchParams(location.search).get("next") || "/"));
          return;
        }
        $("pw").setAttribute("aria-invalid", "true");
        // Too many attempts: the wait the server asks for, counted down, and
        // the button off until then (ROB-18).
        if (r.status === 429) {
          let left = Math.max(1, Math.ceil(Number(j.retry_after || r.headers.get("retry-after")) || 1));
          const say = () => { msg.textContent = `Too many attempts. Try again in ${left} s.`; };
          say(); $("pw").select();
          const t = setInterval(() => { if (--left > 0) return say(); clearInterval(t); msg.textContent = "You can try again now."; btn.disabled = false; }, 1000);
          return;
        }
        msg.textContent = r.status === 401 ? "Wrong password." : `Sign-in failed (HTTP ${r.status}).`;
      } catch { msg.textContent = "Could not reach the server. Your password was not sent; try again."; }
      btn.disabled = false; $("pw").select();
    });
    $("pw").addEventListener("input", () => $("pw").removeAttribute("aria-invalid"));
  }

  // ---- live updates over SSE ----
  const live = $("live");
  // Live updates that stop (stream refused, server gone) say so on every
  // page, phones included, after a grace period that hides quick reconnects;
  // the pill goes when the stream is back (ROB-13). A paused hidden tab is
  // not stale. The stream is retried for as long as the page is open (see
  // connect), so the pill says it is trying, and goes away by itself.
  let offT = null, offPill = null;
  function setLive(on, text) {
    if (live) {
      live.querySelector(".dot").className = "dot " + (on ? "ok" : "off");
      live.querySelector(".live-t").textContent = text;
      live.title = "Live updates: " + text; // the rail shows only the dot
    }
    if (on || text !== "reconnecting") { clearTimeout(offT); offT = null; if (offPill) { offPill.remove(); offPill = null; root.classList.remove("stale"); } return; }
    if (offT || offPill) return; // retries keep failing: the first one starts the clock
    offT = setTimeout(() => {
      offT = null;
      const at = new Date(lastLive).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
      offPill = el("div", "live-off"); offPill.setAttribute("role", "status");
      offPill.append(el("span", "dot"), el("span", "", `Live updates stopped at ${at}. Trying to reconnect; this page may be out of date.`));
      body.append(offPill); root.classList.add("stale");
    }, 4000);
  }
  // ---- keep the reader's place while live data changes the page ----
  // Live updates replace whole sections every few seconds. When a section
  // above the reading position changes height, everything below moves.
  // Chrome's CSS scroll anchoring cannot help (its anchor node is the one
  // being replaced) and Safari on iOS has no scroll anchoring at all, so on
  // phones the page kept sliding away. So: (1) updates wait while the user
  // is touching or scrolling, (2) every update is anchored by hand: note
  // what is at the top of the screen, change the DOM, scroll by however much
  // that moved, (3) focus is restored without scrolling, (4) the sidebar
  // tree keeps its own scroll position.
  let selfScroll = false, busyUntil = 0, flushT = null;
  const held = new Map();
  const busy = () => { if (!selfScroll) busyUntil = Date.now() + 1500; };
  ["touchstart", "touchmove", "wheel", "scroll"].forEach((ev) => window.addEventListener(ev, busy, { passive: true }));
  function anchorNow() {
    if (window.scrollY < 4) return null; // at the very top nothing can jump
    const top = doc.querySelector(".top");
    const y = (top ? top.getBoundingClientRect().bottom : 0) + 6;
    const el = doc.elementFromPoint(Math.min(innerWidth / 2, 160), y);
    if (!el || el === body || el === doc.documentElement) return null;
    // The nearest ancestor with an id survives a swap by id even when `el`
    // itself is inside the replaced section.
    let idEl = el.parentElement;
    while (idEl && idEl !== body && !idEl.id) idEl = idEl.parentElement;
    const hasId = idEl && idEl !== body;
    return { el, top: el.getBoundingClientRect().top, id: hasId ? idEl.id : null, idTop: hasId ? idEl.getBoundingClientRect().top : 0 };
  }
  function keepAnchor(a) {
    if (!a) return;
    let d = 0;
    if (a.el.isConnected) d = a.el.getBoundingClientRect().top - a.top;
    else if (a.id && $(a.id)) d = $(a.id).getBoundingClientRect().top - a.idTop;
    if (Math.abs(d) < 1) return;
    selfScroll = true;
    window.scrollBy(0, d);
    requestAnimationFrame(() => requestAnimationFrame(() => { selfScroll = false; }));
  }
  function swapNodes(nodes) {
    const anchor = anchorNow();
    const tree = $("tree"), treeTop = tree ? tree.scrollTop : 0;
    const active = doc.activeElement, aid = active && active.id;
    for (const fresh of nodes) {
      const old = fresh.id && $(fresh.id);
      if (!old) continue;
      // Keyboard users inside the table or tree keep their place; the next
      // update after focus leaves brings it up to date.
      if ((fresh.id === "ctable" || fresh.id === "tree") && old.contains(active)) continue;
      const before = $$(".tw", old).map((x) => x.textContent);
      // The live "Needs attention" is rendered without a session, so it has
      // no probe lines (NAV-11): a signed-in page keeps the one it was
      // served with only while the same failing run goes on (same
      // `data-since`); a new run or a recovery drops it.
      if (fresh.id === "ov-attn") $$(".probe[data-since]", old).forEach((p) => {
        const c = p.closest("li") && p.closest("li").querySelector("[data-c]");
        const n = c && $$("[data-c]", fresh).find((x) => x.dataset.c === c.dataset.c);
        const why = n && n.parentNode.querySelector(".why");
        if (why && !why.querySelector(".probe") && why.dataset.since === p.dataset.since) why.append(p.cloneNode(true));
      });
      // The firing list too: signed in, a rule's name opens its dialog.
      if (fresh.id === "al-live" && $("ru-list")) $$("b[data-rule]", fresh).forEach((b) => {
        const a = doc.createElement("a");
        a.className = "inc-rule-a"; a.dataset.rule = b.dataset.rule; a.textContent = b.textContent;
        a.href = "/alerts?edit=rule:" + encodeURIComponent(b.dataset.rule);
        b.replaceWith(a);
      });
      // In place (HB.morph): focus, selection, scroll and open <details>
      // stay. The table is rebuilt by the script from its rows, so it is
      // swapped whole (it is skipped above while it holds focus).
      const now = fresh.id === "ctable" ? (old.replaceWith(fresh), fresh) : HB.morph(old, fresh);
      tween(now, before);
      if (now.id === "tree") { treeApply(); now.scrollTop = treeTop; }
      else if (now.id === "ctable") applyView(false);
      else if (now.id === "ctr-live") { detailLive(); tableLive(); }
      else { sparks(now); times(now); localTicks(now); }
    }
    keepAnchor(anchor);
    // Never scroll to restore focus (it used to pull the page to whatever
    // was tapped last).
    if (aid && $(aid) && doc.activeElement !== $(aid)) $(aid).focus({ preventScroll: true });
    alarm();
  }
  // Numbers that changed count to their new value over 160 ms (whole
  // numbers only; nothing under prefers-reduced-motion).
  const calm = matchMedia("(prefers-reduced-motion: reduce)");
  function tween(fresh, before) {
    if (calm.matches) return;
    $$(".tw", fresh).forEach((x, i) => {
      const a = parseFloat(before[i]), b = parseFloat(x.textContent);
      if (!isFinite(a) || !isFinite(b) || a === b || !/^\d+$/.test(x.textContent.trim())) return;
      const t0 = performance.now(), dir = b > a ? "up" : "down";
      x.classList.add(dir);
      const step = (t) => {
        const f = Math.min(1, (t - t0) / 160);
        x.textContent = String(Math.round(a + (b - a) * (1 - (1 - f) * (1 - f))));
        if (f < 1) requestAnimationFrame(step); else setTimeout(() => x.classList.remove(dir), 600);
      };
      requestAnimationFrame(step);
    });
  }

  // ---- alarm tab: problem count in the title and the favicon ----
  const baseTitle = doc.title, icon = doc.querySelector('link[rel="icon"]'), baseIcon = icon && icon.href;
  let lastN = -1;
  function badge(n) {
    if (n === lastN) return;
    lastN = n;
    doc.title = n > 0 ? `(${n}) ${baseTitle}` : baseTitle;
    if (!icon) return;
    if (n <= 0) { icon.href = baseIcon; return; }
    const t = n > 9 ? "9+" : String(n);
    const mark = doc.querySelector("svg.logo").cloneNode(true);
    mark.removeAttribute("class");
    mark.innerHTML += `<circle cx="18" cy="6" r="7" fill="#ef4444"/><text x="18" y="9" font-family="Arial,sans-serif" font-size="${t.length > 1 ? 7 : 9}" font-weight="700" text-anchor="middle" fill="#fff">${t}</text>`;
    icon.href = "data:image/svg+xml," + encodeURIComponent(new XMLSerializer().serializeToString(mark));
  }
  function alarm() {
    const tr = $("tree"), al = $("nav-al");
    if (!tr) return;
    // Firing alerts usually cover the unhealthy containers (the default rule
    // watches them), so the larger of the two, not the sum.
    badge(Math.max(parseInt(tr.dataset.p, 10) || 0, al && !al.hidden ? parseInt(al.textContent, 10) || 0 : 0));
  }
  alarm();
  // While the tab is hidden the live stream is closed (see below); a pinned
  // tab still updates its badge from a tiny JSON once a minute.
  let alarmPoll = null;
  async function pollAlarm() {
    try { const r = await fetch("/alarm.json", { credentials: "same-origin" }); if (r.ok) badge((await r.json()).problems || 0); } catch {}
  }

  // ---- freshness: "updated N s ago", amber when stale (> 3 refreshes) ----
  let lastLive = Date.now();
  const upd = $("upd"), refresh = Math.max(5, Number(body.dataset.refresh) || 5);
  function freshness() {
    const age = (Date.now() - lastLive) / 1000, stale = age > 3 * refresh;
    if (upd) { upd.textContent = (stale ? "stale · " : "") + "updated " + (age < 2 ? "just now" : dur(age) + " ago"); upd.classList.toggle("stale", stale); }
  }
  setInterval(freshness, 1000);
  freshness();

  // ---- since you last looked (overview): counts from the event log ----
  const since = $("since"), SEEN = "rh-seen";
  async function sinceLast() {
    const last = store.get(SEEN, 0), now = nowS();
    if (!since || !last || now - last < 300) return;
    let ev = [];
    try { ev = (await (await fetch("/api/events?limit=200", { credentials: "same-origin" })).json()).events || []; } catch { return; }
    ev = ev.filter((x) => x.ts > last);
    const n = (f) => ev.filter(f).length;
    const crash = n((x) => (x.kind === "container.die" && x.level !== "info") || x.kind === "container.oom");
    const starts = n((x) => x.kind === "container.start"), alerts = n((x) => x.kind.startsWith("alert.fired"));
    const parts = [];
    if (crash) parts.push([crash, crash === 1 ? "crash" : "crashes"]);
    if (starts) parts.push([starts, starts === 1 ? "start" : "starts"]);
    if (alerts) parts.push([alerts, alerts === 1 ? "alert" : "alerts"]);
    since.replaceChildren(`Since you last looked (${dur(now - last)} ago): `);
    if (!parts.length) since.append("nothing happened.");
    parts.forEach((p, i) => { if (i) since.append(" · "); since.append(el("b", "", String(p[0])), " " + p[1]); });
    since.hidden = false;
  }
  if (since) {
    sinceLast();
    const seen = () => store.set(SEEN, Math.floor(nowS()));
    window.addEventListener("pagehide", seen);
    doc.addEventListener("visibilitychange", () => { if (doc.hidden) seen(); });
    if (!store.get(SEEN, 0)) seen();
  }
  // Updates arriving while the user touches or scrolls are held (newest per
  // section wins) and applied 1.5 s after the interaction stops.
  function queue(html) {
    const t = doc.createElement("template");
    t.innerHTML = html;
    for (const c of Array.from(t.content.children)) if (c.id) held.set(c.id, c);
    flush();
  }
  function flush() {
    clearTimeout(flushT);
    const wait = busyUntil - Date.now();
    if (wait > 0) { flushT = setTimeout(flush, wait + 50); return; }
    if (!held.size) return;
    const nodes = Array.from(held.values());
    held.clear();
    swapNodes(nodes);
  }
  function swap(html) { queue(html); }
  // One stream per visible page. It is closed when the page is hidden for a
  // while or put in the back/forward cache: open EventSources otherwise pin
  // HTTP/1.1 connections (6 per host), which stalled later navigations, and
  // a background tab would keep the server's refresh loop running.
  let es = null, hiddenAt = 0, hideTimer = null;
  // A stream the server refused (a proxy's 502 while Hobnail restarts, a
  // 401) is closed for good by the browser, which retries only dropped
  // connections: so it is opened again here, after 5 s, then twice as long
  // each time up to a minute, and never while the tab is hidden (showing it
  // again connects at once). A stream that opens resets the wait.
  let retryT = null, retryMs = 5000;
  // Every logs page: a viewer (#lv) or the /logs list (#lpick).
  const logsPage = !!($("lv") || $("lpick"));
  function connect() {
    if (!window.EventSource) { setLive(false, "no live updates"); return; }
    if (es) return;
    clearTimeout(retryT); retryT = null;
    const s = (es = new EventSource("/api/stream"));
    es.addEventListener("hello", (e) => {
      if (e.data && e.data !== body.dataset.sha) { location.reload(); return; }
      setLive(true, "live");
    });
    es.addEventListener("frag", (e) => {
      const i = e.data.indexOf("\n");
      lastLive = Date.now();
      if (i > 0) queue(e.data.slice(i + 1));
    });
    // A config change reloads the page, but never a log viewer: it would lose
    // its search, filters, pause, range and scroll (owner decision Y12, LG-03).
    es.addEventListener("reload", () => { if (!logsPage) location.reload(); });
    es.onerror = () => {
      if (es !== s) return;
      setLive(false, "reconnecting");
      if (s.readyState !== 2) return; // the browser retries this one itself
      s.close(); es = null;
      clearTimeout(retryT);
      retryT = setTimeout(() => { retryT = null; if (!doc.hidden) connect(); }, retryMs);
      retryMs = Math.min(60000, retryMs * 2);
    };
    es.onopen = () => { retryMs = 5000; setLive(true, "live"); };
  }
  function disconnect(text) { clearTimeout(retryT); retryT = null; if (es) { es.close(); es = null; } setLive(false, text); }
  function resume() {
    clearTimeout(hideTimer);
    // Paused for over a minute: reload for fresh data (widgets refresh slowly),
    // but never over an open dialog or unsaved input (owner decision Y12).
    if (!es && hiddenAt && Date.now() - hiddenAt > 60000 && !logsPage && !(window.HB && HB.unsaved && HB.unsaved())) { location.reload(); return; }
    hiddenAt = 0;
    connect();
  }
  doc.addEventListener("visibilitychange", () => {
    if (doc.hidden) hideTimer = setTimeout(() => { hiddenAt = Date.now(); disconnect("paused"); alarmPoll = setInterval(pollAlarm, 60000); }, 10000);
    else { clearInterval(alarmPoll); resume(); }
  });
  window.addEventListener("pagehide", () => { clearTimeout(hideTimer); hiddenAt = Date.now(); disconnect("paused"); });
  window.addEventListener("pageshow", (e) => { if (e.persisted) resume(); });

  // ---- navigation feedback (ROB-12): a full page load shows a 2 px line
  // after 150 ms (the CSS delay, so a fast load shows nothing), and a page
  // tab or range that was clicked is marked at once. Runs after every other
  // click handler: a click one of them took over is not a navigation.
  const navbar = el("div", "navbar"); navbar.setAttribute("aria-hidden", "true"); body.append(navbar);
  doc.addEventListener("click", (e) => {
    const a = e.target.closest && e.target.closest("a[href]");
    if (!a || e.defaultPrevented || e.button || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || a.target || a.hasAttribute("download")) return;
    const u = new URL(a.href, location.href);
    if (u.origin !== location.origin || u.pathname.startsWith("/api/") || (u.pathname === location.pathname && u.search === location.search)) return;
    navbar.classList.add("on"); clearTimeout(navT); navT = setTimeout(navOff, 10000);
    if (a.matches(".tabs .tab, .cd-tabs a, .seg a")) a.setAttribute("data-pending", "");
  });
  // A navigation that does not happen (the "leave site?" prompt dismissed,
  // a slow server given up on) must not leave the line on: the next press
  // in this page, coming back from the cache, or 10 s end it.
  let navT = null;
  const navOff = () => { clearTimeout(navT); navT = null; navbar.classList.remove("on"); $$("[data-pending]").forEach((x) => x.removeAttribute("data-pending")); };
  ["pointerdown", "keydown"].forEach((t) => doc.addEventListener(t, () => { if (navbar.classList.contains("on")) navOff(); }, true));
  window.addEventListener("pageshow", navOff);

  sparks(doc);
  times(doc);
  connect();
})();
