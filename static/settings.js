// Settings pages only (loaded by /settings/*). Every tab is rendered by the
// server with its values (signed in only); this script only saves. Lists
// use the dialog (ui.js, window.HB) and are swapped in from the tab's
// `?frag=list` after a save; settings with several fields that stay on the
// page are page forms on the same submit path (`pageForm`); a single
// control saves when changed (`saveOnChange`). Nothing here inserts API
// data as HTML (the exceptions are the widget preview and the list
// fragments, both escaped by our own server). Every write is HB.api:
// X-RH-CSRF, same-origin credentials and the base_hash the user started
// from (THE BASE-HASH RULE, above `post`). Runs once window.HB exists:
// ui.js comes with app.js, which is deferred after this script.
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

  // Phones: the tab strip scrolls; show the current tab, and fade the edge
  // that has more tabs behind it (SET-23).
  const nav = $("set-nav"), cur = nav && nav.querySelector("a[aria-current]");
  if (nav) {
    const edges = () => {
      const more = nav.scrollWidth - nav.clientWidth;
      nav.classList.toggle("more-l", more > 1 && nav.scrollLeft > 1);
      nav.classList.toggle("more-r", more > 1 && nav.scrollLeft < more - 1);
    };
    if (cur && nav.scrollWidth > nav.clientWidth) cur.scrollIntoView({ inline: "center", block: "nearest" });
    nav.addEventListener("scroll", edges, { passive: true });
    window.addEventListener("resize", edges, { passive: true });
    edges();
  }

  const main = $("set-main"), meta = doc.querySelector('meta[name="rh-csrf"]');
  if (!main || main.dataset.signed !== "1" || !meta) return;
  // /static/app.js (window.HB) did not load: stay the no-JavaScript page.
  // Its "Saving needs JavaScript" notes are still shown (no html.hb-ready).
  if (!window.HB) { onReload = () => {}; return; }
  // The tab, or on another page that carries a tab's lists and dialogs (the
  // Alerts page; ui::settings::embed) that tab, read from its own `?frag=`.
  const TAB = main.dataset.tab, FRAG = main.dataset.frag || location.pathname;
  const S = { hash: null, saving: 0, recheck: false, refused: 0 };
  const EMBED = main.classList.contains("set-embed");
  // `/settings` opens the tab used last in this browser (Y17; app.js
  // points the Settings links at it).
  if (location.pathname.startsWith("/settings")) try { localStorage.setItem("rh-set-tab", TAB); } catch {}

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
  const pill = (kind, text) => el("span", { class: "pill " + (kind || ""), text });
  const code = (text, cls) => el("code", { class: cls || "", text });
  const muted = (text) => el("span", { class: "muted", text });
  function bytes(n) {
    const u = ["B", "KiB", "MiB", "GiB", "TiB"];
    let i = 0, v = n;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    return (i ? v.toFixed(v < 10 ? 1 : 0) : v) + " " + u[i];
  }
  const fmtTime = (ts) => new Date(ts * 1000).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  // A control of a form by name (the first of a group).
  function ctl(form, name) {
    const c = form.elements.namedItem(name);
    return c && !c.tagName && c.length ? c[0] : c;
  }

  // ---- API (HB.api: the CSRF header, a fresh token after a new sign-in) ----
  const call = (method, path, body) => HB.api(method, "/api/settings/" + path, body);

  // ---- banners: an outside change while there is input, a lost session ----
  const BANNERS = {
    changed: ["info", "info", "The config file was changed elsewhere (another tab or a hand edit).", "Saving these edits gives a note with the ways out; nothing is overwritten."],
    expired: ["bad", "lock", "Your session expired.", "Sign in again in a new tab, then save here again; your input stays."],
  };
  function banner(kind) {
    const box = $("set-banners");
    if (box.querySelector(`[data-b="${kind}"]`)) return;
    const [cls, ic, head, text] = BANNERS[kind];
    const act = kind === "expired" ? el("a", { class: "btn sm primary", href: "/login?next=" + encodeURIComponent(location.pathname), target: "_blank", rel: "noopener" }, icon("log-in"), "Sign in (new tab)") : null;
    const x = el("button", { type: "button", class: "icon-btn sm", "aria-label": "Dismiss", title: "Dismiss", onclick: () => b.remove() }, icon("x"));
    const b = el("div", { class: "banner " + cls, role: "alert", "data-b": kind }, icon(ic),
      el("div", null, el("strong", { text: head }), " ", text, act ? el("span", { class: "b-act" }, act) : null), x);
    box.append(b);
  }
  // The one toast system (HB.toast: capped, dismissible, never over a dialog).
  const toast = (text, kind) => HB.toast(text, { kind: kind || "ok" });
  const moved = (r) => r.note && /changed on disk/.test(r.note[1]);
  const why = (r) => (r.note && r.note[1]) || Object.values(r.fields || {}).join(" ");

  // ---- unsaved input and outside changes ----
  // Page forms say whether they hold input (`form._dirty`); dialogs are
  // HB.dirty(). Nothing reloads or refreshes over either.
  const pageDirty = () => $$("form[data-hash]", main).some((f) => f._dirty && f._dirty());
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
    const T = TABS[TAB];
    if (!S.hash || !T || !T.load) return;
    if (doc.querySelector("dialog.dlg[open]")) { S.later = true; return; }
    const r = await call("GET", "meta");
    if (r.status !== 200 || r.data.hash === S.hash) return;
    if (doc.querySelector("dialog.dlg[open]")) { S.later = true; return; }
    if (dirty()) banner("changed");
    // A page that only carries a tab's dialogs (the Overview, the Alerts
    // page) reloads like any other page (owner decision Y12): its greeting,
    // title, strips and tables follow the file too. Not over a dialog or
    // input (above), and not right after one of its own writes was refused
    // for this very change: that write's message stays, and its list was
    // refreshed already.
    else if (EMBED) { if (Date.now() - S.refused < 5000) await T.load(); else { S.leaving = true; location.reload(); } }
    else if (await T.load()) toast("Reloaded: the config file changed", "info");
  }

  // ---- values ----
  function parseJ(v) {
    if (v === "true") return true;
    if (v === "false") return false;
    if (/^-?\d+(\.\d+)?$/.test(v)) return Number(v);
    return v;
  }
  const dur = (v) => (v != null && /^\d+$/.test(String(v)) ? Number(v) : v);
  const onAct = (act, fn) => $$(`[data-act="${act}"]`, main).forEach((b) => b.addEventListener("click", fn));
  // A textarea keeps its text exactly (HB.values trims every field).
  const formVals = (form) => { const o = HB.values(form); $$("textarea[name]", form).forEach((t) => { o[t.name] = t.value; }); return o; };
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

  const TABS = {};
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
    if (!ids.some((id) => $(id))) return false; // none of them on this page (the Overview)
    let r;
    const all = new URLSearchParams(location.search).get("revs") === "all" ? "&revs=all" : ""; // Config file: every revision
    try { r = await fetch(FRAG + "?frag=list" + all, { credentials: "same-origin", cache: "no-store" }); } catch { return false; }
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
        // A changed row is replaced at once (its new self takes focus back); a
        // deleted one fades out where it was (HB.remove), out of the way at once.
        const keep = new Set(want), still = new Set(Array.from(b.rows).map((x) => x.dataset.key));
        Array.from(a.rows).forEach((x) => {
          if (keep.has(x) || x._gone) return;
          if (still.has(x.dataset.key)) { x.remove(); return; }
          x._gone = 1; x.removeAttribute("data-key"); x.querySelectorAll("[data-key]").forEach((k) => k.removeAttribute("data-key"));
          HB.remove(x);
        });
        const live = () => Array.from(a.rows).filter((x) => !x._gone);
        want.forEach((w, i) => { const c = live()[i]; if (c !== w) a.insertBefore(w, c || null); });
      }
      const row = flash && box.querySelector(`tr[data-key="${CSS.escape(flash)}"]`);
      if (row) HB.flash(row);
    }
    if (S.after) S.after(); // a tab's filters and selection, put back on the new rows
    return done;
  }
  // THE BASE-HASH RULE (every list form, the next ones included): a write is
  // based on the file version the user was looking at when they started it.
  // A dialog takes its base when it opens (ui.js: the `data-hash` of the
  // list its trigger sits in), a confirm, an arrow click or a row switch
  // takes it before anything is awaited, and that base is sent as base_hash.
  // Nothing adopts a newer hash on its behalf, so the server answers 409
  // whenever the file moved, and an id that is a list position can never
  // hit another entry. The one hash a control may move to without asking
  // is one written by a save from this page: the user's own write is not an
  // outside change. So after a save every control whose base was the file
  // that save started from (its base_hash) moves to the file it wrote: the
  // `data-hash` of lists, switches and boxes, page forms' base and an open
  // dialog's (`adopt`; "Save and send test" stays open on what it saved:
  // HB.edited). A control already based on an older file stays there (its
  // save still gets the 409), and a write that replaces the whole file (the
  // raw editor, a restore or an Undo) moves nothing else. custom.css is its
  // own file with its own hash: it never moves a config base, nor S.hash,
  // which only says which config version the page shows.
  //
  // A write: counted in S.saving like save() does, so a config change event
  // that arrives meanwhile waits for the answer and then finds the new hash
  // instead of reloading the list. A 401 that the fresh token did not cure
  // shows the "session expired" banner with its Sign in action (every save
  // path; the caller puts its control back).
  const CSS_PATH = "appearance";
  const WHOLE = (path) => path === "config" || path.startsWith("revision/");
  async function post(path, body) {
    S.saving++;
    let r;
    try { r = await call("POST", path, body); } finally { S.saving--; }
    if (r.status === 200 && r.data && r.data.hash && path !== CSS_PATH) {
      if (!WHOLE(path)) adopt(body.base_hash, r.data.hash);
      S.hash = r.data.hash;
    }
    if (r.status === 401) banner("expired");
    if (r.status === 409) S.refused = Date.now();
    if (S.recheck && !S.saving) { S.recheck = false; setTimeout(checkChanged, 0); }
    return r;
  }
  // Every base on `from` (the file before this page's own save) moves to `to`.
  function adopt(from, to) {
    if (!from || !to || from === to) return;
    $$("[data-hash]").forEach((x) => { if (x.dataset.hash === from) x.dataset.hash = to; });
    $$("form[data-hb]").forEach((f) => { if (f._base === from) f._base = to; });
    const c = HB.current();
    if (c && c.base === from) c.base = to;
  }
  const itemOfEl = (x) => { try { return JSON.parse((x && x.dataset && x.dataset.item) || "{}"); } catch { return {}; } };
  const rowItem = (tr) => itemOfEl(tr.querySelector("[data-item]"));
  const rowOf = (o, key) => { const b = $(o.list); return b && b.querySelector(`tr[data-key="${CSS.escape(key)}"]`); };
  const keyId = (key) => key.slice(key.indexOf(":") + 1);
  const revNo = (d) => (d.revision != null ? ` · revision #${d.revision}` : "");
  const quoted = (names) => names.map((n) => `“${n}”`).join(", ");
  const lists = (o) => [o.list].concat(o.also || []);
  // The values a dialog was opened with (or, after "Save and send test",
  // what it saved): what an edit is re-found by after a 409.
  const openedWith = (c) => c.was || itemOfEl(c.trigger);

  // After a 409: the page shows the file as it is now, and this is its
  // hash. A tab (and the Alerts page) refreshes the dialog's list and takes
  // that list's hash; a page that carries the dialog without its list (the
  // Overview) swaps in its own parts (`TABS[TAB].load`) and takes the hash
  // they carry. Never a list that is not on the page.
  async function freshBase(o) {
    if ($(o.list)) return (await refreshList(lists(o))) && $(o.list).dataset.hash;
    const T = TABS[TAB];
    if (!T || !T.load || !(await T.load())) return null;
    return S.hash;
  }
  // A list's add / edit dialog. `o`: {list, also, path, kind, noun,
  // item(values, was) → the API item, keep(item, was, base) → the item with
  // the keys the form does not show (async), id(key) → the API id,
  // savedKey(values, editKey, rowsBefore), movedKey(key, newIndex),
  // label(values, c), ask(values, c) → a note that stops the first Save
  // (a rename that changes rules), test(values, key, data) → the note of a
  // "save and …" action, request(values, edit, was, c) → the request body
  // without base_hash when it is not {action, id, item} (null: nothing
  // changed, nothing is sent)}. Saves send the dialog's own base (c.base),
  // never S.hash.
  function listDialog(dlg, o) {
    if (!dlg) return;
    dlg._submit = async (v, ctx) => {
      const c = HB.current();
      const edit = ctx.mode === "edit" && ctx.key;
      const was = openedWith(c);
      // Nothing changed since the dialog opened (or since its last save):
      // nothing is sent, so a moved file is no conflict either. "Save and
      // send test" still sends the test.
      if (edit && HB.pristine()) {
        if (ctx.action !== "save" && o.test) {
          const t = await o.test(v, c.key, {});
          return { ok: true, stay: true, note: [t[0], `No changes to save. ${t[1]}`] };
        }
        return { ok: true, toast: "No changes", focus: `[data-dlg][data-key="${CSS.escape(c.key)}"]` };
      }
      const stop = edit && o.ask && o.ask(v, c, was);
      if (stop) return stop;
      const before = $$("tr[data-key]", $(o.list)).length;
      // An edit sends only what changed (THE DIFF RULE, above `onlyChanged`).
      const req = o.request ? o.request(v, edit, was, c) : edit ? { action: "edit", id: o.id(c.key), item: o.item(v, was) } : { action: "add", item: o.item(v, null) };
      if (!req) return { ok: true, toast: "No changes", focus: `[data-dlg][data-key="${CSS.escape(c.key)}"]` };
      const body = { base_hash: c.base, ...req };
      const r = HB.result(await post(o.path, body), {
        // 409, "Reload and keep my input": show the file as it is now and
        // keep every value typed. RETARGET RULE: an add simply moves to the
        // new version. An edit moves only when the entry it was opened on
        // can be found again unambiguously: exactly one entry of the new file
        // has all the values the dialog was opened with. Then key and base
        // follow it. Otherwise Save (and Delete…) stay disabled and only
        // Discard is offered: the input can be copied from the fields, but it
        // can never be saved onto another entry.
        // The new base is the hash of what the page now shows for this
        // dialog: its list, or on a page without one (the Overview's "Add")
        // the page parts that carry it, swapped in first (`freshBase`).
        reload: async () => {
          const base = await freshBase(o);
          const cc = HB.current(), box = $(o.list);
          if (!cc || cc.dlg !== dlg) return;
          if (!base) { HB.note("bad", "Could not load the current file. Your input is still here; try again."); return; }
          if (!edit) { cc.base = base; HB.note("ok", "Reloaded the current file. Save again to add it."); ctx.form.querySelector("[type=submit]").focus(); return; }
          const same = box ? $$("tr[data-key]", box).filter((tr) => JSON.stringify(rowItem(tr)) === JSON.stringify(was)) : [];
          if (same.length === 1) {
            cc.key = same[0].dataset.key; cc.base = base;
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
      if (S.saved && !d.unchanged) await S.saved(); // a page that carries the dialog (Overview)
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
        toast: d.unchanged ? "No changes" : `${edit ? "Saved" : "Added"} “${o.label(v, c)}”${revNo(d)}`,
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
  const confirmDelete = (o, name) => HB.confirm(o.confirmText ? o.confirmText(name) : { title: `Delete the ${o.noun} “${name}”?`, body: "It is removed from the config file. You can undo this from the message that follows, or restore it later under Config file.", ok: `Delete ${o.noun}` });
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
    const res = await post(o.path, { base_hash: base, ...(o.delBody ? o.delBody(key) : { action: "delete", id: o.id(key) }) });
    const r = HB.result(res);
    if (!r.ok) {
      const d = res.data || {};
      toast(d.used_by ? d.error : moved(r) ? `Not ${o.keepRow ? "removed" : "deleted"}: the config file changed since you asked. Nothing was removed; check the list and try again.` : why(r), "bad");
      refreshList(lists(o));
      return;
    }
    const row = rowOf(o, key);
    if (row && !o.keepRow) { HB.remove(row); await new Promise((res) => setTimeout(res, 170)); }
    await refreshList(lists(o), o.keepRow ? key : null);
    const d = r.data;
    const undo = undoAction(base, d, () => refreshList(lists(o), key), `Restored “${name}”`);
    HB.toast(`${o.deleted ? o.deleted(name) : `Deleted “${name}”`}${revNo(d)}`, undo ? { action: undo } : {});
    const add = $(o.add);
    if (add && (doc.activeElement === doc.body || !doc.activeElement)) add.focus();
  }
  // Undo of a write (owner decision Y16): restore the revision just before
  // it, which holds the file as it was; its hash proves that before
  // anything is written (the revision list has no hashes, so this fetches
  // that one revision's text). Refused (409) if the file changed since the
  // write. `before`: the base the write was sent with; `d`: its answer.
  function undoAction(before, d, refresh, said) {
    const after = d.hash, rev = d.revision;
    if (rev == null || d.unchanged) return null;
    return {
      label: "Undo",
      run: async () => {
        const prev = await call("GET", `revision/${rev - 1}`);
        if (prev.status !== 200 || prev.data.hash !== before) { toast("Cannot undo: the file as it was before is not in the revisions. Look under Config file.", "bad"); return; }
        const u = await post(`revision/${rev - 1}/restore`, { base_hash: after });
        if (u.status === 409) { toast("Not undone: the config file changed since. Its revisions are under Config file.", "bad"); return; }
        const ur = HB.result(u);
        if (!ur.ok) { toast(ur.note ? ur.note[1] : "Not undone.", "bad"); return; }
        await refresh();
        toast(`${said}${revNo(ur.data)}`);
      },
    };
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
    // Optimistic: the row takes its new place now; a refusal puts it back.
    const other = rows[j], flip = () => (dir === "up" ? HB.swap(other, tr) : HB.swap(tr, other));
    flip();
    (async () => {
      const r = HB.result(await post(o.path, { base_hash: base, action: "reorder", order }));
      btn.removeAttribute("aria-busy");
      o.moving = false;
      if (!r.ok) {
        if (tr.isConnected && other.isConnected) { dir === "up" ? HB.swap(tr, other) : HB.swap(other, tr); }
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
    // A condition of another type is not shown: like a field that is not
    // there, it is sent (as null) only when the rule had a value for it.
    const shown = { ...v };
    for (const k of Object.values(RULE_FOR).flat()) if (!cond.includes(k)) delete shown[k];
    if (v.metric !== "disk") delete shown.disk;
    return onlyChanged(it, shown, was, ["name", "type"], ["on", "channels"]);
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
      // The Alerts page: a firing rule's name opens that rule's dialog here,
      // as its row's Edit does (its link reloads the page with ?edit= when
      // the rule is not in the list).
      doc.addEventListener("click", (e) => {
        const a = e.target.closest && e.target.closest("a[data-rule]");
        if (!a || e.button || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
        const t = $("ru-list").querySelector(`[data-dlg][data-key="rule:${CSS.escape(a.dataset.rule)}"]`);
        if (t) { e.preventDefault(); t.click(); }
      });
      // A rule's on / off switch saves on change (owner decision Y21), based
      // on the list's version at the click (the base-hash rule).
      $("ru-list").addEventListener("change", async (e) => {
        const sw = e.target.closest("[data-toggle]");
        if (!sw) return;
        const box = $("ru-list"), tr = sw.closest("tr[data-key]"), base = box.dataset.hash, key = tr.dataset.key;
        const v = rowItem(tr), on = sw.checked;
        HB.hold(sw, true);
        // Only the switch changes: name and type say which rule, nothing else is sent.
        const r = HB.result(await post("alerts", { base_hash: base, action: "edit", id: keyId(key), item: { name: v.name, type: v.type, enabled: on ? null : false } }));
        if (!r.ok) {
          sw.checked = !on; HB.hold(sw, false); HB.flash(tr);
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
      if (sys) sys.addEventListener("change", async () => {
        const on = sys.checked, base = $("w-list").dataset.hash;
        HB.hold(sys, true);
        const r = HB.result(await post("system", { base_hash: base, enabled: on }));
        HB.hold(sys, false);
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


  // ---- page forms: a setting with several fields that stays on the page ----
  // General, Sample interval, custom.css and the raw editor are forms with
  // `data-hb` (ui.js: validation, busy, errors on their fields, the note)
  // and `data-hash`, the file version their values were rendered from: the
  // form's base (THE BASE-HASH RULE). `o`: {path, body(now, changed) → the
  // request without base_hash, fresh() → {hash, values} of the file now
  // (for a 409), done(d, now, changed), saved(d, changed) → the toast}. A
  // save sends only the fields whose text changed (THE DIFF RULE); nothing
  // changed is "No changes" and no request. After a save the form is based
  // on what its own save wrote; nothing else moves its base.
  function pageForm(form, o) {
    if (!form) return;
    const vals = () => (o.values ? o.values(form) : formVals(form));
    const init = () => {
      form._base = form.dataset.hash;
      form._open = vals();
      // A value left as it was rendered is not checked again (ui.js).
      $$("input:not([type=checkbox]):not([type=radio]),textarea", form).forEach((c) => { c._was = c.value; });
    };
    init();
    form._init = init;
    form._dirty = () => !same(vals(), form._open) || !!(form._hb && form._hb.busy);
    const submit = form.querySelector("[type=submit]");
    form._submit = async () => {
      const now = vals(), was = form._open;
      const changed = Object.keys(now).filter((k) => !same(now[k], was[k]));
      const refocus = o.refocus ? o.refocus() : null;
      if (!changed.length) return { ok: true, toast: "No changes", refocus };
      if (o.check) { const f = o.check(now); if (f) return { ok: false, fields: f }; }
      const base = form._base;
      const res = await post(o.path, { base_hash: base, ...o.body(now, changed) });
      const r = HB.result(res, {
        // 409, "Reload and keep my input": the form moves to the file as it
        // is now only when every value it was opened with is still there
        // (RETARGET RULE, as for an edit dialog). Otherwise it cannot be
        // saved: copy what you need, then load the current file.
        reload: async () => {
          const f = await o.fresh();
          if (!f) { HB.noteIn(form, "bad", "Could not read the current file. Your input is still here."); return; }
          if (f.values && Object.keys(was).every((k) => same(f.values[k], was[k]))) {
            form._base = form.dataset.hash = f.hash;
            HB.noteIn(form, "ok", "Reloaded the current file. Save again to apply your input on top of it.");
            submit.focus();
          } else stuck(form, o);
        },
        discard: () => discardForm(form, o),
      });
      if (!r.ok) {
        // custom.css: the note names its own file.
        if (o.file && r.note) r.note[1] = r.note[1].replace("the config file", o.file);
        return r;
      }
      const d = r.data;
      form._base = form.dataset.hash = d.hash || base;
      form._open = now;
      $$("input:not([type=checkbox]):not([type=radio]),textarea", form).forEach((c) => { c._was = c.value; c.defaultValue = c.value; });
      $$("input[type=checkbox]", form).forEach((c) => { c.defaultChecked = c.checked; });
      if (o.done) await o.done(d, now, changed);
      if (!o.file) $$('#set-banners [data-b="changed"]').forEach((b) => b.remove());
      const warn = (d.warnings || []).join(" ");
      return { ok: true, toast: d.unchanged ? "No changes" : (o.saved ? o.saved(d, changed) : `Saved${revNo(d)}`), note: warn && !o.ownWarnings ? ["warn", warn] : null, refocus };
    };
  }
  // A page form whose input cannot be saved onto the file as it is now.
  function stuck(form, o) {
    form.querySelector("[type=submit]").disabled = true;
    HB.noteIn(form, "bad", "The file changed in a way that touches these settings, so your input cannot be saved onto it. Copy what you need, then load the current values.", [{ label: "Discard my input and load the current values", cls: "primary", run: () => discardForm(form, o) }]);
  }
  async function discardForm(form, o) {
    const ok = o.load ? await o.load(form) : await refreshForms([form.id], true);
    if (!ok) { toast("Could not load the current file. Reload the page.", "bad"); return; }
    const f = $(form.id);
    HB.noteIn(f, "");
    const b = f.querySelector("[type=submit]"); if (b) b.disabled = false;
    toast("Showing the file as it is now", "info");
  }
  // Put fresh copies of page parts (by id) in from a same-origin GET of this
  // page: a form only when it holds no input (or `force`). Their handlers
  // are attached again by `PARTS[id]`.
  const PARTS = {};
  async function refreshForms(ids, force) {
    let r;
    try { r = await fetch(location.pathname, { credentials: "same-origin", cache: "no-store" }); } catch { return false; }
    if (r.status === 401) banner("expired");
    if (!r.ok) return false;
    const page = new DOMParser().parseFromString(await r.text(), "text/html");
    let done = false;
    for (const id of ids) {
      const a = $(id), b = page.getElementById(id);
      if (!a || !b || (!force && a._dirty && a._dirty())) continue;
      $$("[data-ready]", b).forEach((x) => { x.disabled = false; });
      a.replaceWith(b);
      if (PARTS[id]) PARTS[id](b);
      if (b.dataset.hash) S.hash = b.dataset.hash;
      done = true;
    }
    return done;
  }
  // A single control that saves when changed (owner decision Y21: the
  // default theme, Recording on / off). The base is the `data-hash` of the
  // element around it, taken at the change, before anything is awaited;
  // after the save it is what that save wrote. `o`: {path, body(v),
  // read(), revert(), saved(v, d), after()}.
  function saveOnChange(c, o) {
    if (!c) return;
    c.addEventListener("change", async () => {
      const box = c.closest("[data-hash]"), base = box.dataset.hash, v = o.read();
      // A switch waits busy and keeps its new look; a select is locked.
      const hold = (on) => (c.type === "checkbox" ? HB.hold(c, on) : (c.disabled = on));
      hold(true);
      const r = HB.result(await post(o.path, { base_hash: base, ...o.body(v) }));
      hold(false);
      if (!r.ok) {
        o.revert();
        toast(moved(r) ? "Not changed: the config file changed since this page was loaded. Nothing was written; the page now shows the current file." : why(r), "bad");
        if (moved(r) && o.after) o.after(true);
        return;
      }
      box.dataset.hash = r.data.hash;
      c.focus();
      toast(r.data.unchanged ? "No changes" : `${o.saved(v)}${revNo(r.data)}`);
      if (o.after) o.after(false);
    });
  }

  // ---- containers and groups: three lists of one file version ----
  const CT_LISTS = ["ex-list", "grp-list", "ov-list", "grp-names"];
  const ctAlso = (id) => CT_LISTS.filter((x) => x !== id);
  // A pattern as a test: `*` any run, `?` one character.
  const globRe = (p) => new RegExp("^" + p.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".") + "$");
  const EXCLUDE = {
    list: "ex-list", also: ctAlso("ex-list"), add: "ex-add", dlg: "dlg-exclude", menu: "m-ex", path: "docker", kind: "exclude", noun: "pattern",
    item: (v) => ({ pattern: v.pattern }),
    id: (key) => Number(keyId(key)),
    savedKey: (v, editKey, before) => editKey || `exclude:${before}`, // appended at the end
    movedKey: (key) => key,
    label: (v) => v.pattern,
    confirmText: (p) => ({ title: `Show containers matching “${p}” again?`, body: "The pattern is removed from the config file and the containers it hides are shown again. You can undo this from the message that follows.", ok: "Remove pattern" }),
    deleted: (p) => `Removed the pattern “${p}”`,
  };
  const GROUPS = {
    list: "grp-list", also: ctAlso("grp-list"), add: "grp-add", dlg: "dlg-group", menu: "m-grp", path: "groups", kind: "group", noun: "group",
    // Columns and layout are not in the form: an edit never sends them, so
    // they stay as they are in the file.
    item: (v, was) => onlyChanged({ name: v.name, title: v.title || null, icon: v.icon || null, collapsed: v.collapsed ? true : null, hide: v.hide ? true : null }, v, was, ["name"]),
    id: keyId,
    savedKey: (v) => `group:${v.name}`,
    movedKey: (key) => key, // keys are names
    label: (v) => v.name,
  };
  const OV_KEYS = ["name", "group", "icon", "url", "description", "hide", "order", "size"];
  // A field's text as the value the file gets (null: the key goes).
  const ovVal = (k, x) => (x === "" || x == null ? null : k === "hide" ? x === "true" : k === "order" ? Number(x) : x);
  const OVERRIDE = {
    list: "ov-list", also: ctAlso("ov-list"), dlg: "dlg-override", path: "containers/override", kind: "override", noun: "container",
    id: keyId,
    savedKey: (v, editKey) => editKey,
    label: (v, c) => v._c || (c && keyId(c.key)),
    // Only the fields whose text changed: set what has a value, clear what
    // was emptied (THE DIFF RULE). Nothing changed: nothing is sent.
    request: (v, edit, was, c) => {
      const set = {}, clear = [];
      for (const k of OV_KEYS) {
        if (sameVal(v[k], was[k])) continue;
        const x = ovVal(k, v[k]);
        if (x === null) clear.push(k); else set[k] = x;
      }
      if (!Object.keys(set).length && !clear.length) return null;
      const b = { names: [keyId(c.key)], clear };
      if (Object.keys(set).length) b.set = set;
      return b;
    },
    // "Remove overrides…" (in the dialog): the row stays, back to defaults.
    delBody: (key) => ({ names: [keyId(key)], remove: true }),
    keepRow: true,
    confirmText: (n) => ({ title: `Remove the overrides of “${n}”?`, body: "Its display name, group, icon, link and the rest go back to the defaults. You can undo this from the message that follows.", ok: "Remove overrides" }),
    deleted: (n) => `Removed the overrides of “${n}”`,
  };
  TABS.containers = {
    ssr: true,
    sel: new Set(),
    init() {
      listDialog($("dlg-exclude"), EXCLUDE); listActions(EXCLUDE);
      listDialog($("dlg-group"), GROUPS); listActions(GROUPS);
      listDialog($("dlg-override"), OVERRIDE); listActions(OVERRIDE);
      S.hash = $("ov-list").dataset.hash;
      S.after = () => this.apply();
      // The pattern dialog: how many containers the pattern hides, live.
      const dx = $("dlg-exclude"), pin = $("ex-pattern"), out = $("ex-match");
      let names = [];
      try { names = JSON.parse(out.dataset.names || "[]"); } catch {}
      const count = () => {
        const p = pin.value.trim();
        if (!p) { out.textContent = ""; return; }
        const re = globRe(p), hit = names.filter((n) => re.test(n));
        out.textContent = !hit.length ? "Matches no container now." : `Hides ${hit.length} container${hit.length > 1 ? "s" : ""} now: ${hit.slice(0, 5).join(", ")}${hit.length > 5 ? ` and ${hit.length - 5} more` : ""}.`;
      };
      pin.addEventListener("input", count);
      dx.addEventListener("hb-open", () => setTimeout(count, 0));
      // The override dialog: "Remove overrides…" only when there are some.
      const dov = $("dlg-override");
      dov.addEventListener("hb-open", (e) => {
        const v = e.detail.values || {};
        dov.querySelector("[data-act=dlg-delete]").hidden = !OV_KEYS.some((k) => v[k]);
      });
      // Search, "Only with overrides", selection: applied to the rows the
      // server rendered (and again after every refresh). The dialog does not
      // depend on them, so filtering never closes it (SET-34).
      $("ov-q").addEventListener("input", () => this.apply());
      $("ov-only").addEventListener("change", () => this.apply());
      main.addEventListener("change", (e) => {
        const t = e.target;
        if (t.id === "ov-all") { for (const tr of this.rows()) if (!tr.hidden) t.checked ? this.sel.add(this.nameOf(tr)) : this.sel.delete(this.nameOf(tr)); this.apply(); }
        else if (t.matches("[data-sel]")) { t.checked ? this.sel.add(t.dataset.sel) : this.sel.delete(t.dataset.sel); this.apply(); }
      });
      $("ov-bulk").addEventListener("click", (e) => { const b = e.target.closest("[data-bulk]"); if (b) this.bulk(b.dataset.bulk, b); });
      this.apply();
      // `?name=<container>`, the older link from a container page: the
      // override dialog, as `?edit=override:<container>` opens it.
      const q = new URLSearchParams(location.search), n = q.get("name");
      if (n && !q.has("edit") && !q.has("add")) {
        try { history.replaceState(null, "", location.pathname + "?edit=" + encodeURIComponent("override:" + n)); } catch {}
        setTimeout(() => {
          if (HB.current()) return;
          const t = $$("#ov-list [data-dlg][data-key]").find((a) => a.dataset.key === "override:" + n);
          let v = null; try { v = JSON.parse(t.dataset.item); } catch {}
          if (t) HB.open(t.dataset.dlg, { trigger: t, values: v, landed: true });
        }, 0);
      }
    },
    rows: () => $$("#ov-list tr[data-key]"),
    nameOf: (tr) => keyId(tr.dataset.key),
    apply() {
      const q = $("ov-q").value.trim().toLowerCase(), only = $("ov-only").checked;
      const rows = this.rows(), names = new Set(rows.map((tr) => this.nameOf(tr)));
      for (const n of Array.from(this.sel)) if (!names.has(n)) this.sel.delete(n);
      let shown = 0, picked = 0;
      for (const tr of rows) {
        const on = (!only || tr.dataset.ov) && (!q || tr.dataset.q.includes(q));
        tr.hidden = !on;
        const cb = tr.querySelector("[data-sel]");
        if (cb) cb.checked = this.sel.has(cb.dataset.sel);
        if (on) { shown++; if (cb && cb.checked) picked++; }
      }
      $("ov-count").textContent = shown === rows.length ? String(rows.length) : `${shown} / ${rows.length}`;
      const all = $("ov-all");
      if (all) { all.checked = shown > 0 && picked === shown; all.indeterminate = picked > 0 && picked < shown; }
      let none = $("ov-none");
      if (!shown && rows.length) { if (!none) { none = el("p", { id: "ov-none", class: "muted none", text: "No container matches." }); $("ov-list").after(none); } } else if (none) none.remove();
      $("ov-bulk").hidden = !this.sel.size;
      $("ov-bulk-n").textContent = `${this.sel.size} selected`;
    },
    // The bulk bar: the selection and the base are taken when the action
    // starts (the base-hash rule); Remove asks first. Each action is one
    // revision, with Undo.
    async bulk(act, b) {
      const box = $("ov-list"), base = box.dataset.hash, names = Array.from(this.sel);
      if (act === "none") { this.sel.clear(); this.apply(); return; }
      if (!names.length || b.getAttribute("aria-busy") === "true") return;
      const many = `${names.length} container${names.length > 1 ? "s" : ""}`;
      let what, said;
      if (act === "group") {
        const g = $("ov-bulk-group").value.trim();
        if (!g) { $("ov-bulk-group").focus(); toast("Type a group name first", "info"); return; }
        what = { set: { group: g } }; said = `Moved ${many} to the group “${g}”`;
      } else if (act === "hide") { what = { set: { hide: true } }; said = `Hid ${many}`; }
      else if (act === "show") { what = { clear: ["hide"] }; said = `${many[0].toUpperCase() + many.slice(1)} shown again`; }
      else if (act === "remove") {
        const ok = await HB.confirm({ title: `Remove the overrides of ${many}?`, body: `${names.slice(0, 5).join(", ")}${names.length > 5 ? ` and ${names.length - 5} more` : ""}: display names, groups, icons and the rest go back to the defaults. You can undo this from the message that follows.`, ok: "Remove overrides" });
        if (!ok) return;
        what = { remove: true }; said = `Removed the overrides of ${many}`;
      } else return;
      b.setAttribute("aria-busy", "true");
      const r = HB.result(await post("containers/override", { base_hash: base, names, ...what }));
      b.removeAttribute("aria-busy");
      if (!r.ok) {
        toast(moved(r) ? "Not changed: the config file changed since this list was loaded. Nothing was written; the list now shows the current file." : why(r), "bad");
        refreshList(CT_LISTS);
        return;
      }
      if (act === "remove") this.sel.clear();
      await refreshList(CT_LISTS);
      for (const n of names) { const tr = $$("#ov-list tr[data-key]").find((x) => this.nameOf(x) === n); if (tr && !tr.hidden) HB.flash(tr); }
      const undo = undoAction(base, r.data, () => refreshList(CT_LISTS), "Undone");
      HB.toast(r.data.unchanged ? "No changes" : `${said}${revNo(r.data)}`, undo ? { action: undo } : {});
    },
    load() { return refreshList(CT_LISTS); },
  };

  // ---- general ----
  const GEN_TEXT = ["title", "greeting", "search_url"];
  const generalForm = (f) => pageForm(f, {
    path: "general",
    // Only what changed; an emptied text removes its key (the default).
    body: (now, changed) => ({ page: Object.fromEntries(changed.map((k) => [k, GEN_TEXT.includes(k) ? now[k] || null : now[k]])) }),
    fresh: async () => {
      const r = await call("GET", "general");
      if (r.status !== 200) return null;
      const p = r.data.page || {}, e = r.data.effective || {};
      return { hash: r.data.hash, values: { title: p.title ?? "", greeting: p.greeting ?? "", search_url: p.search_url ?? "", show_stopped: !!e.show_stopped, show_stats: !!e.show_stats } };
    },
    // The chrome follows a new title (SET-25).
    done: (d, now, changed) => {
      if (!changed.includes("title")) return;
      const site = now.title || ($("gen-title").placeholder || "");
      const b = doc.querySelector(".brand-site"), old = b && b.textContent;
      if (b) { b.textContent = site; b.closest("a").title = "Hobnail · " + site; }
      if (old) doc.title = doc.title.replace(` · ${old} · `, ` · ${site} · `);
    },
  });
  TABS.general = {
    init() { generalForm($("f-general")); PARTS["f-general"] = generalForm; const f = $("f-general"); if (f) S.hash = f.dataset.hash; },
    load() { return refreshForms(["f-general"]); },
  };

  // ---- appearance: the default theme saves when changed; custom.css ----
  const localTheme = () => { try { return localStorage.getItem("rh-theme"); } catch { return null; } };
  const themeBox = (box) => {
    const s = box && $("th-theme");
    if (!s) return;
    $("th-local").hidden = !localTheme();
    saveOnChange(s, {
      path: "general",
      read: () => s.value,
      body: (v) => ({ page: { theme: v || null } }),
      revert: () => { s.value = s.dataset.was; },
      saved: (v) => {
        s.dataset.was = v;
        if (!localTheme()) doc.documentElement.dataset.theme = v || "auto";
        return `Default theme: ${s.selectedOptions[0].textContent}`;
      },
      after: (stale) => { if (stale) refreshForms(["th-box"], true); },
    });
  };
  // custom.css as a note on the form: the retired token names it sets.
  function cssWarnings(list) {
    const box = $("css-warn");
    if (!box) return;
    box.replaceChildren(...(list.length ? [el("div", { class: "note warn" }, icon("alert"), el("div", null, el("strong", { text: "Old token names: " }), list.length === 1 ? list[0] : el("ul", null, list.map((m) => el("li", { text: m })))))] : []));
  }
  TABS.appearance = {
    init() {
      themeBox($("th-box")); PARTS["th-box"] = themeBox;
      const b = $("th-box"); if (b) S.hash = b.dataset.hash;
      onAct("th-forget", () => {
        try { localStorage.removeItem("rh-theme"); } catch {}
        doc.documentElement.dataset.theme = $("th-theme").value || "auto";
        $("th-local").hidden = true;
      });
      const fc = $("f-css"), ta = $("css-text");
      if (!fc) return;
      const max = Number($("css-size").dataset.max) || 65536;
      const size = () => { const n = new Blob([ta.value]).size, s = $("css-size"); s.textContent = `${bytes(n)} of ${bytes(max)}`; s.classList.toggle("over", n > max); };
      // Apply the editor to the page (or `null`: back to the saved custom.css).
      const preview = (text) => {
        let st = $("rh-preview");
        const link = doc.querySelector('link[href^="/custom.css"]');
        if (text == null) { if (st) st.remove(); if (link) link.disabled = false; return; }
        if (!st) { st = el("style", { id: "rh-preview" }); doc.head.append(st); }
        st.textContent = text;
        if (link) link.disabled = true;
      };
      size();
      ta.addEventListener("input", () => { size(); if ($("css-live").checked) preview(ta.value); });
      $("css-live").addEventListener("change", (e) => preview(e.target.checked ? ta.value : null));
      tabIndent(ta);
      pageForm(fc, {
        path: CSS_PATH,
        file: "custom.css",
        // custom.css is its own file: its base is its own hash (data-hash).
        body: (now) => ({ css: now.css }),
        check: (now) => (new Blob([now.css]).size > max ? { css: `custom.css is limited to ${max / 1024} KiB.` } : null),
        fresh: async () => { const r = await call("GET", "appearance"); return r.status === 200 ? { hash: r.data.hash, values: { css: r.data.css } } : null; },
        load: async (f) => {
          const r = await call("GET", "appearance");
          if (r.status !== 200) return false;
          ta.value = r.data.css; f.dataset.hash = r.data.hash; f._init(); size(); preview($("css-live").checked ? ta.value : null);
          return true;
        },
        ownWarnings: true,
        done: (d) => {
          cssWarnings(d.warnings || []);
          // The saved file applies from now on, preview or not.
          const link = doc.querySelector('link[href^="/custom.css"]');
          if (link) link.href = "/custom.css?v=" + Date.now();
        },
        saved: () => "Saved custom.css",
      });
      // The page form's base is custom.css, not the config file.
      onAct("css-revert", () => { ta.value = fc._open.css; size(); preview($("css-live").checked ? ta.value : null); HB.noteIn(fc, ""); toast("Back to the saved custom.css", "info"); });
    },
    load() { return refreshForms(["th-box"]); },
  };
  // Tab indents in a code editor until Esc (then Tab moves focus again).
  function tabIndent(ta) {
    let on = false;
    ta.addEventListener("focus", () => { on = true; });
    ta.addEventListener("keydown", (e) => {
      if (e.key === "Tab" && !e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey && on) {
        e.preventDefault(); ta.setRangeText("  ", ta.selectionStart, ta.selectionEnd, "end"); ta.dispatchEvent(new Event("input", { bubbles: true }));
      }
      if (e.key === "Escape") on = false;
    });
  }

  // ---- history and data ----
  // Export is a plain download: say that it started (SET-62).
  doc.addEventListener("click", (e) => { if (e.target.closest && e.target.closest("#db-export")) toast("Export started. Your browser saves the file (JSON).", "info"); });
  const recBox = (box) => {
    const s = box && $("hi-enabled");
    if (!s) return;
    saveOnChange(s, {
      path: "history",
      read: () => s.checked,
      body: (on) => ({ history: { enabled: on } }),
      revert: () => { s.checked = !s.checked; },
      saved: (on) => `Recording ${on ? "on" : "off"}`,
      // The interval form shows the file this switch just wrote, unless it
      // holds input (then its own save meets the 409 and can retarget).
      after: (stale) => { if (stale) refreshForms(["hi-rec"], true); refreshForms(["f-history"]); },
    });
  };
  const historyForm = (f) => pageForm(f, {
    path: "history",
    body: (now) => ({ history: { sample_interval: now.sample_interval || null } }),
    fresh: async () => {
      const r = await call("GET", "history");
      if (r.status !== 200) return null;
      const v = (r.data.history || {}).sample_interval;
      return { hash: r.data.hash, values: { sample_interval: v == null ? "" : String(v) } };
    },
    saved: (d) => `Saved the sample interval${revNo(d)}`,
  });
  TABS.history = {
    init() {
      recBox($("hi-rec")); PARTS["hi-rec"] = recBox;
      historyForm($("f-history")); PARTS["f-history"] = historyForm;
      const f = $("f-history"); if (f) S.hash = f.dataset.hash;
      // Clear history: the one confirm, with the typed phrase. It deletes
      // data that cannot be restored, so nothing is sent before the phrase.
      const b = $("clr-btn");
      b.addEventListener("click", async () => {
        const ok = await HB.confirm({ title: "Clear all history?", body: "Every recorded sample and event is deleted. Config revisions and the login are kept. This cannot be undone.", ok: "Clear history", typed: "clear history" });
        if (!ok) return;
        b.setAttribute("aria-busy", "true"); b.disabled = true;
        const r = HB.result(await call("POST", "history/clear", {}));
        b.removeAttribute("aria-busy"); b.disabled = false;
        if (!r.ok) { toast(why(r) || "Not cleared.", "bad"); return; }
        await refreshList("db-stats");
        toast("History cleared");
        b.focus();
      });
    },
    load() { return refreshForms(["hi-rec", "f-history"]); },
  };

  // ---- config file: the raw editor and the revisions ----
  TABS.config = {
    seq: 0,
    init() {
      const f = $("f-raw"), ta = $("raw-text");
      if (!f) return;
      S.hash = f.dataset.hash;
      const raw = this;
      pageForm(f, {
        path: "config",
        body: (now) => ({ text: now.text }),
        // The whole file is the entry: after a 409 it is never the same.
        fresh: async () => ({ hash: "", values: null }),
        load: (form) => raw.reload(form),
        done: async () => { this.status({ ok: true }); this.validate(); await refreshList("rev-list"); },
        saved: (d) => `Saved config.toml${revNo(d)}`,
        // Saved from the editor (Ctrl+S): back to it, caret where it was.
        refocus: () => { const r = raw.fromEditor; raw.fromEditor = false; return r ? ta : null; },
      });
      ta.addEventListener("input", () => {
        this.gutter();
        clearTimeout(this.t);
        this.t = setTimeout(() => this.validate(), 400);
      });
      ta.addEventListener("scroll", () => { $("raw-gutter").scrollTop = ta.scrollTop; this.mark(); });
      tabIndent(ta);
      ta.addEventListener("keydown", (e) => { if ((e.ctrlKey || e.metaKey) && (e.key === "s" || e.key === "S")) { e.preventDefault(); this.fromEditor = true; f.requestSubmit(); } });
      onAct("raw-revert", () => { ta.value = f._open.text; this.gutter(); this.validate(); HB.noteIn(f, ""); toast("Back to the file as it was loaded", "info"); });
      onAct("raw-goto", () => this.goto(this.err && this.err.line));
      this.gutter();
      this.validate();
      // Revisions: View and Diff in a dialog, Restore after the confirm.
      $("rev-sec").addEventListener("click", (e) => {
        const b = e.target.closest("[data-rev-act]");
        if (!b || b.disabled) return;
        const tr = b.closest("tr[data-rev]");
        const r = { id: Number(tr.dataset.rev), ts: Number(tr.dataset.ts), note: tr.dataset.note };
        if (b.dataset.revAct === "restore") this.restore(r, b);
        else this.open(r, b.dataset.revAct, b);
      });
      const dr = $("dlg-rev");
      dr.querySelector("[data-act=rev-restore]").addEventListener("click", () => this.restore(dr._rev, dr.querySelector("[data-act=rev-restore]")));
      dr.querySelector("[data-act=rev-open]").addEventListener("click", async () => {
        const text = dr._text;
        if (text == null) return;
        if (f._dirty() && !(await HB.confirm({ title: "Replace your unsaved edits?", body: "The editor gets this revision's text; your edits there are lost. Nothing is saved until you press Save.", ok: "Replace" }))) return;
        HB.close(true);
        ta.value = text; this.gutter(); this.validate(); ta.focus();
      });
    },
    // The file as it is now, into the editor (after "load the current values").
    // `clean`: an outside change, applied only to an editor without input.
    // Input typed while the file is fetched wins: the editor keeps it and
    // the banner says the file changed (its save then meets the 409).
    async reload(form, clean) {
      const before = $("raw-text").value;
      const r = await call("GET", "config");
      if (r.status !== 200) return false;
      if (clean && ($("raw-text").value !== before || form._dirty())) { banner("changed"); return false; }
      // The caret and the scroll stay where they were (setting the value
      // would put the caret at the end, so typing then lands there).
      const ta = $("raw-text"), s0 = ta.selectionStart, s1 = ta.selectionEnd, top = ta.scrollTop;
      ta.value = r.data.text;
      const n = ta.value.length;
      ta.setSelectionRange(Math.min(s0, n), Math.min(s1, n));
      ta.scrollTop = top; form.dataset.hash = r.data.hash; form._init(); S.hash = r.data.hash;
      this.gutter(); this.validate();
      return true;
    },
    gutter() {
      const ta = $("raw-text"), n = ta.value.split("\n").length;
      if (n !== this.lines) { this.lines = n; $("raw-gutter").textContent = Array.from({ length: n }, (_, i) => i + 1).join("\n"); }
      $("raw-gutter").scrollTop = ta.scrollTop;
      this.mark();
    },
    mark() {
      const hl = $("raw-hl"), ta = $("raw-text"), line = this.err && this.err.line;
      if (!line) { hl.hidden = true; return; }
      const lh = parseFloat(getComputedStyle(ta).lineHeight) || 20, pad = parseFloat(getComputedStyle(ta).paddingTop) || 0;
      const top = pad + (line - 1) * lh - ta.scrollTop;
      hl.style.top = top + "px"; hl.style.height = lh + "px";
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
      const n = ++this.seq, f = $("f-raw");
      const res = await call("POST", "config/validate", { text: $("raw-text").value });
      if (n !== this.seq) return;
      if (res.status === 200) {
        this.status(res.data);
        if (res.data.hash && res.data.hash !== f._base && f._dirty()) banner("changed");
      } else if (res.status === 401) banner("expired");
      else this.status({ ok: false, error: (res.data && res.data.error) || `HTTP ${res.status}` });
    },
    status(v) {
      const box = $("raw-status"), go = main.querySelector("[data-act=raw-goto]");
      this.err = v.ok ? null : v;
      go.hidden = !v.line;
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
    // View / Diff: the revision dialog (Esc, ×, Close and the backdrop close it).
    async open(r, kind, trigger) {
      const dr = $("dlg-rev"), body = $("dlg-rev-b");
      dr._rev = r; dr._text = null;
      $("dlg-rev-t").textContent = kind === "view" ? `Revision #${r.id} · ${fmtTime(r.ts)}${r.note ? " · " + r.note : ""}` : `What restoring #${r.id} would change`;
      dr.querySelector("[data-act=rev-open]").hidden = kind !== "view";
      body.replaceChildren(muted("Loading…"));
      body.setAttribute("aria-busy", "true");
      HB.open(dr, { trigger });
      const d = await call("GET", kind === "view" ? `revision/${r.id}` : `revision/${r.id}/diff`);
      body.removeAttribute("aria-busy");
      if (!HB.current() || HB.current().dlg !== dr || dr._rev !== r) return;
      if (d.status !== 200) { body.replaceChildren(el("p", { class: "err-line", text: (d.data && d.data.error) || `HTTP ${d.status}` })); if (d.status === 401) banner("expired"); return; }
      if (kind === "view") { dr._text = d.data.text; body.replaceChildren(el("pre", { class: "code-view", tabindex: "0", text: d.data.text })); return; }
      const x = d.data, hunks = x.hunks || [];
      const out = [el("p", { class: "diff-sum" }, `From the current file to revision #${r.id}: `, el("span", { class: "d-add", text: `+${x.added}` }), " ", el("span", { class: "d-del", text: `−${x.removed}` }), x.approximate ? muted(" (approximate)") : null)];
      if (!hunks.length) out.push(el("p", { class: "muted", text: "Identical to the current file." }));
      else out.push(el("div", { class: "diff", tabindex: "0" }, hunks.map((h) => [
        el("div", { class: "d-hunk", text: `@@ -${h.old_start},${h.old_lines} +${h.new_start},${h.new_lines} @@` }),
        ...h.lines.map((l) => el("div", { class: l.op === "+" ? "d-l d-add" : l.op === "-" ? "d-l d-del" : "d-l" }, el("span", { class: "d-op", "aria-hidden": "true", text: l.op }), el("span", { class: "sr", text: l.op === "+" ? "added: " : l.op === "-" ? "removed: " : "" }), el("span", { text: l.text || " " }))),
      ])));
      body.replaceChildren(...out);
    },
    // Restore: based on the file the revision list was rendered against,
    // taken now, before the confirm is awaited (the base-hash rule).
    async restore(r, b) {
      const f = $("f-raw"), base = $("rev-list").dataset.hash, edits = f && f._dirty();
      const ok = await HB.confirm({ title: `Restore revision #${r.id}?`, body: `${fmtTime(r.ts)}${r.note ? ", " + r.note : ""}. It is written as the config file; the current file stays in the revisions.${edits ? " Your unsaved edits in the editor are replaced." : ""}`, ok: "Restore" });
      if (!ok) return;
      if (HB.current() && HB.current().dlg.id === "dlg-rev") HB.close(true);
      const res = await post(`revision/${r.id}/restore`, { base_hash: base });
      const x = HB.result(res);
      if (!x.ok) {
        toast(moved(x) ? `Not restored: the config file changed since this list was loaded. Nothing was written; the list now shows the current file.` : why(x), "bad");
        await refreshList("rev-list");
        // Focus stays on that revision's Restore when the refresh replaced its row.
        const nb = b && !b.isConnected && $("rev-list").querySelector(`tr[data-rev="${r.id}"] [data-rev-act=restore]`);
        if (nb) nb.focus();
        return;
      }
      if (f) await this.reload(f);
      await refreshList("rev-list", x.data.revision != null ? `rev:${x.data.revision}` : null);
      toast(x.data.unchanged ? `Revision #${r.id} is the file as it is now: nothing changed` : `Restored revision #${r.id}${revNo(x.data)}`);
      if (b && b.isConnected) b.focus();
    },
    // An outside change: the revisions follow; the editor only without edits.
    async load() {
      const f = $("f-raw");
      if (f && !f._dirty() && !(await this.reload(f, true))) { await refreshList("rev-list"); return false; }
      return refreshList("rev-list");
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
        const b = e.currentTarget;
        const ok = await HB.confirm({ title: "Sign out every other session?", body: "Every other browser and device has to sign in again. This one stays signed in.", ok: "Sign out others" });
        if (!ok) return;
        b.disabled = true; b.setAttribute("aria-busy", "true");
        const r = HB.result(await call("POST", "security/sign-out-others", {}));
        b.disabled = false; b.removeAttribute("aria-busy");
        toast(r.ok ? "Every other session was signed out" : why(r), r.ok ? "ok" : "bad");
        b.focus();
      });
    },
  };

  // The Overview, signed in (ui::settings::embed): the bookmark and widget
  // dialogs behind its "Add" links, no list. After a save, and after a
  // change elsewhere, its Bookmarks and Widgets panels are swapped in from
  // the page itself (never over a dialog: checkChanged waits for it).
  // An outside change reloads the page instead (checkChanged); `load` is
  // this page's own save, and the reload after a 409 (freshBase). Focus,
  // and an open dialog's trigger, move to the same control in the new
  // parts (the Add link, a bookmark) instead of falling to <body>.
  TABS["bookmarks widgets"] = {
    init() {
      TABS.bookmarks.init(); TABS.widgets.init();
      const h = doc.querySelector(".head-add[data-hash]"); if (h) S.hash = h.dataset.hash;
      S.saved = () => this.load();
    },
    async load() {
      const ids = ["ov-marks", "ov-w"], cc = HB.current(), act = doc.activeElement;
      const within = (x) => x && x.nodeType === 1 && ids.some((id) => $(id) && $(id).contains(x));
      const sel = (x) => {
        if (!within(x)) return null;
        if (x.id) return "#" + CSS.escape(x.id);
        if (x.dataset.dlg) return `[data-dlg="${CSS.escape(x.dataset.dlg)}"]`;
        const h = x.getAttribute("href");
        return h ? `a[href="${CSS.escape(h)}"]` : null;
      };
      const ts = cc && sel(cc.trigger), as = sel(act);
      const ok = await refreshForms(ids, true);
      const find = (s) => s && ids.map((id) => $(id) && $(id).querySelector(s)).find(Boolean);
      if (cc && ts && cc.trigger && !cc.trigger.isConnected) cc.trigger = find(ts) || cc.trigger;
      if (as && !act.isConnected) { const n = find(as); if (n) n.focus({ preventScroll: true }); }
      const h = doc.querySelector(".head-add[data-hash]"); if (h) S.hash = h.dataset.hash;
      return ok;
    },
  };

  const T = TABS[TAB];
  if (T) T.init();
  };
  // This deferred script runs before app.js: wait for DOMContentLoaded, which
  // comes after every deferred script, also when app.js failed to load.
  if (window.HB) start();
  else document.addEventListener("DOMContentLoaded", start, { once: true });
})();
