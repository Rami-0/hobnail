// Settings pages only (loaded by /settings/*). Lists that moved onto the
// dialog (Bookmarks, Widgets, Notifications) are rendered by the server and
// swapped in from its `?frag=list` after a save; the other tabs load from
// /api/settings/* and render with createElement/textContent (never
// innerHTML with API data; the exceptions are the widget preview and the
// list fragments, both HTML escaped by our own server). Every save is HB.api: X-RH-CSRF, same-origin
// credentials and the base_hash of what was loaded. Runs once window.HB
// exists: ui.js comes with app.js, which is deferred after this script.
(() => {
  "use strict";
  // ---- never auto-reload a signed-in settings page: input would be lost ----
  // app.js reloads the page on the SSE "reload" event (any config change).
  // It opens the stream when it runs, which is after this deferred script and
  // before `start`, so the wrapper is installed here; `start` points the event
  // at checkChanged (the file hash) once it knows the page is signed in.
  let onReload = () => location.reload();
  const ES = window.EventSource;
  if (ES) {
    const Wrapped = function (url, opts) {
      const es = new ES(url, opts);
      const add = es.addEventListener.bind(es);
      es.addEventListener = (type, fn, o) => add(type, type === "reload" ? () => onReload() : fn, o);
      return es;
    };
    Wrapped.prototype = ES.prototype;
    window.EventSource = Wrapped;
  }
  const start = () => {
  const doc = document, SVG = "http://www.w3.org/2000/svg";
  const $ = (id) => doc.getElementById(id);
  const $$ = (s, el) => Array.from((el || doc).querySelectorAll(s));

  // ---- copy buttons (also on the signed-out lock panel) ----
  doc.addEventListener("click", async (e) => {
    const b = e.target.closest && e.target.closest("[data-copy]");
    if (!b) return;
    const src = $(b.dataset.copy);
    if (!src) return;
    let ok = false;
    try { await navigator.clipboard.writeText(src.textContent); ok = true; } catch {
      const r = doc.createRange(); r.selectNodeContents(src);
      const s = getSelection(); s.removeAllRanges(); s.addRange(r);
      try { ok = doc.execCommand("copy"); } catch {}
    }
    const t = b.lastChild;
    t.textContent = ok ? "Copied" : "Press Ctrl C";
    setTimeout(() => { t.textContent = "Copy"; }, 2000);
  });

  // Phones: the tab strip scrolls; show the current tab.
  const cur = doc.querySelector(".set-nav a[aria-current]");
  if (cur && cur.parentElement.parentElement.scrollWidth > cur.closest("nav").clientWidth) cur.scrollIntoView({ inline: "center", block: "nearest" });

  const main = $("set-main"), meta = doc.querySelector('meta[name="rh-csrf"]');
  if (!main || main.dataset.signed !== "1" || !meta) return;
  // /static/app.js (window.HB) did not load: stay the no-JavaScript page.
  // Its "Saving needs JavaScript" notes are still shown (no html.hb-ready).
  if (!window.HB) { onReload = () => {}; return; }
  const TAB = main.dataset.tab;
  const S = { hash: null, saving: 0, recheck: false };

  // A config change checks the file hash instead of reloading (see the top).
  onReload = () => checkChanged();

  // ---- DOM helpers ----
  function el(tag, attrs, ...kids) {
    const e = doc.createElement(tag);
    for (const k in attrs || {}) {
      const v = attrs[k];
      if (v == null || v === false) continue;
      if (k === "class") e.className = v;
      else if (k === "text") e.textContent = v;
      else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
      else e.setAttribute(k, v === true ? "" : v);
    }
    for (const c of kids.flat(Infinity)) if (c != null && c !== false) e.append(c.nodeType ? c : String(c));
    return e;
  }
  function icon(name) {
    const s = doc.createElementNS(SVG, "svg");
    s.setAttribute("class", "i"); s.setAttribute("aria-hidden", "true");
    const u = doc.createElementNS(SVG, "use"); u.setAttribute("href", "#i-" + name);
    s.append(u);
    return s;
  }
  function ibtn(name, label, onclick, cls) {
    return el("button", { type: "button", class: "icon-btn sm " + (cls || ""), "aria-label": label, title: label, onclick }, icon(name));
  }
  function btn(label, onclick, cls, ic) {
    return el("button", { type: "button", class: "btn sm " + (cls || ""), onclick }, ic ? icon(ic) : null, label);
  }
  const pill = (kind, text) => el("span", { class: "pill " + (kind || ""), text });
  const code = (text, cls) => el("code", { class: cls || "", text });
  const muted = (text) => el("span", { class: "muted", text });
  function bytes(n) {
    if (n == null) return "—";
    const u = ["B", "KiB", "MiB", "GiB", "TiB"];
    let i = 0, v = n;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return (i ? v.toFixed(v < 10 ? 1 : 0) : v) + " " + u[i];
  }
  function fmtTime(ts) {
    return new Date(ts * 1000).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  }
  function ago(ts) {
    const s = Math.max(0, Date.now() / 1000 - ts);
    if (s < 10) return "just now";
    const [n, u] = s < 3600 ? [s / 60, "m"] : s < 86400 ? [s / 3600, "h"] : [s / 86400, "d"];
    return Math.max(1, Math.floor(n)) + u + " ago";
  }
  const when = (ts) => el("span", { class: "when", title: new Date(ts * 1000).toLocaleString() }, el("span", { text: fmtTime(ts) }), el("span", { class: "muted", text: " · " + ago(ts) }));
  // Tables become stacked rows on phones; each cell is labelled from the header.
  function labelCells(tbody) {
    const heads = $$("thead th", tbody.closest("table")).map((th) => th.textContent.trim());
    for (const tr of tbody.rows) Array.from(tr.cells).forEach((td, i) => {
      if (heads[i] && td.colSpan === 1) td.dataset.l = heads[i];
      if (td.textContent.trim() === "—") td.classList.add("nil");
    });
  }
  function emptyRow(tbody, cols, text) {
    tbody.replaceChildren(el("tr", { class: "empty-row" }, el("td", { colspan: cols }, muted(text))));
  }

  // ---- API (HB.api: the CSRF header, a fresh token after a new sign-in) ----
  const call = (method, path, body) => HB.api(method, "/api/settings/" + path, body);
  async function get(path) {
    const r = await call("GET", path);
    if (r.status === 200) return r.data;
    if (r.status === 401) banner("expired");
    else if (r.data && r.data.file_invalid) banner("invalid", r.data.error);
    else banner("error", (r.data && r.data.error) || "HTTP " + r.status);
    return null;
  }

  // ---- banners, toasts, notes, field errors ----
  const BANNERS = {
    stale: ["warn", "alert", "The config file changed on disk (maybe a hand edit).", "Reload to see it — your unsaved changes are shown below.", "reload"],
    cssstale: ["warn", "alert", "custom.css changed on disk.", "Reload to see it — your unsaved changes are shown below.", "reload"],
    changed: ["info", "info", "The config file was changed elsewhere (another tab or a hand edit).", "Reload to see the current version; saving these edits would be refused.", "reload"],
    expired: ["bad", "lock", "Your session expired, sign in again.", "Nothing was saved. Copy any unsaved changes first: signing in leaves this page.", "login"],
    invalid: ["bad", "alert", "The config file on disk has an error.", "", "config"],
    error: ["bad", "alert", "Could not load these settings.", "", null],
  };
  function banner(kind, extra) {
    const box = $("set-banners");
    if (box.querySelector(`[data-b="${kind}"]`)) return;
    if (kind === "stale") $$('[data-b="changed"]', box).forEach((x) => x.remove());
    const [cls, ic, head, text, act] = BANNERS[kind];
    const actEl = act === "reload" ? btn("Reload", () => location.reload(), "primary", "refresh")
      : act === "login" ? el("a", { class: "btn sm primary", href: "/login?next=" + encodeURIComponent(location.pathname + location.search) }, icon("log-in"), "Sign in")
        : act === "config" && TAB !== "config" ? el("a", { class: "btn sm", href: "/settings/config" }, "Open the Config file tab") : null;
    const b = el("div", { class: "banner " + cls, role: "alert", "data-b": kind }, icon(ic),
      el("div", null, el("strong", { text: head }), " ", text, extra ? el("span", null, " ", code(extra, "wrap")) : null, actEl ? el("span", { class: "b-act" }, actEl) : null),
      kind === "changed" || kind === "error" ? ibtn("x", "Dismiss", () => b.remove()) : null);
    box.append(b);
  }
  // The one toast system (HB.toast: capped, dismissible, never over a dialog).
  const toast = (text, kind) => HB.toast(text, { kind: kind || "ok" });
  function noteBox(scope) {
    return scope && (scope.matches(".sf-note,.sec-note") ? scope : scope.querySelector(":scope > .sf-note, :scope > .sec-note"));
  }
  function note(scope, kind, msgs) {
    const n = noteBox(scope);
    if (!n) return;
    const list = Array.isArray(msgs) ? msgs : [msgs];
    n.className = n.classList.contains("sec-note") ? "sec-note " + kind : "sf-note " + kind;
    n.replaceChildren(icon(kind === "lock" ? "lock" : kind === "warn" ? "alert" : kind === "ok" ? "check" : "alert"),
      list.length === 1 ? el("div", { text: list[0] }) : el("ul", null, list.map((m) => el("li", { text: m }))));
    n.setAttribute("role", kind === "warn" ? "status" : "alert");
    n.hidden = false;
  }
  function clearNote(scope) { const n = noteBox(scope); if (n) { n.hidden = true; n.replaceChildren(); } }
  function setMsg(form, text) { const m = form.querySelector(".sf-msg"); if (m) m.textContent = text; }
  function clearErrs(form) {
    $$(".fld-err", form).forEach((e) => { e.hidden = true; e.textContent = ""; });
    $$("[aria-invalid]", form).forEach((c) => c.removeAttribute("aria-invalid"));
    clearNote(form);
    setMsg(form, "");
  }
  function ctl(form, name) {
    if (!form.elements) return null;
    const c = form.elements.namedItem(name);
    return c && !c.tagName && c.length ? c[0] : c;
  }
  function fieldErr(form, name, text) {
    const c = ctl(form, name);
    const fld = c && c.closest(".fld");
    if (!fld || fld.closest("[hidden]")) return false;
    const e = fld.querySelector(".fld-err");
    e.textContent = text; e.hidden = false;
    c.setAttribute("aria-invalid", "true");
    c.focus();
    return true;
  }
  // The section note for an entity editor (it closes on success), else the form's.
  const warnScope = (form) => (form.matches(".editor,.ov-edit") ? form.closest("section") : form);
  function fail(res, scope) {
    const d = res.data || {}, st = res.status;
    const msg = d.error || (st ? `Request failed (HTTP ${st})` : "Network error");
    if (st === 409 && d.stale) {
      banner(scope && scope.id === "f-css" ? "cssstale" : "stale");
      note(scope, "warn", "Not saved: the file changed on disk since this page loaded.");
    } else if (st === 401) {
      banner("expired");
      note(scope, "bad", "Not saved: your session expired.");
    } else if (st === 403 && d.locked) {
      note(scope, "lock", msg);
    } else if (st === 422) {
      const pos = d.line ? ` (line ${d.line}, column ${d.col})` : "";
      if (d.field && fieldErr(scope, d.field, msg + pos)) note(scope, "bad", "Not saved: see the highlighted field.");
      else note(scope, "bad", msg + pos);
    } else note(scope, "bad", msg);
  }
  async function save(form, path, body, done) {
    clearErrs(form);
    if (warnScope(form) !== form) clearNote(warnScope(form));
    const sub = form.querySelector ? form.querySelector("button[type=submit]") : null;
    if (sub) sub.disabled = true;
    setMsg(form, "Saving…");
    S.saving++;
    const res = await call("POST", path, body);
    S.saving--;
    if (sub) sub.disabled = false;
    setMsg(form, "");
    const d = res.data || {};
    const ok = res.status === 200 && d.ok !== false;
    if (ok) {
      if (d.hash && path !== "appearance") S.hash = d.hash;
      form._dirty = false;
      toast(d.unchanged ? "No changes" : d.revision != null ? `Saved — revision #${d.revision}` : "Saved");
      if (done) await done(d);
      if (d.warnings && d.warnings.length) note(warnScope(form), "warn", d.warnings);
      setMsg(form, d.unchanged ? "No changes" : "Saved");
    } else fail(res, form);
    if (S.recheck && !S.saving) { S.recheck = false; checkChanged(); }
    return ok;
  }

  // ---- unsaved edits and outside changes ----
  main.addEventListener("input", (e) => { const f = e.target.closest("form"); if (f) f._dirty = true; });
  main.addEventListener("change", (e) => { const f = e.target.closest("form"); if (f) f._dirty = true; });
  const pageDirty = () => $$("form", main).some((f) => f._dirty && !f.hidden && f.id !== "f-clear" && !f.hasAttribute("data-hb"));
  HB.keep(pageDirty); // app.js does not reload over it either
  const dirty = () => HB.dirty() || pageDirty();
  window.addEventListener("beforeunload", (e) => { if (dirty() && !S.leaving) { e.preventDefault(); e.returnValue = ""; } });
  // The file changed (another tab, a hand edit). With a dialog or a confirm
  // open nothing is refreshed and nothing is announced: the dialog keeps the
  // base hash it opened with and its save gets a 409 note instead (one
  // message for one conflict). The check runs again once the last closes.
  doc.addEventListener("close", (e) => {
    if (S.later && e.target.matches && e.target.matches("dialog.dlg") && !doc.querySelector("dialog.dlg[open]")) { S.later = false; setTimeout(checkChanged, 0); }
  }, true);
  async function checkChanged() {
    if (S.saving) { S.recheck = true; return; }
    if (!S.hash || !TABS[TAB]) return;
    if (doc.querySelector("dialog.dlg[open]")) { S.later = true; return; }
    const r = await call("GET", "meta");
    if (r.status !== 200 || r.data.hash === S.hash) return;
    if (doc.querySelector("dialog.dlg[open]")) { S.later = true; return; }
    if (dirty()) banner("changed");
    else { await TABS[TAB].load(); toast("Reloaded: the config file changed", "info"); }
  }

  // ---- generic form values (data-t on the controls, see ui/settings/mod.rs) ----
  function parseJ(v) {
    if (v === "true") return true;
    if (v === "false") return false;
    if (/^-?\d+(\.\d+)?$/.test(v)) return Number(v);
    return v;
  }
  function readCtl(c) {
    const v = c.value.trim();
    switch (c.dataset.t) {
      case "bool": return c.checked;
      case "tri": return v === "" ? null : v === "true";
      case "num": return v === "" || isNaN(Number(v)) ? null : Number(v);
      case "list": { const a = v.split(",").map((s) => s.trim()).filter(Boolean); return a.length ? a : null; }
      case "json": return v === "" ? null : parseJ(v);
      default: return v === "" ? null : v;
    }
  }
  const forType = (c, type) => { const w = c.closest("[data-for]"); return !w || !type || w.dataset.for.split(" ").includes(type); };
  const named = (form) => Array.from(form.elements).filter((c) => c.name && !c.name.includes("-") && c.type !== "radio");
  // {name: value}; fields of other types are null (removed). `defs`: a bool
  // equal to its default is null too (the key is left out of the file).
  function collect(form, type, defs) {
    const out = {}, sets = new Set();
    for (const c of named(form)) {
      if (c.dataset.t === "set") {
        sets.add(c.name);
        out[c.name] = out[c.name] || [];
        if (c.checked && forType(c, type)) out[c.name].push(c.value);
        continue;
      }
      let v = forType(c, type) ? readCtl(c) : null;
      if (defs && c.name in defs && v === defs[c.name]) v = null;
      out[c.name] = v;
    }
    for (const k of sets) if (!out[k].length) out[k] = null;
    return out;
  }
  function fill(form, obj) {
    for (const c of named(form)) {
      const v = obj ? obj[c.name] : undefined;
      c.removeAttribute("aria-invalid");
      if (c.dataset.t === "set") c.checked = Array.isArray(v) && v.includes(c.value);
      else if (c.type === "checkbox") c.checked = !!v;
      else if (c.dataset.t === "tri") c.value = v === true ? "true" : v === false ? "false" : "";
      else if (v == null) c.value = "";
      else if (Array.isArray(v)) c.value = v.map(String).join(", ");
      else c.value = typeof v === "object" ? JSON.stringify(v) : String(v);
    }
  }
  const pick = (o, keys) => { const r = {}; for (const k of keys) if (o && o[k] !== undefined) r[k] = o[k]; return r; };
  const dur = (v) => (v != null && /^\d+$/.test(String(v)) ? Number(v) : v);
  // The container override editor (moves onto the dialog with its tab).
  function openEditor(form, title, after) {
    clearErrs(form);
    form._dirty = false;
    const h = form.querySelector(".ed-h");
    if (h) h.textContent = title;
    if (after) after.after(form);
    form.hidden = false;
    form.scrollIntoView({ block: "nearest", behavior: "smooth" });
    const first = Array.from(form.elements).find((c) => !c.closest("[hidden]") && !c.readOnly && /^(INPUT|SELECT|TEXTAREA)$/.test(c.tagName));
    if (first) first.focus({ preventScroll: true });
  }
  function closeEditor(form) { form.hidden = true; form._dirty = false; form._orig = null; }
  main.addEventListener("click", (e) => {
    const b = e.target.closest("[data-act=ed-cancel]");
    if (b) closeEditor(b.closest("form"));
  });
  const onSubmit = (form, fn) => form.addEventListener("submit", (e) => { e.preventDefault(); fn(); });
  const onAct = (act, fn) => $$(`[data-act="${act}"]`, main).forEach((b) => b.addEventListener("click", fn));
  function moveBtns(list, i, reorder) {
    const up = ibtn("arrow-up", "Move up", () => reorder(i, i - 1));
    const dn = ibtn("arrow-down", "Move down", () => reorder(i, i + 1));
    up.disabled = i === 0;
    dn.disabled = i === list.length - 1;
    return [up, dn];
  }
  const swap = (a, i, j) => { const r = a.slice(); [r[i], r[j]] = [r[j], r[i]]; return r; };

  // =================================================================== tabs
  const TABS = {};

  // ---- general ----
  const PAGE_KEYS = ["title", "greeting", "columns", "layout", "theme", "search_url", "show_stopped", "show_stats"];
  TABS.general = {
    init() {
      const f = $("f-general");
      onSubmit(f, () => {
        const patch = {};
        for (const k of PAGE_KEYS) {
          const c = ctl(f, k);
          const nv = readCtl(c);
          if (c.type === "checkbox") { if (nv !== f._eff[k]) patch[k] = nv; } else if (nv !== (f._page[k] ?? null)) patch[k] = nv;
        }
        if (!Object.keys(patch).length) { setMsg(f, "No changes"); toast("No changes"); return; }
        save(f, "general", { base_hash: S.hash, page: patch }, () => this.load());
      });
    },
    async load() {
      const d = await get("general");
      if (!d) return;
      S.hash = d.hash;
      const f = $("f-general"), page = d.page || {};
      f._page = page; f._eff = d.effective; f._dirty = false;
      for (const k of PAGE_KEYS) {
        const c = ctl(f, k), def = d.defaults[k];
        if (c.type === "checkbox") c.checked = !!d.effective[k];
        else if (c.tagName === "SELECT") { c.value = page[k] ?? ""; c.options[0].textContent = `Default (${def})`; } else {
          c.value = page[k] ?? "";
          c.placeholder = k === "greeting" ? "Good morning / afternoon / evening" : String(d.effective[k] ?? def ?? "");
        }
      }
    },
  };

  // ---- containers and groups ----
  const GROUP_KEYS = ["name", "title", "columns", "layout", "collapsed", "icon", "hide"];
  const OVERRIDE_KEYS = ["name", "group", "icon", "url", "description", "hide", "order", "size"];
  TABS.containers = {
    sel: new Set(),
    init() {
      const fx = $("f-exclude"), fg = $("f-groups"), fo = $("f-override");
      const addEx = () => {
        const i = $("ex-new"), v = i.value.trim();
        if (!v) return;
        if (!this.ex.includes(v)) this.ex.push(v);
        i.value = ""; fx._dirty = true; this.renderEx();
      };
      onAct("ex-add", addEx);
      $("ex-new").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); addEx(); } });
      onSubmit(fx, () => save(fx, "docker", { base_hash: S.hash, exclude: this.ex }, () => this.load("ex")));
      const addGrp = () => {
        const i = $("grp-new"), v = i.value.trim();
        if (!v) return;
        if (this.groups.some((g) => g.name === v)) { toast(`Group ${v} is already listed`, "info"); return; }
        this.groups.push({ name: v }); i.value = ""; fg._dirty = true; this.renderGroups();
      };
      onAct("grp-add", addGrp);
      $("grp-new").addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); addGrp(); } });
      onSubmit(fg, () => {
        const groups = this.groups.map((g) => { const o = pick(g, GROUP_KEYS); for (const k in o) if (o[k] === "" || o[k] == null || o[k] === false) delete o[k]; return o; });
        save(fg, "groups", { base_hash: S.hash, groups }, () => this.load("groups"));
      });
      $("ov-q").addEventListener("input", () => this.renderTable());
      $("ov-only").addEventListener("change", () => this.renderTable());
      $("ov-all").addEventListener("change", (e) => {
        for (const r of this.shown) e.target.checked ? this.sel.add(r.name) : this.sel.delete(r.name);
        this.renderTable();
      });
      onSubmit(fo, () => this.saveOverride());
      onAct("ov-cancel", () => this.closeOv());
      onAct("ov-remove", () => this.bulk({ remove: true }, [fo._name], fo));
      const sec = $("ov-sec");
      onAct("bulk-group", () => {
        const g = $("ov-bulk-group").value.trim();
        if (!g) { $("ov-bulk-group").focus(); toast("Type a group name first", "info"); return; }
        this.bulk({ set: { group: g } });
      });
      onAct("bulk-hide", () => this.bulk({ set: { hide: true } }));
      onAct("bulk-show", () => this.bulk({ clear: ["hide"] }));
      onAct("bulk-clear", () => { if (confirm(`Remove all overrides of ${this.sel.size} container(s)?`)) this.bulk({ remove: true }); });
      onAct("bulk-none", () => { this.sel.clear(); this.renderTable(); });
      this.focus = sec.dataset.focus || "";
    },
    async load(part) {
      const d = await get("containers");
      if (!d) return;
      S.hash = d.hash;
      this.known = d.known || [];
      this.ov = d.overrides || {};
      this.standalone = d.docker.standalone_group || "Standalone";
      if (!part || part === "ex") { this.ex = (d.docker.exclude || []).slice(); $("f-exclude")._dirty = false; this.renderEx(); }
      if (!part || part === "groups") { this.groups = (d.groups || []).map((g) => ({ ...g })); $("f-groups")._dirty = false; this.renderGroups(); }
      this.closeOv();
      this.renderTable();
      if (this.focus) {
        const name = this.focus; this.focus = "";
        const tr = $$("#ov-body tr[data-name]").find((t) => t.dataset.name === name);
        if (tr) this.openOv(name, tr);
      }
    },
    renderEx() {
      const ul = $("ex-list");
      if (!this.ex.length) { ul.replaceChildren(el("li", { class: "muted none" }, "No patterns: every container is shown.")); return; }
      ul.replaceChildren(...this.ex.map((p, i) => el("li", { class: "chip-e" }, code(p),
        ibtn("x", `Remove pattern ${p}`, () => { this.ex.splice(i, 1); $("f-exclude")._dirty = true; this.renderEx(); }))));
    },
    renderGroups() {
      const box = $("grp-list"), fg = $("f-groups");
      if (!this.groups.length) { box.replaceChildren(el("p", { class: "muted none" }, "No groups listed: groups appear in their default order (compose projects, then standalone, then bookmarks).")); return; }
      const head = el("div", { class: "row grp-row row-h", "aria-hidden": "true" }, el("span", { text: "Group" }), el("span", { text: "Title" }), el("span", { text: "Icon" }), el("span", { text: "Collapsed" }), el("span", { text: "Hidden" }), el("span"));
      const rows = this.groups.map((g, i) => {
        const id = "grp-" + i;
        const inp = (k, ph, label) => el("input", { class: "in" + (k === "icon" ? " mono" : ""), type: "text", value: g[k] || "", placeholder: ph, "aria-label": `${label} for group ${g.name}`, oninput: (e) => { g[k] = e.target.value.trim(); fg._dirty = true; } });
        const cb = (k, label) => el("label", { class: "cb" }, el("input", { type: "checkbox", checked: !!g[k], onchange: (e) => { g[k] = e.target.checked; fg._dirty = true; } }), el("span", { text: label }));
        const reorder = (a, b) => { if (b < 0 || b >= this.groups.length) return; this.groups = swap(this.groups, a, b); fg._dirty = true; this.renderGroups(); $$(".grp-row:not(.row-h)", box)[b].querySelector("input").focus(); };
        return el("div", { class: "row grp-row", id },
          el("span", { class: "grp-name" }, code(g.name, "nm")),
          inp("title", g.name, "Title"), inp("icon", "icon", "Icon"),
          cb("collapsed", "Collapsed"), cb("hide", "Hidden"),
          el("span", { class: "row-acts" }, moveBtns(this.groups, i, reorder),
            ibtn("x", `Remove group ${g.name} from the list`, () => { this.groups.splice(i, 1); fg._dirty = true; this.renderGroups(); }, "danger")));
      });
      box.replaceChildren(head, ...rows);
    },
    rows() {
      const names = new Set(this.known.map((k) => k.name));
      Object.keys(this.ov).forEach((n) => names.add(n));
      const byName = Object.fromEntries(this.known.map((k) => [k.name, k]));
      return Array.from(names).sort().map((name) => ({ name, k: byName[name] || null, o: this.ov[name] || null }));
    },
    renderTable() {
      const q = $("ov-q").value.trim().toLowerCase(), only = $("ov-only").checked;
      const all = this.rows();
      this.shown = all.filter((r) => (!only || r.o) && (!q || [r.name, r.k && r.k.project, r.k && r.k.image, r.o && r.o.name, r.o && r.o.group].some((s) => s && String(s).toLowerCase().includes(q))));
      $("ov-count").textContent = this.shown.length === all.length ? String(all.length) : `${this.shown.length} / ${all.length}`;
      const tb = $("ov-body"), fo = $("f-override");
      // Keep an open editor (and its unsaved values) across a re-render.
      const open = !fo.hidden && fo._name;
      $("ov-sec").append(fo);
      if (!this.shown.length) emptyRow(tb, 8, all.length ? "No container matches." : "No containers found.");
      else tb.replaceChildren(...this.shown.map((r) => this.row(r)));
      labelCells(tb);
      const tr = open && $$("tr[data-name]", tb).find((t) => t.dataset.name === open);
      if (tr) { tr.classList.add("editing"); tr.after(el("tr", { class: "ov-edit-row" }, el("td", { colspan: 8 }, fo))); } else if (open) closeEditor(fo);
      for (const n of Array.from(this.sel)) if (!all.some((r) => r.name === n)) this.sel.delete(n);
      $("ov-all").checked = this.shown.length > 0 && this.shown.every((r) => this.sel.has(r.name));
      $("ov-bulk").hidden = !this.sel.size;
      $("ov-bulk-n").textContent = `${this.sel.size} selected`;
    },
    row(r) {
      const o = r.o || {}, k = r.k;
      const state = k ? k.state : "unknown";
      const dot = el("span", { class: "dot " + (state === "running" ? "ok" : state === "unknown" ? "off" : state === "exited" ? "bad" : "warn"), role: "img", "aria-label": state, title: state });
      const flags = [];
      if (o.hide === true) flags.push(pill("warn", "hidden"));
      if (o.hide === false) flags.push(pill("", "shown"));
      if (o.order != null) flags.push(pill("", "order " + o.order));
      if (o.size) flags.push(pill("", o.size));
      if (!k) flags.push(pill("off", "not running"));
      const val = (v, mono, dflt) => (v != null && v !== "" ? (mono ? code(String(v), "wrap") : el("span", { class: "wrap", text: String(v) })) : dflt ? muted(dflt) : muted("—"));
      const cb = el("input", { type: "checkbox", "aria-label": `Select ${r.name}`, checked: this.sel.has(r.name), onchange: (e) => { e.target.checked ? this.sel.add(r.name) : this.sel.delete(r.name); this.renderTable(); } });
      const tr = el("tr", { "data-name": r.name, class: r.o ? "has-ov" : "" },
        el("td", { class: "ck" }, cb),
        el("td", { class: "ctr-c" }, el("span", { class: "ctr-n" }, dot, code(r.name, "nm")), k ? el("span", { class: "sub muted", text: (k.project ? k.project + " · " : "") + k.image }) : null),
        el("td", null, val(o.name)), el("td", null, val(o.group, true, k ? k.project || this.standalone : "")),
        el("td", null, val(o.icon, true)), el("td", { class: "url-c" }, val(o.url, true)),
        el("td", { class: "flags" }, flags.length ? flags : muted("—")),
        el("td", { class: "act" }, btn(r.o ? "Edit" : "Customize", () => this.openOv(r.name, tr), "", "edit")));
      return tr;
    },
    openOv(name, tr) {
      const fo = $("f-override");
      this.closeOv();
      fo._name = name; fo._orig = this.ov[name] || {};
      fill(fo, fo._orig);
      $("ov-edit-name").textContent = name;
      $$("[data-act=ov-remove]", fo).forEach((b) => { b.hidden = !this.ov[name]; });
      const er = el("tr", { class: "ov-edit-row" }, el("td", { colspan: 8 }));
      tr.after(er);
      tr.classList.add("editing");
      er.firstChild.append(fo);
      openEditor(fo, null);
    },
    closeOv() {
      const fo = $("f-override");
      if (!fo) return;
      closeEditor(fo);
      $("ov-sec").append(fo);
      $$(".ov-edit-row").forEach((r) => r.remove());
      $$("#ov-body tr.editing").forEach((r) => r.classList.remove("editing"));
    },
    saveOverride() {
      const fo = $("f-override"), v = collect(fo), o = fo._orig || {};
      const set = {}, clear = [];
      for (const k of OVERRIDE_KEYS) {
        const nv = v[k], ov = o[k] ?? null;
        if (nv === null) { if (ov !== null) clear.push(k); } else if (nv !== ov) set[k] = nv;
      }
      if (!Object.keys(set).length && !clear.length) { setMsg(fo, "No changes"); toast("No changes"); return; }
      const body = { base_hash: S.hash, names: [fo._name], clear };
      if (Object.keys(set).length) body.set = set;
      save(fo, "containers/override", body, () => this.load("ov"));
    },
    bulk(what, names, form) {
      const list = names || Array.from(this.sel);
      if (!list.length) return;
      const body = { base_hash: S.hash, names: list, ...what };
      save(form || $("ov-sec"), "containers/override", body, () => { if (!names) this.sel.clear(); return this.load("ov"); });
    },
  };

  // ---- lists rendered by the server (the dialog pattern, DESIGN.md §11.6) ----
  // `refreshList(ids, flash)` fetches this tab's `?frag=list` (the same markup
  // as the page, session only) and puts in each element named by `ids` (the
  // first one is the list the dialog belongs to; a tab whose lists depend on
  // each other refreshes them together). Rows are swapped by data-key: an
  // unchanged row stays as it is (focus and scroll keep), a changed or new
  // one is put in, a gone one is removed, and the row named by `flash`
  // flashes once. Any other element (a form's choice list) is replaced.
  async function refreshList(ids, flash) {
    ids = [].concat(ids);
    let r;
    try { r = await fetch(location.pathname + "?frag=list", { credentials: "same-origin", cache: "no-store" }); } catch { return false; }
    if (r.status === 401) banner("expired");
    if (!r.ok) return false;
    const t = doc.createElement("template");
    t.innerHTML = await r.text(); // our server's markup: every value escaped there
    let done = false;
    for (const id of ids) {
      const next = t.content.getElementById(id), box = $(id);
      if (!next || !box) continue;
      $$("[data-ready]", next).forEach((b) => { b.disabled = false; }); // as ui.js does on load
      if (!next.dataset.hash) { box.replaceWith(next); continue; }
      done = true;
      S.hash = next.dataset.hash;
      box.dataset.hash = next.dataset.hash;
      Object.assign(box.dataset, next.dataset); // the hash, a widget list's order and System tiles
      const a = box.querySelector("tbody"), b = next.querySelector("tbody");
      if (!a || !b) box.replaceChildren(...next.childNodes);
      else {
        const old = new Map($$("tr[data-key]", a).map((x) => [x.dataset.key, x]));
        const same = (o, x) => { const c = o.cloneNode(true); c.classList.remove("row-new"); return c.isEqualNode(x); };
        const want = Array.from(b.rows).map((x) => { const o = old.get(x.dataset.key); return o && same(o, x) ? o : x; });
        want.forEach((w, i) => { if (a.rows[i] !== w) a.insertBefore(w, a.rows[i] || null); });
        while (a.rows.length > want.length) a.rows[want.length].remove();
      }
      const row = flash && box.querySelector(`tr[data-key="${CSS.escape(flash)}"]`);
      if (row) HB.flash(row);
    }
    return done;
  }
  // THE BASE-HASH RULE (every list form, the next ones included): a write is
  // based on the file version the user was looking at when they started it.
  // A dialog takes its base when it opens (ui.js: the `data-hash` of the
  // list its trigger sits in), a confirm, an arrow click or a row switch
  // takes it before anything is awaited, and that base is sent as base_hash.
  // Nothing adopts a newer hash on its behalf, so the server answers 409
  // whenever the file moved, and an id that is a list position can never
  // hit another entry. The one hash a dialog may move to without asking is
  // the one its own save wrote ("Save and send test" stays open on what it
  // saved: HB.edited). S.hash only says which version the list on screen
  // shows.
  //
  // A write of a list: counted in S.saving like save() does, so a config
  // change event that arrives meanwhile waits for the answer and then finds
  // the new hash instead of reloading the list.
  async function post(path, body) {
    S.saving++;
    let r;
    try { r = await call("POST", path, body); } finally { S.saving--; }
    if (r.status === 200 && r.data && r.data.hash) S.hash = r.data.hash;
    if (S.recheck && !S.saving) { S.recheck = false; setTimeout(checkChanged, 0); }
    return r;
  }
  const itemOfEl = (x) => { try { return JSON.parse((x && x.dataset && x.dataset.item) || "{}"); } catch { return {}; } };
  const rowItem = (tr) => itemOfEl(tr.querySelector("[data-item]"));
  const rowOf = (o, key) => $(o.list).querySelector(`tr[data-key="${CSS.escape(key)}"]`);
  const keyId = (key) => key.slice(key.indexOf(":") + 1);
  const revNo = (d) => (d.revision != null ? ` · revision #${d.revision}` : "");
  const quoted = (names) => names.map((n) => `“${n}”`).join(", ");
  const lists = (o) => [o.list].concat(o.also || []);
  // The values a dialog was opened with (or, after "Save and send test",
  // what it saved): what an edit is re-found by after a 409.
  const openedWith = (c) => c.was || itemOfEl(c.trigger);

  // A list's add / edit dialog. `o`: {list, also, path, kind, noun,
  // item(values, was) → the API item, keep(item, was, base) → the item with
  // the keys the form does not show (async), id(key) → the API id,
  // savedKey(values, editKey, rowsBefore), movedKey(key, newIndex),
  // label(values), ask(values, c) → a note that stops the first Save
  // (a rename that changes rules), test(values, key, data) → the note of a
  // "save and …" action}. Saves send the dialog's own base (c.base), never
  // S.hash.
  function listDialog(dlg, o) {
    if (!dlg) return;
    dlg._submit = async (v, ctx) => {
      const c = HB.current();
      const edit = ctx.mode === "edit" && ctx.key;
      const was = openedWith(c);
      const stop = edit && o.ask && o.ask(v, c, was);
      if (stop) return stop;
      const before = $$("tr[data-key]", $(o.list)).length;
      // An edit sends only what changed (THE DIFF RULE, above `onlyChanged`).
      const item = o.item(v, edit ? was : null);
      const body = edit ? { base_hash: c.base, action: "edit", id: o.id(c.key), item } : { base_hash: c.base, action: "add", item };
      const r = HB.result(await post(o.path, body), {
        // 409, "Reload and keep my input": show the file as it is now and
        // keep every value typed. RETARGET RULE: an add simply moves to the
        // new version. An edit moves only when the entry it was opened on
        // can be found again unambiguously: exactly one entry of the new file
        // has all the values the dialog was opened with. Then key and base
        // follow it. Otherwise Save (and Delete…) stay disabled and only
        // Discard is offered: the input can be copied from the fields, but it
        // can never be saved onto another entry.
        reload: async () => {
          await refreshList(lists(o));
          const cc = HB.current(), box = $(o.list);
          if (!cc || cc.dlg !== dlg) return;
          if (!edit) { cc.base = box.dataset.hash; HB.note("ok", "Reloaded the current file. Save again to add it."); ctx.form.querySelector("[type=submit]").focus(); return; }
          const same = $$("tr[data-key]", box).filter((tr) => JSON.stringify(rowItem(tr)) === JSON.stringify(was));
          if (same.length === 1) {
            cc.key = same[0].dataset.key; cc.base = box.dataset.hash;
            HB.note("ok", "Reloaded the current file. Save again to apply your input on top of it.");
            ctx.form.querySelector("[type=submit]").focus();
          } else {
            $$("[type=submit],[data-submit],[data-act=dlg-delete]", dlg).forEach((b) => { b.disabled = true; });
            HB.note("bad", `This ${o.noun} was changed, moved or removed in the file meanwhile, so your input cannot be saved onto it. Copy what you need, discard, and open it again from the list.`, [{ label: "Discard my input", run: () => HB.close(true) }]);
          }
        },
        discard: () => { HB.close(true); refreshList(lists(o)); },
      });
      if (!r.ok) return o.fields ? o.fields(r) : r;
      const key = o.savedKey(v, edit && c.key, before), d = r.data;
      await refreshList(lists(o), d.unchanged ? null : key);
      $$('#set-banners [data-b="changed"], #set-banners [data-b="stale"]').forEach((b) => b.remove());
      const warn = (d.warnings || []).join(" ");
      if (ctx.action !== "save" && o.test) {
        // Saved; the dialog stays open on what it saved (see the rule above).
        // The row is what was saved only if the list shows the file this
        // save wrote; if someone wrote after it, the dialog keeps nothing to
        // re-find (a later 409 then cannot be retargeted: fail closed).
        const row = rowOf(o, key), mine = row && $(o.list).dataset.hash === d.hash;
        HB.edited(c, key, d.hash, mine ? rowItem(row) : {});
        const saved = d.unchanged ? "No changes to save." : `Saved${revNo(d).replace(" · ", " (") + (d.revision != null ? ")" : "")}.`;
        const t = await o.test(v, key, d);
        return { ok: true, stay: true, note: [t[0], `${saved} ${t[1]}${warn ? " " + warn : ""}`] };
      }
      return {
        ok: true,
        toast: d.unchanged ? "No changes" : `${edit ? "Saved" : "Added"} “${o.label(v)}”${revNo(d)}`,
        focus: `[data-dlg][data-key="${CSS.escape(key)}"]`,
        after: warn ? () => setTimeout(() => toast(warn, "info"), 0) : null,
      };
    };
  }
  // Delete with the confirm, then Undo. `key`, `base` and `name` are taken
  // by the caller before the confirm is awaited (the base-hash rule): if the
  // file moved while the confirm was open, the delete is refused (409), never
  // applied to whatever sits at that position now. The toast's Undo restores
  // the revision the delete was made on, refused (409) if the file changed
  // since. An entry that rules still use is not offered for deleting: the
  // message names the rules (the server refuses it the same way).
  const confirmDelete = (o, name) => HB.confirm({ title: `Delete the ${o.noun} “${name}”?`, body: "It is removed from the config file. You can undo this from the message that follows, or restore it later under Config file.", ok: `Delete ${o.noun}` });
  // `data-rules` of a row: the names of the rules that use it, as JSON (a
  // rule name may hold a comma).
  const rulesOf = (tr) => { try { return JSON.parse((tr && tr.dataset.rules) || "[]"); } catch { return []; } };
  function usedBy(o, tr, name) {
    const rules = rulesOf(tr);
    if (!rules.length) return false;
    toast(`The ${o.noun} “${name}” is used by the rule${rules.length > 1 ? "s" : ""} ${quoted(rules)}. ${o.inUse}`, "bad");
    return true;
  }
  async function deleteRow(o, key, base, name) {
    const before = base;
    const res = await post(o.path, { base_hash: base, action: "delete", id: o.id(key) });
    const r = HB.result(res);
    if (!r.ok) {
      const d = res.data || {};
      toast(d.used_by ? d.error : r.note && /changed on disk/.test(r.note[1]) ? `Not deleted: the config file changed since you asked. Nothing was removed; check the list and try again.` : (r.note && r.note[1]) || Object.values(r.fields || {}).join(" "), "bad");
      refreshList(lists(o));
      return;
    }
    const row = rowOf(o, key);
    if (row) { HB.remove(row); await new Promise((res) => setTimeout(res, 170)); }
    await refreshList(lists(o));
    const d = r.data, after = d.hash, rev = d.revision;
    const undo = async () => {
      // The revision just before the delete holds the file as it was; its
      // hash proves it before anything is written. (The revision list has no
      // hashes, so this fetches that one revision's text.)
      const prev = rev != null ? await call("GET", `revision/${rev - 1}`) : null;
      if (!prev || prev.status !== 200 || prev.data.hash !== before) { toast(`Cannot undo: the file as it was before the delete is not in the revisions. Look under Config file.`, "bad"); return; }
      const u = await post(`revision/${rev - 1}/restore`, { base_hash: after });
      if (u.status === 409) { toast("Not undone: the config file changed since the delete. Its revisions are under Config file.", "bad"); return; }
      const ur = HB.result(u);
      if (!ur.ok) { toast(ur.note ? ur.note[1] : "Not undone.", "bad"); return; }
      await refreshList(lists(o), key);
      toast(`Restored “${name}”${revNo(ur.data)}`);
    };
    HB.toast(`Deleted “${name}”${revNo(d)}`, rev != null ? { action: { label: "Undo", run: undo } } : {});
    const add = $(o.add);
    if (add && (doc.activeElement === doc.body || !doc.activeElement)) add.focus();
  }
  // One-click up / down (owner decision Y13: one revision per step). A list
  // that shows only some entries of its array (widgets: not the command
  // ones) carries every id in file order as `data-order`.
  function moveRow(o, btn) {
    const box = $(o.list), tr = btn.closest("tr[data-key]"), rows = $$("tr[data-key]", box);
    const i = rows.indexOf(tr), j = i + (btn.dataset.move === "up" ? -1 : 1);
    if (i < 0 || j < 0 || j >= rows.length || o.moving) return;
    let order = rows.map((r) => o.id(r.dataset.key));
    if (box.dataset.order) {
      try { order = JSON.parse(box.dataset.order); } catch { return; }
      const a = order.indexOf(o.id(rows[i].dataset.key)), b = order.indexOf(o.id(rows[j].dataset.key));
      if (a < 0 || b < 0) return;
      [order[a], order[b]] = [order[b], order[a]];
    } else [order[i], order[j]] = [order[j], order[i]];
    o.moving = true;
    btn.setAttribute("aria-busy", "true");
    const dir = btn.dataset.move, name = o.label(rowItem(tr)), base = box.dataset.hash;
    (async () => {
      const r = HB.result(await post(o.path, { base_hash: base, action: "reorder", order }));
      btn.removeAttribute("aria-busy");
      o.moving = false;
      if (!r.ok) {
        toast(r.note && /changed on disk/.test(r.note[1]) ? "Not moved: the config file changed since this list was loaded. Nothing was changed; the list now shows the current file." : (r.note && r.note[1]) || "Not moved.", "bad");
        refreshList(lists(o));
        return;
      }
      const key = o.movedKey(tr.dataset.key, j);
      await refreshList(lists(o), key);
      const nr = rowOf(o, key), same = nr && nr.querySelector(`[data-move="${dir}"]`);
      const f = same && !same.disabled ? same : nr && nr.querySelector("[data-move]:not([disabled])");
      if (f) f.focus();
      toast(`Moved “${name}” ${dir}${revNo(r.data)}`);
    })();
  }
  // Row actions of a server-rendered list: ↑ / ↓, the ⋯ menu (Edit, Delete…)
  // and Delete… inside the dialog.
  function listActions(o) {
    const box = $(o.list);
    if (!box) return;
    S.hash = box.dataset.hash;
    box.addEventListener("click", (e) => { const b = e.target.closest("[data-move]"); if (b && !b.disabled) moveRow(o, b); });
    doc.addEventListener("click", (e) => {
      const a = e.target.closest("[data-row-act]"), t = HB.menuFor;
      if (a && t && t.dataset.menu === o.menu && box.contains(t)) {
        const tr = t.closest("tr[data-key]");
        if (a.dataset.rowAct === "edit") tr.querySelector("[data-dlg]").click();
        else {
          // Taken now, before the confirm is awaited (the base-hash rule).
          const key = tr.dataset.key, base = box.dataset.hash, name = o.label(rowItem(tr));
          if (!usedBy(o, tr, name)) confirmDelete(o, name).then((ok) => ok && deleteRow(o, key, base, name));
        }
      }
      const del = e.target.closest("[data-act=dlg-delete]"), c = HB.current();
      if (del && c && c.dlg.id === o.dlg && c.key) {
        // The dialog's own key and base, and the name it was opened on.
        const key = c.key, base = c.base, name = o.label(openedWith(c));
        if (usedBy(o, rowOf(o, key), name)) return;
        confirmDelete(o, name).then((ok) => { if (ok) { HB.close(true); deleteRow(o, key, base, name); } });
      }
    });
  }
  // A rename that rules follow (owner decision Y7: names stay editable). The
  // first Save names the rules that change with it and stops; Save again
  // with the same new name goes ahead. `field`: the name field; `how(old, many)`:
  // what the rules do with the old name.
  const renameAsk = (list, field, how) => (v, c, was) => {
    const nv = v[field], ov = was[field];
    if (!ov || !nv || nv === ov || c.ack === nv) return null;
    const rules = rulesOf($(list).querySelector(`tr[data-key="${CSS.escape(c.key)}"]`));
    if (!rules.length) return null;
    c.ack = nv;
    const many = rules.length > 1;
    return { ok: false, note: ["warn", `The rule${many ? "s" : ""} ${quoted(rules)} ${how(ov, many)}. Saving renames it to “${nv}” and changes ${many ? "those rules" : "that rule"} to match. Press Save again to do both.`] };
  };

  // ---- bookmarks: the reference list ----
  const BOOKMARKS = {
    list: "bm-list", add: "bm-add", dlg: "dlg-bookmark", menu: "m-bm", path: "bookmarks", kind: "bookmark", noun: "bookmark",
    // The API sends a whole item; an emptied optional field removes the key.
    item: (v) => { const it = {}; for (const k of ["name", "url", "group", "icon", "description"]) it[k] = v[k] ? v[k] : null; return it; },
    id: (key) => Number(keyId(key)),
    savedKey: (v, editKey, before) => editKey || `bookmark:${before}`, // appended at the end
    movedKey: (key, j) => `bookmark:${j}`, // keys are positions
    label: (v) => v.name || "(no name)",
  };
  TABS.bookmarks = {
    ssr: true,
    init() { listDialog($("dlg-bookmark"), BOOKMARKS); listActions(BOOKMARKS); },
    load() { return refreshList(BOOKMARKS.list); },
  };

  // ---- notifications: channels and alert rules (server-rendered lists) ----
  const listOf = (s) => { const a = String(s || "").split(",").map((x) => x.trim()).filter(Boolean); return a.length ? a : null; };
  // Every key a channel can have (an add sends them all; null: not set).
  const CH_KEYS = ["type", "url", "url_env", "url_file", "webhook_url_env", "webhook_url_file", "token_env", "token_file", "chat_id", "chat_id_env", "api_base", "headers", "headers_env", "body", "timeout"];
  // Kept as they are in the file (not in the form; never put in the page).
  // Not in the form, but they only apply to one type: removed with it.
  const CH_TYPED = ["chat_id_env", "api_base", "headers", "headers_env", "body"];
  const CH_SECRET = { slack: { env: "webhook_url_env", file: "webhook_url_file" }, telegram: { env: "token_env", file: "token_file" }, webhook: { env: "url_env", file: "url_file", plain: "url" } };
  // THE DIFF RULE (every list edit): the server patches an edited entry
  // with the keys the item has (null removes one) and leaves every absent
  // key exactly as it is in the file (settings/ops.rs). So an edit sends
  // only the keys whose field the user changed, plus `always` (what the
  // server needs to know the entry). A key the form does not show can then
  // never be removed by a save, and a field left as it was is never
  // rewritten ("200" stays a string, `post` stays lower case, an explicit
  // default stays). A field that is not shown any more (another type or
  // source) and had a value is sent as null. `was`: the values the dialog
  // was opened with; null for an add, which sends the whole item.
  const blank = (x) => x == null || x === "" || x === false || (Array.isArray(x) && !x.length);
  const sameVal = (a, b, set) => {
    if (blank(a) && blank(b)) return true;
    if (set) return JSON.stringify([...(a || [])].sort()) === JSON.stringify([...(b || [])].sort());
    return JSON.stringify(a) === JSON.stringify(b);
  };
  function onlyChanged(item, v, was, always, sets) {
    if (!was) return item;
    const out = {};
    for (const k of Object.keys(item)) {
      const touched = always.includes(k) || (k in v ? !sameVal(v[k], was[k], (sets || []).includes(k)) : !blank(was[k]));
      if (touched) out[k] = item[k];
    }
    return out;
  }
  function channelItem(v, was) {
    const it = { name: v.name };
    for (const k of CH_KEYS) it[k] = null;
    it.type = v.type;
    const k = (CH_SECRET[v.type] || {})[v.src];
    if (k && v[k]) it[k] = v[k];
    if (v.type === "telegram" && v.chat_id) it.chat_id = /^-?\d+$/.test(v.chat_id) && Number.isSafeInteger(Number(v.chat_id)) ? Number(v.chat_id) : v.chat_id;
    it.timeout = v.timeout ? dur(v.timeout) : null;
    if (!was) return it;
    for (const x of CH_TYPED) delete it[x];
    const out = onlyChanged(it, v, was, ["name", "type"]);
    // Another type: what only the old one had goes too (else the file
    // would not load); the same type keeps them, untouched.
    if (was.type !== v.type) for (const x of CH_TYPED) out[x] = null;
    return out;
  }
  const CHANNELS = {
    list: "ch-list", also: ["ru-list", "ru-channels"], add: "ch-add", dlg: "dlg-channel", menu: "m-ch", path: "channels", kind: "channel", noun: "channel",
    inUse: "Take it out of those rules first (or delete them), then delete the channel.",
    item: channelItem,
    id: keyId,
    savedKey: (v) => `channel:${v.name}`,
    label: (v) => v.name,
    ask: renameAsk("ch-list", "name", (old, many) => `${many ? "send" : "sends"} to “${old}”`),
    // "Save and send test" (owner decision Y14: the test runs on the saved
    // channel, with the existing route).
    test: async (v) => {
      const r = await call("POST", "channels/test", { name: v.name });
      const ok = r.status === 200 && r.data.ok;
      return ok ? ["ok", `Test sent to “${v.name}”. Check that it arrived.`] : ["bad", `The test to “${v.name}” failed: ${(r.data && r.data.error) || "HTTP " + r.status}.`];
    },
  };
  const DEFAULT_ON = ["stopped", "unhealthy", "restarting"];
  // The condition keys of each rule type (as `alerts::rules::validate`).
  const RULE_FOR = {
    container: ["containers", "exclude", "on", "restarts", "window"],
    http: ["url", "interval", "timeout", "expect_status", "contains", "failures"],
    host: ["metric", "disk", "above", "below", "interval"],
    widget: ["widget", "field", "above", "below", "equals", "not_equals", "on_error"],
    log_match: ["pattern", "containers", "exclude", "stack", "count", "window"],
  };
  // The dialog's values as the API item: the whole rule for an add, only
  // what changed for an edit (the diff rule).
  function ruleItem(v, was) {
    const it = { name: v.name, type: v.type }, cond = RULE_FOR[v.type] || [];
    const val = (k) => (cond.includes(k) && v[k] != null && v[k] !== "" ? v[k] : null);
    for (const k of ["url", "contains", "pattern", "stack", "metric", "widget", "field", "disk"]) it[k] = val(k);
    if (v.metric !== "disk") it.disk = null;
    for (const k of ["containers", "exclude"]) it[k] = val(k) && listOf(v[k]);
    for (const k of ["restarts", "expect_status", "failures", "count", "above", "below"]) it[k] = val(k) == null ? null : Number(v[k]);
    for (const k of ["equals", "not_equals"]) it[k] = val(k) == null ? null : parseJ(v[k]);
    for (const k of ["window", "interval", "timeout"]) it[k] = val(k) == null ? null : dur(v[k]);
    it.on = cond.includes("on") && Array.isArray(v.on) ? v.on : null;
    it.on_error = cond.includes("on_error") && v.on_error ? true : null;
    for (const k of ["for", "cooldown"]) it[k] = v[k] ? dur(v[k]) : null;
    for (const k of ["message", "recovery"]) it[k] = v[k] || null;
    it.channels = Array.isArray(v.channels) ? v.channels : [];
    it.enabled = v.enabled === false ? false : null;
    // A new rule that fires on the default conditions leaves `on` out.
    if (!was && it.on && it.on.length === DEFAULT_ON.length && DEFAULT_ON.every((x) => it.on.includes(x))) it.on = null;
    return onlyChanged(it, v, was, ["name", "type"], ["on", "channels"]);
  }
  const RULES = {
    list: "ru-list", also: ["ch-list"], add: "ru-add", dlg: "dlg-rule", menu: "m-ru", path: "alerts", kind: "rule", noun: "rule",
    item: ruleItem,
    id: keyId,
    savedKey: (v) => `rule:${v.name}`,
    label: (v) => v.name,
  };
  // Checks across fields, shown with the others (ui.js HB.validate).
  function ruleCheck(v) {
    const e = {}, n = (k) => v[k] != null && v[k] !== "";
    const one = (keys, labels) => { if (keys.filter(n).length !== 1) e[keys[0]] = `Set exactly one of: ${labels}.`; };
    if (v.type === "host") one(["above", "below"], "Above or Below");
    if (v.type === "widget") one(["above", "below", "equals", "not_equals"], "Above, Below, Equals or Not equals");
    if (v.type === "log_match" && !n("containers") && !n("stack")) e.containers = "Set Containers, Stack or both.";
    if (v.type === "container" && Array.isArray(v.on) && !v.on.length) e.on = "Pick at least one.";
    return e;
  }
  TABS.notifications = {
    ssr: true,
    init() {
      const dc = $("dlg-channel"), dr = $("dlg-rule");
      listDialog(dc, CHANNELS); listActions(CHANNELS);
      listDialog(dr, RULES); listActions(RULES);
      dr.querySelector("form")._check = ruleCheck;
      // Opening a channel: the read-only lines, the set / found state of the
      // saved secret, and a channel bound to its destination locked.
      dc.addEventListener("hb-open", (e) => {
        const v = e.detail.values || {}, f = dc.querySelector("form");
        const ro = dc.querySelector("[data-ro]");
        ro.querySelector("ul").replaceChildren(...(v._ro || []).map(([a, b]) => el("li", null, a + ": ", code(b, "wrap"))));
        ro.hidden = !(v._ro || []).length;
        const bound = !!v._bound;
        for (const k of ["url", "url_env", "url_file", "token_env", "token_file", "webhook_url_env", "webhook_url_file"]) {
          const x = ctl(f, k);
          x.readOnly = bound;
          x.title = bound ? "Bound to this channel's destination: change it in the config file on the host" : "";
        }
        $$("input[name=type], input[name=src]", f).forEach((r) => { if (bound && !r.checked) { r.disabled = true; r.setAttribute("data-locked", ""); } });
        // A chat id read from a variable (in the file) replaces the field:
        // without a name it is neither checked nor sent.
        const chat = f.querySelector("#ch-chat-id");
        chat.closest(".fld").classList.toggle("ro-off", !!v._chat_env);
        chat.name = v._chat_env ? "" : "chat_id";
        if (!v._chat_env) { chat.value = v.chat_id || ""; chat._was = chat.value; }
        $$(".envst", f).forEach((x) => x.remove());
        for (const [k, ok] of Object.entries(v._st || {})) {
          const x = ctl(f, k);
          const hint = x && x.closest(".fld").querySelector(".hint");
          if (hint) hint.append(el("span", { class: "envst" }, " ", ok ? pill("ok", k.endsWith("_file") ? "file found" : "set") : pill("bad", k.endsWith("_file") ? "no file" : "not set in the container")));
        }
      });
      // Send test from the row: one click, the result in the row and a toast.
      $("ch-list").addEventListener("click", async (e) => {
        const b = e.target.closest("[data-test]");
        if (!b || b.getAttribute("aria-busy") === "true") return;
        const name = b.dataset.test, cell = b.closest("tr").querySelector(".test-res");
        b.setAttribute("aria-busy", "true"); b.setAttribute("aria-disabled", "true");
        const t = b.lastChild, label = t.textContent; t.textContent = "Sending…";
        const r = await call("POST", "channels/test", { name });
        b.removeAttribute("aria-busy"); b.removeAttribute("aria-disabled"); t.textContent = label;
        const ok = r.status === 200 && r.data.ok;
        const msg = ok ? "Test sent" : (r.data && r.data.error) || `HTTP ${r.status}`;
        if (cell) { cell.className = "sub test-res " + (ok ? "ok-t" : "err-line"); cell.textContent = (ok ? "✓ " : "✗ ") + msg; }
        toast(ok ? `Test sent to “${name}”` : `Test to “${name}” failed: ${msg}`, ok ? "ok" : "bad");
        if (r.status === 401) banner("expired");
      });
      // A rule's on / off switch saves on change (owner decision Y21), based
      // on the list's version at the click (the base-hash rule).
      $("ru-list").addEventListener("change", async (e) => {
        const sw = e.target.closest("[data-toggle]");
        if (!sw) return;
        const box = $("ru-list"), tr = sw.closest("tr[data-key]"), base = box.dataset.hash, key = tr.dataset.key;
        const v = rowItem(tr), on = sw.checked;
        sw.disabled = true;
        // Only the switch changes: name and type say which rule, nothing else is sent.
        const r = HB.result(await post("alerts", { base_hash: base, action: "edit", id: keyId(key), item: { name: v.name, type: v.type, enabled: on ? null : false } }));
        if (!r.ok) {
          sw.checked = !on; sw.disabled = false;
          toast(r.note && /changed on disk/.test(r.note[1]) ? `Not switched: the config file changed since this list was loaded. Nothing was changed; check the list and try again.` : (r.note && r.note[1]) || Object.values(r.fields || {}).join(" "), "bad");
          refreshList(lists(RULES));
          return;
        }
        await refreshList(lists(RULES), key);
        const n = rowOf(RULES, key), s2 = n && n.querySelector("[data-toggle]");
        if (s2) s2.focus();
        toast(`Rule “${v.name}” switched ${on ? "on" : "off"}${revNo(r.data)}`);
      });
    },
    load() { return refreshList(lists(CHANNELS)); },
  };

  // ---- widgets: custom-api widgets (server-rendered list) ----
  // Field and display rows, in order: [[a, b], …] without empty rows.
  const kvRows = (form, kind) => $$(`[data-rows="${kind}"] .kv-row`, form).map((r) => $$("input", r).map((i) => i.value.trim())).filter(([a, b]) => a || b);
  const W_FIELD = { fields: "fk.0", rows: "rl.0", status: "status_field" };
  // The dialog as the API item: whole for an add (and for Fetch now), only
  // what changed for an edit (the diff rule, above `onlyChanged`). Header
  // values, the body and `output` are never in it: an edit leaves them as
  // they are in the file.
  function widgetItem(v, form, was, whole) {
    const it = { id: v.id, type: "custom-api", url: v.url };
    for (const k of ["title", "icon", "link"]) it[k] = v[k] || null;
    for (const k of ["refresh", "timeout"]) it[k] = v[k] ? dur(v[k]) : null;
    it.method = v.method === "POST" ? "POST" : null;
    it.enabled = v.enabled === false ? false : null;
    const fl = kvRows(form, "field"), rl = kvRows(form, "row");
    const fields = {};
    for (const [a, b] of fl) if (a) fields[a] = b;
    it.fields = Object.keys(fields).length ? fields : null;
    const rows = rl.map(([label, value]) => ({ label, value }));
    if (v.mode === "rows") { it.rows = rows.length ? rows : null; it.template = null; } else { it.template = v.template || null; it.rows = null; }
    // OK values as typed in the file while their text is unchanged.
    const okSame = was && v.status_ok === was.status_ok && Array.isArray(was._status_ok);
    const ok = okSame ? was._status_ok : (listOf(v.status_ok) || []).map(parseJ);
    it.status = v.status_field ? (ok.length ? { field: v.status_field, ok } : { field: v.status_field }) : null;
    if (!was || whole) return it;
    const out = { id: it.id, type: it.type, url: it.url };
    for (const k of ["title", "icon", "link", "refresh", "timeout", "method"]) if (!sameVal(v[k], was[k])) out[k] = it[k];
    if (v.enabled !== was.enabled) out.enabled = it.enabled;
    if (JSON.stringify(fl) !== JSON.stringify(was.fields || [])) out.fields = it.fields;
    if (v.mode !== was.mode) { out.template = it.template; out.rows = it.rows; } // the display choice changed
    else if (v.mode === "rows") { if (JSON.stringify(rl) !== JSON.stringify(was.rows || [])) out.rows = it.rows; }
    else if (!sameVal(v.template, was.template)) out.template = it.template;
    if (!sameVal(v.status_field, was.status_field) || !sameVal(v.status_ok, was.status_ok)) out.status = it.status;
    return out;
  }
  // Fetch now runs the whole widget, so an existing one needs the keys the
  // page never holds (header values, the body): read from the settings API,
  // used only if that read is the file the dialog was opened on. Otherwise
  // nothing is fetched (never a request without its headers).
  async function previewItem(v, form, c) {
    const was = c.key ? openedWith(c) : null, item = widgetItem(v, form, was, true);
    const keep = (was && was._keep) || [];
    if (!keep.length) return { item };
    const r = await call("GET", "widgets");
    const orig = r.status === 200 && r.data.hash === c.base && ((r.data.widgets || []).find((w) => w.id === was.id && !w.locked) || {}).item;
    if (!orig) return { error: r.status === 401 ? "Not fetched: your session expired." : "Not fetched: the widget's headers or body could not be read from the file this dialog was opened on (it may have changed since). Close and open the widget again." };
    for (const k of keep) item[k] = orig[k] === undefined ? null : orig[k];
    return { item };
  }
  const WIDGETS = {
    list: "w-list", add: "w-add", dlg: "dlg-widget", menu: "m-w", path: "widgets", kind: "widget", noun: "widget",
    inUse: "Change or delete those rules first, then delete the widget.",
    item: (v, was) => widgetItem(v, $("dlg-widget").querySelector("form"), was),
    id: keyId,
    savedKey: (v) => `widget:${v.id}`,
    movedKey: (key) => key, // keys are ids
    label: (v) => v.id,
    ask: renameAsk("w-list", "id", (old, many) => `${many ? "watch" : "watches"} the widget “${old}”`),
    // A server error about the rows lands on their first input.
    fields: (r) => {
      const f = r.fields || {}, out = {};
      for (const [k, m] of Object.entries(f)) out[W_FIELD[k] || k] = m;
      return { ...r, fields: out };
    },
  };
  TABS.widgets = {
    ssr: true,
    init() {
      const dlg = $("dlg-widget"), form = dlg.querySelector("form");
      listDialog(dlg, WIDGETS); listActions(WIDGETS);
      // Rows of name / value pairs: built from the item when the dialog
      // opens, renumbered (fk.0, fk.1, …) after every change.
      const renumber = (box) => $$(".kv-row", box).forEach((r, i) => { const [a, b] = $$("input", r); a.name = a.name.replace(/\.\d+$/, "." + i); b.name = b.name.replace(/\.\d+$/, "." + i); });
      const kvRow = (kind, a, b) => {
        const [la, lb, pa, pb, ka, kb] = kind === "field" ? ["Field name", "Path in the response", "name", "path.to.value", "fk", "fv"] : ["Label", "Value (template)", "Label", "{{field | number}}", "rl", "rv"];
        return el("div", { class: "row kv-row", "data-kind": kind },
          el("input", { class: "in mono", type: "text", name: ka + ".0", value: a, placeholder: pa, "aria-label": la, autocomplete: "off", spellcheck: "false" }),
          el("input", { class: "in mono", type: "text", name: kb + ".0", value: b, placeholder: pb, "aria-label": lb, autocomplete: "off", spellcheck: "false" }),
          el("button", { class: "ia", type: "button", "data-kv-del": "", "aria-label": kind === "field" ? "Remove this field" : "Remove this row", "data-tip": "Remove" }, icon("x")),
          el("span", { class: "kv-got muted" }));
      };
      const fillRows = (kind, list) => { const box = form.querySelector(`[data-rows="${kind}"]`); box.replaceChildren(...list.map(([a, b]) => kvRow(kind, a, b))); renumber(box); };
      dlg.addEventListener("hb-open", (e) => {
        const v = e.detail.values || {};
        fillRows("field", v.fields && v.fields.length ? v.fields : [["", ""]]);
        fillRows("row", v.rows || []);
        $$(".kv-got", form).forEach((x) => { x.textContent = ""; });
        const ro = dlg.querySelector("[data-ro]");
        ro.querySelector("ul").replaceChildren(...(v._ro || []).map(([a, b]) => el("li", null, code(a), ": " + b)));
        ro.hidden = !(v._ro || []).length;
        const url = ctl(form, "url"), bound = !!v._bound;
        url.readOnly = bound;
        url.title = bound ? "Bound to its secret headers: change it in the config file on the host" : "";
        const out = $("wf-preview"); out.hidden = true; out.replaceChildren();
        const first = dlg.querySelector("[role=tab]"); if (first && first.getAttribute("aria-selected") !== "true") first.click();
      });
      form.addEventListener("click", (e) => {
        const add = e.target.closest("[data-kv-add]"), del = e.target.closest("[data-kv-del]");
        if (add) {
          const box = form.querySelector(`[data-rows="${add.dataset.kvAdd}"]`);
          box.append(kvRow(add.dataset.kvAdd, "", "")); renumber(box);
          box.lastChild.querySelector("input").focus();
        } else if (del) {
          const r = del.closest(".kv-row"), box = r.parentElement, next = r.nextElementSibling || r.previousElementSibling;
          r.remove(); renumber(box);
          (next ? next.querySelector("input") : form.querySelector(`[data-kv-add="${box.dataset.rows}"]`)).focus();
        }
      });
      // Fetch now: the unsaved form, run once by the server; the result
      // (the tile and the values it extracted, never the raw response) is
      // shown in the dialog.
      const fetchBtn = dlg.querySelector("[data-act=wf-preview]");
      fetchBtn.addEventListener("click", async () => {
        const c = HB.current();
        if (!c || c.dlg !== dlg || c.busy || fetchBtn.getAttribute("aria-busy") === "true") return;
        const { n, first } = HB.validate(form);
        const out = $("wf-preview");
        if (n) { out.hidden = false; out.replaceChildren(el("p", { class: "err-line", text: `Fix ${n === 1 ? "the highlighted field" : n + " highlighted fields"} first, then fetch again.` })); first.closest("[role=tabpanel]") && dlg.querySelector(`[aria-controls="${first.closest("[role=tabpanel]").id}"]`).click(); first.focus(); return; }
        fetchBtn.setAttribute("aria-busy", "true"); fetchBtn.setAttribute("aria-disabled", "true");
        const t = fetchBtn.lastChild, label = t.textContent; t.textContent = "Fetching…";
        out.hidden = false; out.replaceChildren(muted("Fetching…"));
        const v = HB.values(form);
        const pi = await previewItem(v, form, c);
        const res = pi.item ? await call("POST", "widgets/preview", { item: pi.item }) : { status: 0, data: { error: pi.error } };
        fetchBtn.removeAttribute("aria-busy"); fetchBtn.removeAttribute("aria-disabled"); t.textContent = label;
        const d = res.data || {};
        if (res.status !== 200) {
          // A field's error goes on that field (its tab selected), as a save's would.
          const f = res.status === 422 && d.field && (W_FIELD[d.field] || d.field);
          if (f && HB.fieldError(form, f, d.error)) out.replaceChildren(el("p", { class: "err-line", text: "Not fetched: fix the highlighted field, then fetch again." }));
          else out.replaceChildren(el("p", { class: "err-line" }, res.status === 403 ? [icon("lock"), " "] : null, d.error || (res.status ? `HTTP ${res.status}` : "The server did not answer.")));
          if (res.status === 401) banner("expired");
          return;
        }
        const head = el("div", { class: "pv-head" }, d.ok ? pill("ok", "OK") : pill("bad", "Error"), muted(` ${d.duration_ms} ms`));
        const tile = el("div", { class: "tile widget pv-tile" + (d.ok ? "" : " is-err") }, el("div", { class: "tile-title" }, el("span", { class: "w-title", text: v.title || v.id })));
        if (d.html) {
          // Server-rendered by our widget renderer, every value escaped there.
          const tp = doc.createElement("template");
          tp.innerHTML = d.html;
          tile.append(tp.content);
        }
        if (d.error) tile.append(el("div", { class: "err-line", text: d.error }));
        const vals = d.values || {};
        const show = (x) => (typeof x === "string" ? x : JSON.stringify(x));
        // Next to each field row: what its path found.
        $$('[data-rows="field"] .kv-row', form).forEach((r) => {
          const name = r.querySelector("input").value.trim(), got = r.querySelector(".kv-got");
          got.textContent = !name ? "" : name in vals ? "→ " + show(vals[name]) : "→ not found";
        });
        const list = Object.entries(vals);
        const table = list.length ? el("table", { class: "tbl small pv-vals" }, el("thead", null, el("tr", null, el("th", { text: "Field" }), el("th", { text: "Got" }))),
          el("tbody", null, list.map(([k, x]) => el("tr", null, el("td", null, code(k)), el("td", null, code(show(x), "wrap")))))) : null;
        out.replaceChildren(head, el("div", { class: "pv-body" }, tile, table));
      });
      // System tiles: a single switch that saves on change (owner decision
      // Y21), based on the version of the list on screen at the click.
      const sys = $("sys-tiles");
      sys.addEventListener("change", async () => {
        const on = sys.checked, base = $("w-list").dataset.hash;
        sys.disabled = true;
        const r = HB.result(await post("system", { base_hash: base, enabled: on }));
        sys.disabled = false;
        if (!r.ok) {
          sys.checked = !on;
          toast(r.note && /changed on disk/.test(r.note[1]) ? "Not switched: the config file changed since this page was loaded. Reload to see it, then switch again." : (r.note && r.note[1]) || "Not switched.", "bad");
          return;
        }
        await refreshList(WIDGETS.list);
        sys.focus();
        toast(`System tiles ${on ? "shown" : "hidden"}${revNo(r.data)}`);
      });
    },
    // An outside change: the list and the System tiles switch follow it.
    async load() { const ok = await refreshList(WIDGETS.list); const b = $(WIDGETS.list); if (b && b.dataset.sys) $("sys-tiles").checked = b.dataset.sys === "1"; return ok; },
  };

  // ---- appearance ----
  TABS.appearance = {
    init() {
      const ft = $("f-theme"), fc = $("f-css"), ta = $("css-text");
      onSubmit(ft, () => {
        const v = ctl(ft, "theme").value || null;
        if (v === (this.page.theme ?? null)) { setMsg(ft, "No changes"); toast("No changes"); return; }
        save(ft, "general", { base_hash: S.hash, page: { theme: v } }, () => {
          if (!this.local()) doc.documentElement.dataset.theme = v || "auto";
          return this.load();
        });
      });
      onAct("th-forget", () => {
        try { localStorage.removeItem("rh-theme"); } catch {}
        doc.documentElement.dataset.theme = this.page.theme || "auto";
        $("th-local").hidden = true;
      });
      ta.addEventListener("input", () => { this.size(); if ($("css-live").checked) this.preview(ta.value); });
      ta.addEventListener("keydown", (e) => {
        if (e.key === "Tab" && !e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey && this.tabIndent) {
          e.preventDefault(); ta.setRangeText("  ", ta.selectionStart, ta.selectionEnd, "end"); ta.dispatchEvent(new Event("input", { bubbles: true }));
        }
        if (e.key === "Escape") this.tabIndent = false;
      });
      ta.addEventListener("focus", () => { this.tabIndent = true; });
      $("css-live").addEventListener("change", (e) => this.preview(e.target.checked ? ta.value : null));
      onAct("css-revert", () => { ta.value = this.css; fc._dirty = false; this.size(); this.preview(null); clearErrs(fc); setMsg(fc, "Reverted"); });
      onSubmit(fc, () => {
        if (ta.value.length > this.max) { fieldErr(fc, "css", `custom.css is limited to ${this.max / 1024} KiB.`); return; }
        save(fc, "appearance", { base_hash: this.cssHash, css: ta.value }, (d) => { this.cssHash = d.hash; this.css = ta.value; });
      });
    },
    local() { try { return localStorage.getItem("rh-theme"); } catch { return null; } },
    async load() {
      const [g, c] = await Promise.all([get("general"), get("appearance")]);
      if (g) {
        S.hash = g.hash;
        this.page = g.page || {};
        const s = ctl($("f-theme"), "theme");
        s.value = this.page.theme ?? "";
        s.options[0].textContent = `Default (${g.defaults.theme})`;
        $("f-theme")._dirty = false;
        $("th-local").hidden = !this.local();
      }
      if (c) {
        this.cssHash = c.hash; this.css = c.css; this.max = c.max_bytes;
        $("css-text").value = c.css;
        $("f-css")._dirty = false;
        this.size();
      }
    },
    size() {
      const n = new Blob([$("css-text").value]).size;
      const s = $("css-size");
      s.textContent = `${bytes(n)} of ${bytes(this.max || 65536)}`;
      s.classList.toggle("over", n > (this.max || 65536));
    },
    // Apply the editor to the page (or `null`: back to the saved custom.css).
    preview(text) {
      let st = $("rh-preview");
      const link = doc.querySelector('link[href="/custom.css"]');
      if (text == null) { if (st) st.remove(); if (link) link.disabled = false; return; }
      if (!st) { st = el("style", { id: "rh-preview" }); doc.head.append(st); }
      st.textContent = text;
      if (link) link.disabled = true;
    },
  };

  // ---- history and data ----
  TABS.history = {
    init() {
      const f = $("f-history"), fc = $("f-clear"), conf = $("clr-confirm");
      onSubmit(f, () => {
        const patch = {}, h = this.h;
        const en = ctl(f, "enabled").checked, iv = readCtl(ctl(f, "sample_interval"));
        if (en !== (h.enabled ?? true)) patch.enabled = en;
        if (String(iv ?? "") !== String(h.sample_interval ?? "")) patch.sample_interval = dur(iv);
        if (!Object.keys(patch).length) { setMsg(f, "No changes"); toast("No changes"); return; }
        save(f, "history", { base_hash: S.hash, history: patch }, () => this.load());
      });
      const sub = fc.querySelector("button[type=submit]");
      conf.addEventListener("input", () => { sub.disabled = conf.value.trim().toLowerCase() !== "clear history"; });
      onSubmit(fc, async () => {
        if (conf.value.trim().toLowerCase() !== "clear history") return;
        clearErrs(fc);
        sub.disabled = true;
        const res = await call("POST", "history/clear", {});
        if (res.status === 200 && res.data.ok) { toast("History cleared"); note(fc, "ok", "History cleared."); conf.value = ""; this.load(); } else { fail(res, fc); sub.disabled = false; }
      });
    },
    async load() {
      const d = await get("history");
      if (!d) return;
      S.hash = d.hash;
      this.h = d.history || {};
      const f = $("f-history");
      ctl(f, "enabled").checked = this.h.enabled ?? true;
      ctl(f, "sample_interval").value = this.h.sample_interval ?? "";
      f._dirty = false;
      const db = d.db || {}, dl = $("db-stats");
      if (!db.enabled) { dl.replaceChildren(el("p", { class: "muted" }, "The history database is not available (see the server log), so nothing is recorded.")); $("db-export").hidden = true; return; }
      const row = (k, v) => [el("div", null, el("dt", { text: k }), el("dd", { class: "num" }, v))];
      dl.replaceChildren(...row("Size on disk", bytes(db.size_bytes)), ...row("Host samples", (db.sys_rows ?? 0).toLocaleString()),
        ...row("Container samples", (db.ctr_rows ?? 0).toLocaleString()), ...row("Events", (db.event_rows ?? 0).toLocaleString()),
        ...row("Config revisions", (db.revisions ?? 0).toLocaleString()), ...row("Oldest sample", db.oldest_ts ? when(db.oldest_ts) : "—"));
    },
  };

  // ---- config file ----
  TABS.config = {
    seq: 0,
    init() {
      const f = $("f-raw"), ta = $("raw-text");
      ta.addEventListener("input", () => {
        f._dirty = ta.value !== this.text;
        this.gutter();
        clearTimeout(this.t);
        this.t = setTimeout(() => this.validate(), 400);
      });
      ta.addEventListener("scroll", () => { $("raw-gutter").scrollTop = ta.scrollTop; this.mark(); });
      ta.addEventListener("keydown", (e) => {
        if (e.key === "Tab" && !e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey && this.tabIndent) {
          e.preventDefault(); ta.setRangeText("  ", ta.selectionStart, ta.selectionEnd, "end"); ta.dispatchEvent(new Event("input", { bubbles: true }));
        }
        if (e.key === "Escape") this.tabIndent = false; // Tab moves focus again
        if ((e.ctrlKey || e.metaKey) && e.key === "s") { e.preventDefault(); f.requestSubmit(); }
      });
      ta.addEventListener("focus", () => { this.tabIndent = true; });
      onAct("raw-revert", () => { ta.value = this.text; f._dirty = false; this.gutter(); this.validate(); clearErrs(f); });
      onAct("raw-goto", () => this.goto(this.err && this.err.line));
      onSubmit(f, () => {
        if (ta.value === this.text) { setMsg(f, "No changes"); toast("No changes"); return; }
        save(f, "config", { base_hash: S.hash, text: ta.value }, () => Promise.all([this.load(), this.revs()])).then((ok) => {
          if (!ok) this.validate();
        });
      });
      onAct("rev-close", () => { $("rev-view").hidden = true; });
    },
    async load() {
      const [d] = await Promise.all([get("config"), this.revs()]);
      if (!d) return;
      S.hash = d.hash;
      this.text = d.text;
      $("raw-text").value = d.text;
      $("raw-path").textContent = d.path + (d.exists ? "" : " (does not exist yet)");
      $("f-raw")._dirty = false;
      this.gutter();
      this.status(d.valid || { ok: true });
    },
    gutter() {
      const n = $("raw-text").value.split("\n").length;
      if (n !== this.lines) {
        this.lines = n;
        $("raw-gutter").textContent = Array.from({ length: n }, (_, i) => i + 1).join("\n");
      }
      $("raw-gutter").scrollTop = $("raw-text").scrollTop;
      this.mark();
    },
    mark() {
      const hl = $("raw-hl"), ta = $("raw-text"), line = this.err && this.err.line;
      if (!line) { hl.hidden = true; return; }
      const lh = parseFloat(getComputedStyle(ta).lineHeight) || 20, pad = parseFloat(getComputedStyle(ta).paddingTop) || 0;
      const top = pad + (line - 1) * lh - ta.scrollTop;
      hl.style.top = top + "px";
      hl.style.height = lh + "px";
      hl.hidden = top < 0 || top > ta.clientHeight - 4;
    },
    goto(line) {
      if (!line) return;
      const ta = $("raw-text"), lines = ta.value.split("\n");
      let start = 0;
      for (let i = 0; i < line - 1 && i < lines.length; i++) start += lines[i].length + 1;
      ta.focus();
      ta.setSelectionRange(start, start + (lines[line - 1] || "").length);
      const lh = parseFloat(getComputedStyle(ta).lineHeight) || 20;
      ta.scrollTop = Math.max(0, (line - 1) * lh - ta.clientHeight / 3);
    },
    async validate() {
      const n = ++this.seq;
      const res = await call("POST", "config/validate", { text: $("raw-text").value });
      if (n !== this.seq) return;
      if (res.status === 200) {
        this.status(res.data);
        if (res.data.hash && S.hash && res.data.hash !== S.hash && $("f-raw")._dirty) banner("changed");
      } else if (res.status === 401) banner("expired");
      else this.status({ ok: false, error: res.data.error || `HTTP ${res.status}` });
    },
    status(v) {
      const box = $("raw-status"), go = main.querySelector("[data-act=raw-goto]");
      this.err = v.ok ? null : v;
      go.hidden = !(v.line);
      $("raw-text").toggleAttribute("aria-invalid", !v.ok);
      if (v.ok) {
        const w = v.warnings || [];
        box.className = "raw-status " + (w.length ? "warn" : "ok");
        box.replaceChildren(...[icon(w.length ? "alert" : "check"), el("span", { text: w.length ? "Valid, with warnings:" : "Valid: this config loads." }), w.length ? el("ul", null, w.map((m) => el("li", { text: m }))) : null].filter(Boolean));
      } else if (v.locked) {
        box.className = "raw-status lock";
        box.replaceChildren(icon("lock"), el("span", { text: "Would be refused: " + String(v.error).replace(/^refused:\s*/i, "") }));
      } else {
        box.className = "raw-status bad";
        box.replaceChildren(icon("alert"), el("span", null, v.line ? el("b", { text: `Line ${v.line}, column ${v.col}: ` }) : null, v.error));
      }
      this.mark();
    },
    async revs() {
      const d = await get("revisions");
      const tb = $("rev-list");
      if (!d) return;
      if (!d.enabled) { emptyRow(tb, 6, "Revisions need the history database, which is not available."); return; }
      const list = d.revisions || [];
      if (!list.length) { emptyRow(tb, 6, "No revisions yet. The first save from the app keeps the file as it was, too."); return; }
      const sec = tb.closest("section");
      tb.replaceChildren(...list.map((r, i) => el("tr", { "data-id": r.id },
        el("td", { class: "num" }, code("#" + r.id)), el("td", null, when(r.ts)),
        el("td", null, r.author === "file" ? pill("", "on disk") : el("span", { text: r.author })),
        el("td", { class: "wrap" }, el("span", { text: r.note || "" }), i === 0 ? [" ", pill("ok", "latest")] : null),
        el("td", { class: "num" }, bytes(r.bytes)),
        el("td", { class: "act" }, btn("View", () => this.view(r)), btn("Diff", () => this.diff(r)),
          btn("Restore", () => this.restore(r, sec), "", "refresh")))));
      labelCells(tb);
    },
    show(title, body) {
      $("rev-view-t").textContent = title;
      $("rev-view-b").replaceChildren(...body);
      $("rev-view").hidden = false;
      $("rev-view").scrollIntoView({ block: "nearest", behavior: "smooth" });
    },
    async view(r) {
      const d = await get("revision/" + r.id);
      if (!d) return;
      this.show(`Revision #${r.id} · ${fmtTime(r.ts)} · ${r.note || ""}`, [
        el("div", { class: "rv-acts" }, btn("Open in the editor", () => {
          const ta = $("raw-text");
          if ($("f-raw")._dirty && !confirm("Replace your unsaved edits in the editor?")) return;
          ta.value = d.text; $("f-raw")._dirty = ta.value !== this.text; this.gutter(); this.validate(); ta.focus();
        }, "", "edit"), muted(" Review it there, then Save (or use Restore).")),
        el("pre", { class: "code-view", text: d.text })]);
    },
    async diff(r) {
      const d = await get(`revision/${r.id}/diff`);
      if (!d) return;
      const hunks = d.hunks || [];
      const body = [el("p", { class: "diff-sum" }, "What restoring would change, from the current file to revision #" + r.id + ": ",
        el("span", { class: "d-add", text: `+${d.added}` }), " ", el("span", { class: "d-del", text: `−${d.removed}` }), d.approximate ? muted(" (approximate)") : null)];
      if (!hunks.length) body.push(el("p", { class: "muted", text: "Identical to the current file." }));
      else body.push(el("div", { class: "diff" }, hunks.map((h) => [
        el("div", { class: "d-hunk", text: `@@ -${h.old_start},${h.old_lines} +${h.new_start},${h.new_lines} @@` }),
        ...h.lines.map((l) => el("div", { class: l.op === "+" ? "d-l d-add" : l.op === "-" ? "d-l d-del" : "d-l" }, el("span", { class: "d-op", "aria-hidden": "true", text: l.op }), el("span", { class: "sr", text: l.op === "+" ? "added: " : l.op === "-" ? "removed: " : "" }), el("span", { text: l.text || " " }))),
      ])));
      this.show(`Diff: current → revision #${r.id}`, body);
    },
    async restore(r, sec) {
      const extra = $("f-raw")._dirty ? "\n\nYour unsaved edits in the editor will be replaced." : "";
      if (!confirm(`Restore revision #${r.id} (${fmtTime(r.ts)}, ${r.note || "no note"})?\n\nThis writes it as the config file. The current file stays in the revisions.${extra}`)) return;
      const ok = await save(sec, `revision/${r.id}/restore`, { base_hash: S.hash }, () => this.load());
      if (ok) $("rev-view").hidden = true;
    },
  };

  // ---- security ----
  TABS.security = {
    init() {
      // Change password: a page form on the dialog's submit path (data-hb):
      // all problems at once, busy button, passwords never trimmed.
      const f = $("f-password");
      f._check = (v) => (v.repeat && v.new !== v.repeat ? { repeat: "Does not match the new password." } : {});
      f._submit = async (v) => {
        const r = HB.result(await call("POST", "security/password", { current: v.current, new: v.new }));
        if (!r.ok) return r;
        f.reset();
        S.leaving = true;
        setTimeout(() => { location.href = "/login?next=" + encodeURIComponent("/settings/security"); }, 2500);
        return { ok: true, toast: "Password changed", note: ["ok", "Password changed. Every session was signed out, this one too: sign in again with the new password."] };
      };
      onAct("sign-out-others", async (e) => {
        const b = e.currentTarget, sec = b.closest("section");
        b.disabled = true;
        const res = await call("POST", "security/sign-out-others", {});
        b.disabled = false;
        if (res.status === 200 && res.data.ok) { toast("Other sessions signed out"); note(sec, "ok", "Every other session was signed out."); } else fail(res, sec);
      });
    },
    async load() {
      const d = await get("security");
      if (!d) return;
      const tb = $("lg-list"), list = d.recent_logins || [];
      if (!list.length) { emptyRow(tb, 3, "No logins recorded yet."); return; }
      tb.replaceChildren(...list.map((e) => el("tr", null, el("td", null, when(e.ts)),
        el("td", null, e.kind === "auth.login" ? pill("ok", "signed in") : pill("bad", "failed")),
        el("td", { class: "wrap" }, e.detail ? code(e.detail, "wrap") : muted("—")))));
      labelCells(tb);
    },
  };

  const T = TABS[TAB];
  if (T) { T.init(); if (!T.ssr) T.load(); }
  };
  // This deferred script runs before app.js: wait for DOMContentLoaded, which
  // comes after every deferred script, also when app.js failed to load.
  if (window.HB) start();
  else document.addEventListener("DOMContentLoaded", start, { once: true });
})();
