// Settings pages only (loaded by /settings/*). Loads each tab from
// /api/settings/*, renders lists with createElement/textContent (never
// innerHTML with API data; the one exception is the widget preview, which
// is HTML escaped by our own server renderer), saves with fetch: X-RH-CSRF,
// same-origin credentials and the base_hash of what was loaded.
(() => {
  "use strict";
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
  const TAB = main.dataset.tab;
  const S = { hash: null, saving: 0, recheck: false };

  // ---- never auto-reload a settings page: unsaved edits would be lost ----
  // app.js reloads the page on the SSE "reload" event (any config change).
  // Here that event checks the file hash instead (see checkChanged).
  const ES = window.EventSource;
  if (ES) {
    const Wrapped = function (url, opts) {
      const es = new ES(url, opts);
      const add = es.addEventListener.bind(es);
      es.addEventListener = (type, fn, o) => add(type, type === "reload" ? () => checkChanged() : fn, o);
      return es;
    };
    Wrapped.prototype = ES.prototype;
    window.EventSource = Wrapped;
  }

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

  // ---- API ----
  async function call(method, path, body) {
    const headers = { "X-RH-CSRF": meta.content };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    let r;
    try {
      r = await fetch("/api/settings/" + path, { method, credentials: "same-origin", headers, body: body === undefined ? undefined : JSON.stringify(body) });
    } catch (e) {
      return { status: 0, data: { error: "Network error: the server did not answer (" + e.message + ")" } };
    }
    let data = {};
    try { data = await r.json(); } catch {}
    return { status: r.status, data };
  }
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
  function toast(text, kind) {
    const n = el("div", { class: "toast " + (kind || "ok") }, icon(kind === "bad" ? "alert" : kind === "info" ? "info" : "check"), el("span", { text }));
    $("toasts").append(n);
    setTimeout(() => { n.classList.add("out"); setTimeout(() => n.remove(), 250); }, 4200);
  }
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
  const dirty = () => $$("form", main).some((f) => f._dirty && !f.hidden && f.id !== "f-clear");
  window.addEventListener("beforeunload", (e) => { if (dirty() && !S.leaving) { e.preventDefault(); e.returnValue = ""; } });
  async function checkChanged() {
    if (S.saving) { S.recheck = true; return; }
    if (!S.hash || !TABS[TAB]) return;
    const r = await call("GET", "meta");
    if (r.status !== 200 || r.data.hash === S.hash) return;
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
  function showType(form, type) {
    $$("[data-for]", form).forEach((w) => { w.hidden = !w.dataset.for.split(" ").includes(type); });
  }
  const pick = (o, keys) => { const r = {}; for (const k of keys) if (o && o[k] !== undefined) r[k] = o[k]; return r; };
  const dur = (v) => (v != null && /^\d+$/.test(String(v)) ? Number(v) : v);
  function required(form, names) {
    let ok = true;
    for (const n of names) { const c = ctl(form, n); if (c && !c.value.trim()) { if (ok) fieldErr(form, n, "Required."); ok = false; } }
    if (!ok) note(form, "bad", "Not saved: fill in the required fields.");
    return ok;
  }
  // Entity editors (bookmark, widget, channel, rule, override).
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
  const ENV_RE = /^[A-Z_][A-Z0-9_]*$/;

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

  // ---- bookmarks ----
  const BOOKMARK_KEYS = ["name", "url", "group", "icon", "description"];
  TABS.bookmarks = {
    init() {
      const f = $("f-bookmark");
      onAct("bm-new", () => { f._orig = null; f._id = null; fill(f, {}); openEditor(f, "Add bookmark", $("bm-list").closest(".tbl-wrap")); });
      onSubmit(f, () => {
        clearErrs(f);
        if (!required(f, ["name", "url"])) return;
        const item = { ...pick(f._orig, BOOKMARK_KEYS), ...collect(f) };
        const body = f._id == null ? { base_hash: S.hash, action: "add", item } : { base_hash: S.hash, action: "edit", id: f._id, item };
        save(f, "bookmarks", body, () => { closeEditor(f); return this.load(); });
      });
    },
    async load() {
      const d = await get("bookmarks");
      if (!d) return;
      S.hash = d.hash;
      const list = d.bookmarks || [], tb = $("bm-list"), f = $("f-bookmark"), sec = tb.closest("section");
      if (!list.length) { emptyRow(tb, 4, "No bookmarks yet. Add links to things that are not containers."); return; }
      const reorder = (i, j) => {
        if (j < 0 || j >= list.length) return;
        save(sec, "bookmarks", { base_hash: S.hash, action: "reorder", order: swap(list.map((b) => b.id), i, j) }, () => this.load());
      };
      tb.replaceChildren(...list.map((b, i) => {
        const it = b.item || {};
        return el("tr", null,
          el("td", null, el("span", { class: "nm-t", text: it.name || "(no name)" }), it.description ? el("span", { class: "sub muted", text: it.description }) : null),
          el("td", { class: "url-c" }, code(it.url || "", "wrap")),
          el("td", null, it.group ? code(it.group) : muted("Bookmarks")),
          el("td", { class: "act" }, moveBtns(list, i, reorder),
            ibtn("edit", `Edit ${it.name}`, () => { f._orig = it; f._id = b.id; fill(f, it); openEditor(f, `Edit bookmark: ${it.name}`, tb.closest(".tbl-wrap")); }),
            ibtn("x", `Delete ${it.name}`, () => { if (confirm(`Delete the bookmark "${it.name}"?`)) save(sec, "bookmarks", { base_hash: S.hash, action: "delete", id: b.id }, () => { closeEditor(f); return this.load(); }); }, "danger")));
      }));
      labelCells(tb);
    },
  };

  // ---- widgets ----
  const WIDGET_KEYS = ["id", "type", "title", "icon", "link", "url", "method", "body", "headers", "headers_env", "headers_file", "refresh", "timeout", "enabled", "output", "template", "status", "fields", "rows"];
  TABS.widgets = {
    init() {
      const fs = $("f-system"), f = $("f-widget");
      onSubmit(fs, () => {
        const v = ctl(fs, "enabled").checked;
        if (v === this.sys) { setMsg(fs, "No changes"); toast("No changes"); return; }
        save(fs, "system", { base_hash: S.hash, enabled: v }, () => this.load());
      });
      onAct("w-new", () => this.edit(null));
      onAct("wf-field-add", () => { $("wf-fields").append(this.kvRow("field", "", "")); f._dirty = true; $("wf-fields").lastChild.querySelector("input").focus(); });
      onAct("wf-row-add", () => { $("wf-rows").append(this.kvRow("row", "", "")); f._dirty = true; $("wf-rows").lastChild.querySelector("input").focus(); });
      $$("input[name=wf-mode]", f).forEach((r) => r.addEventListener("change", () => this.mode(r.value)));
      onAct("wf-preview", () => this.preview());
      onSubmit(f, () => {
        clearErrs(f);
        if (!required(f, ["id", "url"])) return;
        const item = this.item();
        const body = f._orig ? { base_hash: S.hash, action: "edit", id: f._orig.id, item } : { base_hash: S.hash, action: "add", item };
        save(f, "widgets", body, () => { closeEditor(f); return this.load(); });
      });
    },
    async load() {
      const [d, env] = await Promise.all([get("widgets"), call("GET", "env")]);
      if (!d) return;
      S.hash = d.hash;
      this.env = {};
      if (env.status === 200) for (const v of env.data.vars || []) this.env[v.name] = v.set;
      this.sys = !!d.system.enabled;
      ctl($("f-system"), "enabled").checked = this.sys;
      $("f-system")._dirty = false;
      this.all = d.widgets || [];
      const onPage = new Set($$("#w-locked [data-id]").map((x) => x.dataset.id));
      const shown = this.all.filter((w) => !w.locked || !onPage.has(w.id));
      const tb = $("w-list"), sec = tb.closest("section");
      if (!shown.length) { emptyRow(tb, 5, "No custom-api widgets yet."); return; }
      const editable = this.all.filter((w) => !w.locked);
      const reorder = (i, j) => {
        if (j < 0 || j >= editable.length) return;
        const ids = this.all.map((w) => w.id);
        const a = ids.indexOf(editable[i].id), b = ids.indexOf(editable[j].id);
        save(sec, "widgets", { base_hash: S.hash, action: "reorder", order: swap(ids, a, b) }, () => this.load());
      };
      tb.replaceChildren(...shown.map((w) => {
        const it = w.item || {};
        const title = el("td", null, el("span", { class: "nm-t", text: it.title || w.id }), " ", code(w.id, "sub-c"));
        if (w.locked) {
          return el("tr", { class: "locked-row" }, title, el("td", { colspan: 3 }, el("span", { class: "lock-note" }, icon("lock"), "Defined in the config file on the host — command widgets can't be created, changed or enabled from the app.")), el("td"));
        }
        const i = editable.indexOf(w);
        const secret = it.headers_env || it.headers_file;
        return el("tr", null, title,
          el("td", { class: "url-c" }, code(it.url || "", "wrap")),
          el("td", { class: "num" }, code(it.refresh != null ? String(it.refresh) : "30s")),
          el("td", null, w.enabled ? pill("ok", "enabled") : pill("off", "disabled"), secret ? el("span", { title: "Has secret headers (set on the host)" }, " ", pill("", "secret headers")) : null),
          el("td", { class: "act" }, moveBtns(editable, i, reorder),
            ibtn("edit", `Edit ${w.id}`, () => this.edit(w)),
            ibtn("x", `Delete ${w.id}`, () => { if (confirm(`Delete the widget "${w.id}"?`)) save(sec, "widgets", { base_hash: S.hash, action: "delete", id: w.id }, () => { closeEditor($("f-widget")); return this.load(); }); }, "danger")));
      }));
      labelCells(tb);
    },
    kvRow(kind, a, b) {
      const [la, lb, pa, pb] = kind === "field" ? ["Field name", "Path in the response", "name", "path.to.value"] : ["Label", "Value (template)", "Label", "{{field | number}}"];
      const f = $("f-widget");
      const row = el("div", { class: "row kv-row", "data-kind": kind },
        el("input", { class: "in mono", type: "text", value: a, placeholder: pa, "aria-label": la, autocomplete: "off", spellcheck: "false" }),
        el("input", { class: "in mono", type: "text", value: b, placeholder: pb, "aria-label": lb, autocomplete: "off", spellcheck: "false" }),
        ibtn("x", kind === "field" ? "Remove field" : "Remove row", () => { row.remove(); f._dirty = true; }, "danger"));
      return row;
    },
    mode(m) {
      $("wf-mode-template").hidden = m !== "template";
      $("wf-mode-rows").hidden = m !== "rows";
      $$("input[name=wf-mode]").forEach((r) => { r.checked = r.value === m; });
    },
    edit(w) {
      const f = $("f-widget"), it = w ? w.item : { type: "custom-api" };
      f._orig = w ? it : null;
      fill(f, it);
      ctl(f, "enabled").checked = it.enabled !== false;
      ctl(f, "method").value = String(it.method || "").toUpperCase() === "POST" ? "POST" : "";
      ctl(f, "status.field").value = (it.status && it.status.field) || "";
      ctl(f, "status.ok").value = it.status && it.status.ok ? it.status.ok.map(String).join(", ") : "";
      $("wf-fields").replaceChildren(...Object.entries(it.fields || {}).map(([k, v]) => this.kvRow("field", k, v)));
      $("wf-rows").replaceChildren(...(it.rows || []).map((r) => this.kvRow("row", r.label || "", r.value || "")));
      this.mode(it.rows && it.rows.length && !it.template ? "rows" : "template");
      // Secret headers: read-only, and they pin the URL (guard.rs secret binding).
      const sec = [], url = ctl(f, "url");
      for (const [h, n] of Object.entries(it.headers_env || {})) sec.push(el("li", null, code(h), " from env ", code(n), " ", this.env[n] ? pill("ok", "set") : pill("bad", "not set")));
      for (const [h, p] of Object.entries(it.headers_file || {})) sec.push(el("li", null, code(h), " from file ", code(p)));
      for (const h of Object.keys(it.headers || {})) sec.push(el("li", null, code(h), " plain header (value in the file)"));
      $("wf-secrets-list").replaceChildren(...sec);
      $("wf-secrets").hidden = !sec.length;
      const bound = !!(Object.keys(it.headers_env || {}).length || Object.keys(it.headers_file || {}).length);
      url.readOnly = bound;
      url.title = bound ? "Bound to its secret headers: change it in the config file on the host" : "";
      $("wf-preview").hidden = true;
      $("wf-preview").replaceChildren();
      openEditor(f, w ? `Edit widget: ${w.id}` : "Add custom-api widget", $("w-list").closest(".tbl-wrap"));
    },
    item() {
      const f = $("f-widget"), v = collect(f, null, { enabled: true });
      const item = { ...pick(f._orig, WIDGET_KEYS), ...v, type: "custom-api" };
      delete item["status.field"]; delete item["status.ok"];
      item.refresh = dur(item.refresh); item.timeout = dur(item.timeout);
      const kv = (box) => $$(".kv-row", box).map((r) => $$("input", r).map((i) => i.value.trim())).filter(([a, b]) => a || b);
      const fields = {};
      for (const [a, b] of kv($("wf-fields"))) if (a) fields[a] = b;
      item.fields = Object.keys(fields).length ? fields : null;
      if ($("wf-mode-rows").hidden) item.rows = null;
      else {
        item.template = null;
        const rows = kv($("wf-rows")).map(([label, value]) => ({ label, value }));
        item.rows = rows.length ? rows : null;
      }
      const sf = ctl(f, "status.field").value.trim();
      const okv = ctl(f, "status.ok").value.split(",").map((s) => s.trim()).filter(Boolean).map(parseJ);
      item.status = sf ? (okv.length ? { field: sf, ok: okv } : { field: sf }) : null;
      return item;
    },
    async preview() {
      const f = $("f-widget"), out = $("wf-preview"), b = f.querySelector("[data-act=wf-preview]");
      clearErrs(f);
      if (!required(f, ["id", "url"])) return;
      out.hidden = false;
      out.replaceChildren(muted("Fetching…"));
      b.disabled = true;
      const res = await call("POST", "widgets/preview", { item: this.item() });
      b.disabled = false;
      const d = res.data || {};
      if (res.status !== 200) {
        if (res.status === 422 && d.field && fieldErr(f, d.field, d.error)) out.replaceChildren(el("p", { class: "err-line", text: "Fix the highlighted field, then fetch again." }));
        else out.replaceChildren(el("p", { class: "err-line" }, res.status === 403 ? icon("lock") : null, " ", d.error || `HTTP ${res.status}`));
        if (res.status === 401) banner("expired");
        return;
      }
      const head = el("div", { class: "pv-head" }, d.ok ? pill("ok", "OK") : pill("bad", "Error"), muted(` ${d.duration_ms} ms`));
      const tile = el("div", { class: "tile widget pv-tile" + (d.ok ? "" : " is-err") }, el("div", { class: "tile-title" }, el("span", { class: "w-title", text: ctl(f, "title").value.trim() || ctl(f, "id").value.trim() })));
      if (d.html) {
        // Server-rendered by our widget renderer, every value escaped there.
        const t = doc.createElement("template");
        t.innerHTML = d.html;
        tile.append(t.content);
      }
      if (d.error) tile.append(el("div", { class: "err-line", text: d.error }));
      const vals = Object.entries(d.values || {});
      const table = vals.length ? el("table", { class: "tbl small pv-vals" }, el("thead", null, el("tr", null, el("th", { text: "Value" }), el("th", { text: "Got" }))),
        el("tbody", null, vals.map(([k, v]) => el("tr", null, el("td", null, code(k)), el("td", null, code(typeof v === "string" ? v : JSON.stringify(v), "wrap")))))) : null;
      out.replaceChildren(head, el("div", { class: "pv-body" }, tile, table));
    },
  };

  // ---- notifications ----
  const CHANNEL_FIELDS = ["name", "type", "url", "url_env", "url_file", "webhook_url_env", "webhook_url_file", "token_env", "token_file", "chat_id", "chat_id_env", "api_base", "headers", "headers_env", "body", "timeout"];
  const RULE_KEYS = ["name", "type", "containers", "exclude", "on", "restarts", "window", "url", "interval", "timeout", "expect_status", "contains", "failures", "pattern", "stack", "count", "metric", "disk", "widget", "field", "above", "below", "equals", "not_equals", "on_error", "for", "cooldown", "channels", "message", "recovery", "enabled"];
  const HEALTH = { ok: ["ok", "ok"], failing: ["bad", "failing"], misconfigured: ["warn", "misconfigured"], idle: ["off", "idle"] };
  TABS.notifications = {
    init() {
      const fc = $("f-channel"), fr = $("f-rule");
      ctl(fc, "type").addEventListener("change", (e) => showType(fc, e.target.value));
      ctl(fr, "type").addEventListener("change", (e) => showType(fr, e.target.value));
      onAct("ch-new", () => this.editChannel(null));
      onAct("ru-new", () => this.editRule(null));
      onSubmit(fc, () => {
        clearErrs(fc);
        if (!required(fc, ["name"])) return;
        const type = ctl(fc, "type").value, v = collect(fc, type);
        for (const k of ["webhook_url_env", "token_env", "url_env"]) {
          if (v[k] && !ENV_RE.test(v[k])) { fieldErr(fc, k, "Use the NAME of an environment variable: A-Z, 0-9 and _ (e.g. RH_SLACK_WEBHOOK_URL), not the secret itself."); return; }
        }
        const orig = fc._orig ? fc._orig.item : null;
        const item = { ...pick(orig, CHANNEL_FIELDS), ...v };
        // Fields of another type go when the type changes.
        if (orig && orig.type !== type) for (const k of ["url_file", "webhook_url_file", "token_file", "chat_id_env", "api_base", "headers", "headers_env", "body"]) item[k] = null;
        const body = fc._orig ? { base_hash: S.hash, action: "edit", id: fc._orig.id, item } : { base_hash: S.hash, action: "add", item };
        save(fc, "channels", body, () => { closeEditor(fc); return this.load(); });
      });
      onSubmit(fr, () => {
        clearErrs(fr);
        if (!required(fr, ["name"])) return;
        const type = ctl(fr, "type").value;
        const item = { ...pick(fr._orig, RULE_KEYS), ...collect(fr, type, { enabled: true, on_error: false }) };
        for (const k of ["window", "interval", "timeout", "for", "cooldown"]) item[k] = dur(item[k]);
        const body = fr._orig ? { base_hash: S.hash, action: "edit", id: fr._orig.name, item } : { base_hash: S.hash, action: "add", item };
        save(fr, "alerts", body, () => { closeEditor(fr); return this.load(); });
      });
    },
    async load() {
      const d = await get("notifications");
      if (!d) return;
      S.hash = d.hash;
      this.channels = d.channels || [];
      this.rules = d.rules || [];
      this.renderChannels();
      this.renderRules();
    },
    secretCell(c) {
      const refs = (c.secrets || []).map((r) => el("div", { class: "sec-ref" }, r.kind === "env" ? [code(r.name), " ", r.set ? pill("ok", "set") : pill("bad", "not set")] : [code(r.path, "wrap"), " ", r.exists ? pill("ok", "file found") : pill("bad", "no file")]));
      const it = c.item || {};
      if (it.url) refs.push(el("div", { class: "sec-ref" }, pill("warn", "plain URL"), muted(" url in the file")));
      if (it.chat_id != null) refs.push(el("div", { class: "sec-ref muted" }, "chat ", code(String(it.chat_id))));
      return refs.length ? refs : muted("—");
    },
    renderChannels() {
      const tb = $("ch-list"), sec = tb.closest("section");
      if (!this.channels.length) { emptyRow(tb, 5, "No channels. Alerts are shown on the page only until you add one and name it in a rule."); return; }
      tb.replaceChildren(...this.channels.map((c) => {
        const st = c.status || {}, h = HEALTH[st.health] || ["off", st.health || "not loaded"];
        const stCell = el("td", { class: "ch-st" }, pill(h[0], h[1]), st.error ? el("div", { class: "sub err-line", text: st.error }) : null,
          st.sent || st.failed ? el("div", { class: "sub muted", text: `${st.sent || 0} sent, ${st.failed || 0} failed` }) : null);
        const test = btn("Send test", async () => {
          test.disabled = true;
          const res = await call("POST", "channels/test", { name: c.id });
          test.disabled = false;
          const ok = res.status === 200 && res.data.ok;
          const msg = ok ? "Test notification sent" : (res.data && res.data.error) || `HTTP ${res.status}`;
          stCell.querySelectorAll(".test-res").forEach((x) => x.remove());
          stCell.append(el("div", { class: "sub test-res " + (ok ? "ok-t" : "err-line"), text: (ok ? "✓ " : "✗ ") + msg }));
          toast(ok ? `Test sent to ${c.id}` : `Test failed: ${msg}`, ok ? "ok" : "bad");
          if (res.status === 401) banner("expired");
        }, "", "zap");
        return el("tr", null,
          el("td", null, code(c.id, "nm")), el("td", null, pill("", c.type || "?")),
          el("td", null, this.secretCell(c)), stCell,
          el("td", { class: "act" }, test, ibtn("edit", `Edit ${c.id}`, () => this.editChannel(c)),
            ibtn("x", `Delete ${c.id}`, () => { if (confirm(`Delete the channel "${c.id}"? Rules that send to it must be changed first.`)) save(sec, "channels", { base_hash: S.hash, action: "delete", id: c.id }, () => { closeEditor($("f-channel")); return this.load(); }); }, "danger")));
      }));
      labelCells(tb);
      if (this.channels.some((c) => c.warnings && c.warnings.length)) note(sec, "warn", this.channels.flatMap((c) => (c.warnings || []).map((w) => `${c.id}: ${w}`)));
    },
    editChannel(c) {
      const f = $("f-channel"), it = c ? c.item : { type: "slack" };
      f._orig = c;
      fill(f, { ...it, name: c ? c.id : "" });
      const type = it.type || "slack";
      ctl(f, "type").value = type;
      showType(f, type);
      // Read-only: secret files, api_base and header secrets (set on the host; bound to the destination).
      const ro = [];
      for (const k of ["url_file", "webhook_url_file", "token_file", "chat_id_env", "api_base", "body"]) if (it[k] != null) ro.push(el("li", null, code(k), " = ", code(String(it[k]), "wrap")));
      for (const [h, n] of Object.entries(it.headers_env || {})) ro.push(el("li", null, "header ", code(h), " from env ", code(n)));
      for (const h of Object.keys(it.headers || {})) ro.push(el("li", null, "header ", code(h), " (plain, in the file)"));
      $("ch-ro-list").replaceChildren(...ro);
      $("ch-ro").hidden = !ro.length;
      const bound = (it.type === "telegram" && it.api_base != null) || (it.type === "webhook" && Object.keys(it.headers_env || {}).length > 0);
      for (const k of ["token_env", "url_env", "url"]) { const x = ctl(f, k); x.readOnly = bound; x.title = bound ? "Bound to this channel's destination: change it in the config file on the host" : ""; }
      // set / not set next to the saved env names
      $$(".envst", f).forEach((x) => x.remove());
      for (const r of (c && c.secrets) || []) {
        const x = r.kind === "env" && ctl(f, r.field);
        if (x) x.after(el("span", { class: "envst" }, r.set ? pill("ok", "set") : pill("bad", "not set in the container")));
      }
      openEditor(f, c ? `Edit channel: ${c.id}` : "Add channel", $("ch-list").closest(".tbl-wrap"));
    },
    summary(r) {
      const j = (a) => (Array.isArray(a) ? a.join(", ") : a);
      switch (r.type) {
        case "container": return `${j(r.containers) || "*"}${r.exclude ? " except " + j(r.exclude) : ""} on ${j(r.on) || "stopped, unhealthy, restarting"}${r.restarts ? `, ${r.restarts} restarts` : ""}`;
        case "http": return `${r.url || "?"}${r.expect_status ? " → " + r.expect_status : ""}${r.contains ? ` contains "${r.contains}"` : ""}`;
        case "host": return `${r.metric || "?"}${r.disk ? " " + r.disk : ""}${r.above != null ? " > " + r.above : ""}${r.below != null ? " < " + r.below : ""}`;
        case "widget": return `${r.widget || "?"}.${r.field || "?"}${r.above != null ? " > " + r.above : ""}${r.below != null ? " < " + r.below : ""}${r.equals != null ? " = " + r.equals : ""}${r.not_equals != null ? " ≠ " + r.not_equals : ""}${r.on_error ? ", on error" : ""}`;
        case "log_match": return `/${r.pattern || "?"}/ in ${r.stack ? "stack " + r.stack : j(r.containers) || "?"}${r.count ? `, ${r.count}×` : ""}${r.window ? " in " + r.window : ""}`;
        default: return "";
      }
    },
    renderRules() {
      const tb = $("ru-list"), sec = tb.closest("section");
      if (!this.rules.length) { emptyRow(tb, 6, "No alert rules."); return; }
      tb.replaceChildren(...this.rules.map((x) => {
        const r = x.item || {}, chans = r.channels || [];
        return el("tr", { class: r.enabled === false ? "stopped" : "" },
          el("td", null, code(r.name, "nm"), r.enabled === false ? [" ", pill("off", "disabled")] : null),
          el("td", null, pill("", r.type)),
          el("td", { class: "wrap" }, el("span", { class: "mono-s", text: this.summary(r) })),
          el("td", null, chans.length ? chans.map((c) => el("span", { class: "chip-s", text: c })) : muted("page only")),
          el("td", { class: "muted" }, `for ${r.for ?? (r.type === "container" ? "30s" : "0s")}, cooldown ${r.cooldown ?? "15m"}`),
          el("td", { class: "act" }, ibtn("edit", `Edit ${r.name}`, () => this.editRule(x)),
            ibtn("x", `Delete ${r.name}`, () => { if (confirm(`Delete the rule "${r.name}"?`)) save(sec, "alerts", { base_hash: S.hash, action: "delete", id: r.name }, () => { closeEditor($("f-rule")); return this.load(); }); }, "danger")));
      }));
      labelCells(tb);
    },
    editRule(x) {
      const f = $("f-rule"), it = x ? x.item : { type: "container" };
      f._orig = x ? it : null;
      // Channel choices: the saved channels plus any the rule names.
      const names = Array.from(new Set([...this.channels.map((c) => c.id), ...(it.channels || [])]));
      const box = $("ru-channels-opts");
      box.replaceChildren(...(names.length ? names.map((n) => el("label", { class: "cb" }, el("input", { type: "checkbox", name: "channels", value: n, "data-t": "set" }), el("span", { class: "mono", text: n }))) : [muted("No channels yet (add one above).")]));
      fill(f, it);
      ctl(f, "enabled").checked = it.enabled !== false;
      const type = it.type || "container";
      ctl(f, "type").value = type;
      if (!it.metric) ctl(f, "metric").value = "disk";
      showType(f, type);
      openEditor(f, x ? `Edit rule: ${it.name}` : "Add rule", $("ru-list").closest(".tbl-wrap"));
    },
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
      const f = $("f-password");
      onSubmit(f, async () => {
        clearErrs(f);
        const cur = ctl(f, "current").value, nw = ctl(f, "new").value, rep = ctl(f, "repeat").value;
        if (!cur) { fieldErr(f, "current", "Required."); return; }
        if ([...nw].length < 8) { fieldErr(f, "new", "At least 8 characters."); return; }
        if (nw !== rep) { fieldErr(f, "repeat", "Does not match the new password."); return; }
        const sub = f.querySelector("button[type=submit]");
        sub.disabled = true; setMsg(f, "Saving…");
        const res = await call("POST", "security/password", { current: cur, new: nw });
        sub.disabled = false; setMsg(f, "");
        if (res.status === 200 && res.data.ok) {
          f.reset();
          note(f, "ok", "Password changed. Every session was signed out, this one too: sign in again with the new password.");
          toast("Password changed");
          S.leaving = true;
          setTimeout(() => { location.href = "/login?next=" + encodeURIComponent("/settings/security"); }, 2500);
        } else if (res.status === 400 && /current/i.test(res.data.error || "")) fieldErr(f, "current", res.data.error);
        else fail(res, f);
      });
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
  if (T) { T.init(); T.load(); }
})();
