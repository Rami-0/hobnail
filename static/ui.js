// hobnail component layer (DESIGN.md §11.4 to §11.9) as window.HB: in-page
// tabs, field validation, the add / edit dialog (URL is the dialog, unsaved
// guard, busy state, server errors on their field), toast, confirm, menu
// (popover) and list flashes. Lifted from the reviewed prototype; served as
// the first part of /static/app.js. No framework; never inserts text as
// HTML. Everything here is an enhancement: without it, every trigger is a
// link to a server-rendered page with the same form.
(() => {
  "use strict";
  const doc = document, root = doc.documentElement;
  const $$ = (s, el) => Array.from((el || doc).querySelectorAll(s));
  const calm = matchMedia("(prefers-reduced-motion: reduce)");
  const HB = (window.HB = {});
  root.classList.add("js");
  function el(tag, cls, text) { const e = doc.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; }
  function icon(name) {
    const s = doc.createElementNS("http://www.w3.org/2000/svg", "svg"), u = doc.createElementNS(s.namespaceURI, "use");
    s.setAttribute("class", "i"); s.setAttribute("aria-hidden", "true"); u.setAttribute("href", "#i-" + name); s.append(u); return s;
  }
  HB.el = el; HB.icon = icon;

  // ---- in-page tabs (role=tablist); page tabs are plain links ----
  function select(tab) {
    $$("[role=tab]", tab.closest("[role=tablist]")).forEach((t) => {
      const on = t === tab;
      t.setAttribute("aria-selected", String(on)); t.tabIndex = on ? 0 : -1;
      const p = doc.getElementById(t.getAttribute("aria-controls")); if (p) p.hidden = !on;
    });
  }
  doc.addEventListener("click", (e) => { const t = e.target.closest && e.target.closest("[role=tab]"); if (t) select(t); });
  doc.addEventListener("keydown", (e) => {
    const t = e.target.closest && e.target.closest("[role=tab]");
    if (!t || !/^(ArrowLeft|ArrowRight|Home|End)$/.test(e.key)) return;
    const l = $$("[role=tab]", t.closest("[role=tablist]")), i = l.indexOf(t);
    const n = l[e.key === "Home" ? 0 : e.key === "End" ? l.length - 1 : (i + (e.key === "ArrowRight" ? 1 : -1) + l.length) % l.length];
    e.preventDefault(); select(n); n.focus();
  });

  // ---- validation: every problem at once, each on its field ----
  const RULES = {
    http: [/^https?:\/\/[^\s/?#]+[^\s]*$/i, "Use a link that starts with http:// or https://."],
    duration: [/^\d+(ms|s|m|h|d)$/, "Use a duration like 30s, 5m or 2h."],
    env: [/^[A-Z_][A-Z0-9_]*$/, "Capital letters, digits and _ only, like RH_WEBHOOK_URL."],
    id: [/^[a-z0-9][a-z0-9_-]*$/, "Lowercase letters, digits, - and _ only."],
  };
  // Only disabled controls are skipped (fields of another type are disabled);
  // a field in a closed <details> or a hidden tab panel is still checked.
  function problem(c) {
    if (c.disabled || !c.name || c.type === "hidden") return "";
    if (c.type === "checkbox" || c.type === "radio") return c.required && !c.form.querySelector(`[name="${CSS.escape(c.name)}"]:checked`) ? "Pick one." : "";
    const v = c.type === "password" ? c.value : c.value.trim();
    if (!v) return c.required ? "Required." : "";
    if (c.maxLength > 0 && v.length > c.maxLength) return `At most ${c.maxLength} characters.`;
    if (c.type === "number") {
      const n = Number(v), lo = c.min !== "" ? +c.min : -Infinity, hi = c.max !== "" ? +c.max : Infinity;
      const range = c.min !== "" && c.max !== "" ? ` from ${c.min} to ${c.max}` : "";
      if (!isFinite(n) || (c.step !== "any" && !Number.isInteger(n))) return `Use a whole number${range}.`;
      if (n < lo || n > hi) return `Use a number${range || (n < lo ? " of " + lo + " or more" : " up to " + hi)}.`;
    }
    // A value an edit dialog (or a page form, `data-hash`) was opened with is
    // left as it is in the file: its format rule is not applied again (a
    // hand-written `url = "/files"` stays editable; only changed fields are
    // sent, so the server never checks it either).
    if (c._was !== undefined && c.value === c._was && c.closest(".dlg[data-mode=edit],form[data-hash]")) return "";
    const r = RULES[c.dataset.v];
    if (r && !r[0].test(v)) return c.dataset.msg || r[1];
    if (c.pattern && !new RegExp(`^(?:${c.pattern})$`).test(v)) return c.dataset.msg || "Check the format.";
    return "";
  }
  function setErr(c, msg) {
    const f = c.closest(".fld"), e = f && f.querySelector(".fld-err");
    if (!e) return;
    e.textContent = msg; e.hidden = !msg;
    msg ? c.setAttribute("aria-invalid", "true") : c.removeAttribute("aria-invalid");
  }
  // Make a field visible before focusing it: open its <details>, select its tab.
  function reveal(c) {
    for (let d = c.closest("details"); d; d = d.parentElement.closest("details")) d.open = true;
    const p = c.closest("[role=tabpanel]");
    if (p && p.hidden) { const t = doc.querySelector(`[role=tab][aria-controls="${CSS.escape(p.id)}"]`); t && select(t); }
    c.focus();
  }
  const fields = (form) => Array.from(form.elements).filter((c) => c.name && !/^(BUTTON|FIELDSET)$/.test(c.tagName));
  // `form._check(values)` adds checks across fields ({name: message}), e.g.
  // "Does not match the new password."; they show with the others.
  HB.validate = (form) => {
    let first = null, n = 0;
    const extra = (form._check && form._check(values(form))) || {};
    for (const c of fields(form)) { const m = problem(c) || (!c.disabled && extra[c.name]) || ""; setErr(c, m); if (m) { n++; first = first || c; } }
    return { n, first };
  };
  doc.addEventListener("input", (e) => { const c = e.target; if (c.form && c.form.hasAttribute("data-hb") && c.getAttribute("aria-invalid") && !problem(c)) setErr(c, ""); });
  doc.addEventListener("change", (e) => { const c = e.target; if (c.form && c.form.hasAttribute("data-hb")) setErr(c, problem(c)); });

  // ---- form values (passwords are never trimmed) ----
  function values(form) {
    const o = {};
    for (const c of fields(form)) {
      if (c.disabled) continue;
      if (c.type === "radio") { if (c.checked) o[c.name] = c.value; }
      else if (c.type === "checkbox") { if (c.dataset.t === "set") (o[c.name] = o[c.name] || []), c.checked && o[c.name].push(c.value); else o[c.name] = c.checked; }
      else o[c.name] = c.type === "password" ? c.value : c.value.trim();
    }
    return o;
  }
  function fill(form, v) {
    // A control a page script locked for one entry (`data-locked`, e.g. the
    // type of a channel bound to its destination) is free again for the next.
    $$("[data-locked]", form).forEach((c) => { c.disabled = false; c.removeAttribute("data-locked"); });
    for (const c of fields(form)) {
      const x = v ? v[c.name] : undefined;
      if (c.type === "radio") c.checked = x != null ? c.value === String(x) : c.defaultChecked;
      else if (c.type === "checkbox") c.checked = Array.isArray(x) ? x.includes(c.value) : x != null ? !!x : c.defaultChecked;
      else if (c.tagName === "SELECT") { const o = Array.from(c.options); c.value = x != null && o.some((y) => y.value === String(x)) ? String(x) : (o.find((y) => y.defaultSelected) || o[0] || {}).value || ""; }
      else { c.value = x != null ? String(x) : c.defaultValue; c._was = c.value; }
      setErr(c, "");
    }
    form.dispatchEvent(new Event("hb-fill"));
  }
  HB.values = values; HB.fill = fill;
  // An error on one field, shown and revealed (its tab selected, its
  // <details> opened) as a save's would be; false when the form has no such field.
  HB.fieldError = (form, name, msg) => {
    const x = form.elements.namedItem(name), c = x && !x.tagName && x.length ? x[0] : x;
    if (!c || c.disabled) return false;
    setErr(c, msg); reveal(c);
    return true;
  };
  // Fields that belong to one choice (data-for="name:a b", default name "type").
  function byType(form) {
    $$("[data-for]", form).forEach((w) => {
      const [k, l] = w.dataset.for.includes(":") ? w.dataset.for.split(":") : ["type", w.dataset.for];
      const t = form.querySelector(`[name="${CSS.escape(k)}"]:checked, select[name="${CSS.escape(k)}"]`);
      if (!t) return;
      const on = l.split(" ").includes(t.value) && !w.parentElement.closest("[data-for][hidden]");
      w.hidden = !on; $$("input,select,textarea", w).forEach((c) => { c.disabled = !on || c.closest("[data-for][hidden]") !== null; if (c.disabled) setErr(c, ""); });
    });
    // A chosen option that just disappeared falls back to the first one left.
    const gone = form.querySelector("input[type=radio]:checked:disabled"), alt = gone && form.querySelector(`input[name="${CSS.escape(gone.name)}"]:not(:disabled)`);
    if (alt) { alt.checked = true; byType(form); }
  }
  doc.addEventListener("change", (e) => { const t = e.target; if ((t.type === "radio" || t.tagName === "SELECT") && t.form && t.form.hasAttribute("data-hb")) byType(t.form); });
  doc.addEventListener("hb-fill", (e) => byType(e.target), true);

  // ---- dialog: opens from what was clicked, Esc / backdrop / Cancel ask before dropping input ----
  let cur = null; // { dlg, form, trigger, key, snap, pushed, landed, busy, by, last }
  const snapOf = (form) => JSON.stringify(values(form));
  const dirty = () => cur && !cur.busy && snapOf(cur.form) !== cur.snap;
  const part = (dlg, sel) => dlg.querySelector(sel);
  const others = () => $$("dialog[open]").some((d) => !cur || d !== cur.dlg);
  function note(dlg, kind, text, acts) {
    const n = part(dlg, ".dlg-note");
    if (!n) return null;
    n.hidden = !text; n.className = "note dlg-note " + (kind === "lock" ? "warn" : kind || "");
    n.replaceChildren();
    if (!text) return null;
    const box = el("div");
    box.append(el("span", "", text));
    if (acts && acts.length) { const a = el("div", "acts"); acts.forEach((x) => { const b = el("button", "btn sm " + (x.cls || ""), x.label); b.type = "button"; b.onclick = x.run; a.append(b); }); box.append(a); }
    n.append(icon({ bad: "alert", ok: "check", lock: "lock", warn: "alert" }[kind] || "info"), box);
    return n;
  }
  function msg(dlg, text, bad) { const m = part(dlg, ".dlg-msg"); if (m) { m.textContent = text || ""; m.classList.toggle("bad", !!bad); } }
  function guard(on) {
    const g = part(cur.dlg, ".dlg-guard"), m = part(cur.dlg, ".dlg-main");
    if (!g || !m) return;
    g.hidden = !on; m.hidden = on;
    if (on) g.querySelector("[data-keep]").focus(); else (cur.last || firstField(cur.form) || cur.dlg).focus();
  }
  function firstField(form) { return fields(form).find((c) => !c.disabled && !c.readOnly && c.type !== "hidden" && !c.closest("[hidden]") && c.offsetParent !== null); }
  const acts = (dlg) => $$("[type=submit],[data-submit]", dlg);
  // Busy: the button that was used keeps focus and shows progress; every other
  // action is disabled and the body is inert (a page form marks its body with
  // data-hb-body).
  function busy(c, on) {
    const b = c.by;
    c.busy = on;
    acts(c.dlg).forEach((x) => { if (x !== b) x.disabled = on; });
    if (b) {
      b.setAttribute("aria-busy", String(on)); b.setAttribute("aria-disabled", String(on));
      const t = b.lastChild;
      if (on) { b.dataset.l = t.textContent; t.textContent = b.dataset.busy || "Saving…"; b.focus(); } else if (b.dataset.l) t.textContent = b.dataset.l;
    }
    const body = part(c.dlg, ".dlg-b") || part(c.dlg, "[data-hb-body]"); if (body) body.inert = on;
    $$("[data-dlg-close]", c.dlg).forEach((x) => { x.disabled = on; });
  }
  // The dialog's URL: the current page with add= / edit= from the trigger's href.
  function urlWith(href) {
    const u = new URL(location.href);
    u.searchParams.delete("add"); u.searchParams.delete("edit");
    if (href) new URLSearchParams((href.split("?")[1] || "").split("#")[0]).forEach((v, k) => { if (k === "add" || k === "edit") u.searchParams.set(k, v); });
    return u.href;
  }
  HB.open = (dlg, opt) => {
    if (typeof dlg === "string") dlg = doc.getElementById(dlg);
    if (!dlg || !dlg.showModal) return false;
    opt = opt || {};
    let reuse = null;
    if (cur) {
      if (dirty()) { guard(true); return true; } // never drop input to open another
      reuse = cur; reuse.keep = true; reuse.dlg.close(); done(reuse);
    }
    const form = dlg.querySelector("form"), edit = !!opt.values;
    fill(form, opt.values || null);
    acts(dlg).forEach((b) => { if (b.hasAttribute("data-ready")) b.disabled = false; }); // a refused retarget disabled Save
    const h = part(dlg, ".dlg-h h2");
    if (h && h.dataset.add) h.textContent = (edit ? h.dataset.edit : h.dataset.add).replace("{}", (opt.values && opt.values[h.dataset.key || "name"]) || "");
    acts(dlg).forEach((b) => { if (b.dataset.edit) b.lastChild.textContent = edit ? b.dataset.edit : b.dataset.add; });
    dlg.dataset.mode = edit ? "edit" : "add";
    note(dlg); msg(dlg);
    // The base hash: the file version the dialog's data came from, taken now
    // from the list the trigger sits in (or the dialog's data-list) and never
    // updated behind the dialog's back. Saves send it as base_hash, so the
    // server answers 409 whenever the file moved since the dialog opened.
    const src = (opt.trigger && opt.trigger.closest && opt.trigger.closest("[data-hash]")) || (dlg.dataset.list && doc.getElementById(dlg.dataset.list));
    const c = (cur = dlg._hb = { dlg, form, trigger: opt.trigger || doc.activeElement, key: opt.trigger && opt.trigger.dataset.key, base: src ? src.dataset.hash : null, snap: "", pushed: false, landed: !!opt.landed, busy: false, by: null, last: null });
    const g = part(dlg, ".dlg-guard"); if (g) { g.hidden = true; part(dlg, ".dlg-main").hidden = false; }
    // Come from the trigger: a short offset toward it, not a fly-in.
    const t = c.trigger && c.trigger.getBoundingClientRect && c.trigger.getBoundingClientRect();
    const dx = t && t.width && !calm.matches ? (t.left + t.width / 2 - innerWidth / 2) * 0.12 : 0, dy = t && t.width && !calm.matches ? (t.top + t.height / 2 - innerHeight / 2) * 0.12 : 12;
    dlg.style.setProperty("--dx", dx.toFixed(0) + "px"); dlg.style.setProperty("--dy", dy.toFixed(0) + "px");
    dlg.dispatchEvent(new CustomEvent("hb-open", { detail: opt })); // before the snapshot: a handler may fill more
    dlg.showModal();
    c.snap = snapOf(form);
    const f = (opt.focus && form.elements.namedItem(opt.focus)) || firstField(form) || $$(".dlg-b [tabindex], .dlg-b button", dlg).find((x) => x.offsetParent !== null);
    f && f.focus();
    try {
      if (opt.url) {
        const u = (c.url = urlWith(opt.url));
        if (reuse && (reuse.pushed || reuse.landed)) { history.replaceState({ hbDlg: dlg.id }, "", u); c.pushed = reuse.pushed; c.landed = reuse.landed; }
        else if (!c.landed) { history.pushState({ hbDlg: dlg.id }, "", u); c.pushed = true; }
      }
    } catch {}
    return true;
  };
  HB.close = (force) => {
    if (!cur) return;
    if (!force && dirty()) return guard(true);
    const c = cur;
    c.dlg.close();
    done(c);
    return c;
  };
  // Teardown runs synchronously when we close a dialog: the native `close`
  // event fires later, by which time the same dialog may have been opened
  // again with new state. The state is marked done so it runs once.
  function done(c) {
    if (c.done) return;
    c.done = true;
    if (c.dlg._hb === c) c.dlg._hb = null;
    if (cur === c) cur = null;
    if (c.keep) return;
    try {
      if (c.pushed) { HB._skipPop = true; history.back(); }
      else if (c.landed) history.replaceState(null, "", urlWith(""));
    } catch {}
    if (c.after) c.after();
    const t = (c.trigger && c.trigger.isConnected && c.trigger) || (c.focusSel && doc.querySelector(c.focusSel)) || (c.key && doc.querySelector(`[data-dlg][data-key="${CSS.escape(c.key)}"]`));
    if (t && t.focus) t.focus({ preventScroll: true });
  }
  // A native close request (cancel) goes through the guard. A dialog closed
  // some other way (form method=dialog, script) is torn down by its `close`
  // event, unless that state is already done or the dialog is open again.
  doc.addEventListener("cancel", (e) => { if (cur && e.target === cur.dlg) { e.preventDefault(); if (!cur.busy) HB.close(); } }, true);
  doc.addEventListener("close", (e) => {
    const c = e.target._hb;
    if (!c || c.done || e.target.open) return;
    done(c);
  }, true);
  // One submit path for the dialog and for a page-section form with data-hb
  // (Change password): validation, busy, field errors and notes work alike.
  // The handler is `dialog._submit` / `form._submit` / `HB.submit`; it gets the
  // values and {dlg, form, action, mode, key, trigger} and answers
  // {ok, toast, undo, focus, after, stay, note, fields} (HB.result builds one
  // from a settings API answer).
  async function submit(e) {
    const form = e.target;
    if (!form.hasAttribute || !form.hasAttribute("data-hb")) return;
    const inDlg = !!cur && form === cur.form;
    e.preventDefault(); // never a plain post: the guards need X-RH-CSRF
    if (!inDlg && form.closest("dialog")) return;
    const c = inDlg ? cur : form._hb || (form._hb = { dlg: form, form, page: true });
    if (c.busy) return;
    // Save was disabled (an edit that cannot be re-found after a 409): no
    // way around it, not Enter, not Ctrl/⌘ Enter, not "Save and send test".
    const prim = form.querySelector("[type=submit]");
    if (prim && prim.disabled) return;
    // Enter (implicit submission) uses the primary, the form's first submit button.
    c.by = c.pending || (e.submitter && e.submitter.form === form ? e.submitter : form.querySelector("[type=submit]"));
    c.pending = null;
    const action = (c.by && c.by.dataset.submit) || (c.by && c.by.dataset.action) || "save";
    const { n, first } = HB.validate(form);
    note(c.dlg);
    if (n) { msg(c.dlg, n === 1 ? "1 field needs attention." : `${n} fields need attention.`, true); reveal(first); return; }
    msg(c.dlg);
    const save = (inDlg && c.dlg._submit) || form._submit || HB.submit;
    if (typeof save !== "function") {
      // A page bug, not a network problem: say so where a developer looks.
      console.error(`HB: no submit handler for #${c.dlg.id}: set dialog._submit, form._submit or HB.submit`);
      msg(c.dlg, "This form cannot be saved: the page has no save handler.", true);
      return;
    }
    busy(c, true);
    let res;
    try { res = await save(values(form), { dlg: inDlg ? c.dlg : null, form, action, mode: inDlg ? c.dlg.dataset.mode : "page", key: c.key, base: c.base, trigger: c.trigger }); }
    catch (err) { console.error(err); res = { ok: false, note: ["bad", "The server did not answer. Nothing was saved; your input is still here."] }; }
    if (inDlg && cur !== c) return;
    busy(c, false);
    if (res.ok && !inDlg) {
      const nn = res.note && note(form, res.note[0], res.note[1]);
      if (res.toast) HB.toast(res.toast, { kind: res.kind || "ok" });
      if (nn) nn.focus();
      else if (res.refocus) res.refocus.focus(); // an editor saved with Ctrl+S
      return;
    }
    if (res.ok) {
      c.snap = snapOf(form);
      if (res.stay) { const nn = note(c.dlg, res.note && res.note[0], res.note && res.note[1]); (nn || c.by).focus(); return; }
      c.after = res.after; c.focusSel = res.focus;
      HB.close(true);
      if (res.toast) HB.toast(res.toast, { kind: res.kind || "ok", action: res.undo });
      return;
    }
    let f1 = null;
    const lost = [];
    for (const [k, m] of Object.entries(res.fields || {})) { const x = form.elements.namedItem(k); const c1 = x && !x.tagName && x.length ? x[0] : x; if (c1 && !c1.disabled) { setErr(c1, m); f1 = f1 || c1; } else lost.push(m); }
    // An error about a field this form does not show goes to the note.
    const nt = res.note || (lost.length ? ["bad", "Not saved: " + lost.join(" ")] : null);
    const nn = nt ? note(c.dlg, nt[0], nt[1], nt[2]) : null;
    if (f1) { msg(c.dlg, "Not saved: see the highlighted field.", true); reveal(f1); }
    else if (nn) nn.focus();
  }
  HB.note = (kind, text, a) => cur && note(cur.dlg, kind, text, a);
  // The note of a page form (its `.dlg-note`); no text hides it.
  HB.noteIn = (form, kind, text, a) => note(form, kind, text, a);
  // After a save that keeps the dialog open ("Save and send test"): the
  // dialog now edits what was saved. `key` and `base` are that entry and the
  // file version the save itself wrote (never a version written by anyone
  // else, see settings.js); `was` the entry's values as the list now has them.
  HB.edited = (c, key, base, was) => {
    c.key = key; c.base = base; c.was = was;
    c.dlg.dataset.mode = "edit";
    const h = part(c.dlg, ".dlg-h h2");
    if (h && h.dataset.edit) h.textContent = h.dataset.edit.replace("{}", (was && was[h.dataset.key || "name"]) || "");
    acts(c.dlg).forEach((b) => { if (!b.dataset.edit) return; if (b === c.by && c.busy) b.dataset.l = b.dataset.edit; else b.lastChild.textContent = b.dataset.edit; });
    fields(c.form).forEach((x) => { if (x.type !== "checkbox" && x.type !== "radio") x._was = x.value; });
    // Closing now returns to the saved entry's row, and the URL names it.
    c.trigger = doc.querySelector(`[data-dlg][data-key="${CSS.escape(key)}"]`) || c.trigger;
    try { const u = new URL(location.href); u.searchParams.delete("add"); u.searchParams.set("edit", key); history.replaceState(history.state, "", u.href); c.url = u.href; } catch {}
  };
  // The open dialog's state ({dlg, key, base, trigger}; for actions inside
  // it, such as Delete…). A handler may retarget `key` and `base` only after
  // re-locating the same item in a newer file (see settings.js listDialog).
  HB.current = () => cur;
  // Whether the open dialog holds input that is not saved yet, a save in
  // flight included; HB.unsaved() adds any open dialog and the page's own
  // checks (HB.keep(fn)): nothing reloads a page while it is true.
  HB.dirty = () => !!cur && (cur.busy || snapOf(cur.form) !== cur.snap);
  // The open dialog's values are the ones it opened with (or last saved).
  HB.pristine = () => !!cur && snapOf(cur.form) === cur.snap;
  const keeps = [];
  HB.keep = (fn) => keeps.push(fn);
  HB.unsaved = () => !!doc.querySelector("dialog.dlg[open]") || HB.dirty() || keeps.some((f) => f());
  doc.addEventListener("submit", submit);
  doc.addEventListener("keydown", (e) => {
    if (!cur || others()) return; // a confirm or other modal on top handles its own keys
    if (e.key === "Escape") { e.preventDefault(); if (cur.busy) return; const g = part(cur.dlg, ".dlg-guard"); if (g && !g.hidden) guard(false); else HB.close(); }
    else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); cur.form.requestSubmit(); }
  }, true);
  // (`cur.dlg`, not `cur.form`: a field named "contains" would shadow the
  // form's own method, as "id", "name" and "method" shadow its properties.)
  doc.addEventListener("focusin", (e) => { if (cur && cur.dlg.contains(e.target) && e.target.name) cur.last = e.target; });
  // Backdrop: a click that both starts and ends outside the box.
  let downOut = false;
  const outside = (e) => { if (!cur || e.target !== cur.dlg || others()) return false; const r = cur.dlg.getBoundingClientRect(); return e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom; };
  doc.addEventListener("pointerdown", (e) => { downOut = outside(e); });
  doc.addEventListener("click", (e) => {
    if (downOut && outside(e) && !cur.busy) { downOut = false; HB.close(); return; }
    const t = e.target.closest && e.target.closest("[data-dlg],[data-dlg-close],[data-keep],[data-discard],[data-submit]");
    if (!t) return;
    if (t.hasAttribute("data-dlg-close")) { e.preventDefault(); HB.close(); }
    else if (t.hasAttribute("data-keep")) guard(false);
    else if (t.hasAttribute("data-discard")) HB.close(true);
    else if (t.hasAttribute("data-submit")) { if (cur && t.form === cur.form && !cur.busy) { cur.pending = t; cur.form.requestSubmit(); } }
    else if (!(e.ctrlKey || e.metaKey || e.shiftKey || e.button)) {
      let v = null;
      try { v = t.dataset.item ? JSON.parse(t.dataset.item) : null; } catch {}
      if (HB.open(t.dataset.dlg, { trigger: t, values: v, url: t.getAttribute("href"), focus: t.dataset.focus })) e.preventDefault();
    }
  });
  window.addEventListener("popstate", () => {
    if (HB._skipPop) { HB._skipPop = false; return; }
    if (!cur) return;
    cur.pushed = false;
    if (dirty()) { try { history.pushState({ hbDlg: cur.dlg.id }, "", cur.url || location.href); cur.pushed = true; } catch {} setTimeout(() => cur && guard(true), 0); } else HB.close(true);
  });
  // Submit buttons are rendered disabled; they work once this script runs.
  $$("[data-ready]").forEach((b) => { b.disabled = false; });
  root.classList.add("hb-ready"); // hides the "Saving needs JavaScript" notes
  // The URL is the dialog: ?add=x / ?edit=x:id opens it on load (a reload, a
  // shared link, Ctrl-click into a new tab). Without script it is the page form.
  setTimeout(() => {
    const p = new URLSearchParams(location.search);
    // `edit` wins over `add` when both are given (the server's page form agrees).
    for (const k of ["edit", "add"]) {
      if (!p.has(k)) continue;
      const t = $$("[data-dlg][href]").find((a) => new URLSearchParams((a.getAttribute("href").split("?")[1] || "").split("#")[0]).get(k) === p.get(k));
      if (t) { let v = null; try { v = t.dataset.item ? JSON.parse(t.dataset.item) : null; } catch {} HB.open(t.dataset.dlg, { trigger: t, values: v, landed: true, focus: t.dataset.focus }); break; }
    }
  }, 0);

  // ---- toast: one polite stack, at most three, errors stay until closed.
  // While a dialog is open nothing toasts: the message goes to the dialog note.
  let box = null;
  HB.toast = (text, o) => {
    o = o || {};
    const kind = o.kind || "ok";
    if (cur && cur.dlg.open && note(cur.dlg, kind === "info" ? "" : kind, text)) return () => {};
    if (!box) { box = el("div", "toasts"); box.setAttribute("role", "status"); box.setAttribute("aria-live", "polite"); doc.body.append(box); }
    const t = el("div", "toast " + kind);
    if (kind === "bad") t.setAttribute("role", "alert");
    t.append(icon(kind === "bad" ? "alert" : kind === "info" ? "info" : "check"), el("span", "", text));
    if (o.action) { const a = el("button", "btn sm ghost", o.action.label); a.type = "button"; a.onclick = () => { o.action.run(); gone(); }; t.append(a); }
    const x = el("button", "btn sm icon ghost"); x.type = "button"; x.setAttribute("aria-label", "Dismiss"); x.append(icon("x")); x.onclick = () => gone(); t.append(x);
    let timer = null;
    const arm = () => { if (kind !== "bad" && !o.sticky) timer = setTimeout(gone, o.ms || (o.action ? 8000 : 4500)); };
    function gone() { clearTimeout(timer); if (!t.isConnected) return; t.classList.add("out"); setTimeout(() => t.remove(), calm.matches ? 0 : 200); }
    t.addEventListener("pointerenter", () => clearTimeout(timer)); t.addEventListener("pointerleave", arm);
    t.addEventListener("focusin", () => clearTimeout(timer));
    box.append(t);
    while (box.children.length > 3) box.firstElementChild.remove();
    arm();
    return gone;
  };

  // ---- writes: the session cookie, X-RH-CSRF from <meta name=rh-csrf>, same
  // origin; the server's guards stay the judge. After a new sign-in (another
  // tab) the cookie names a new session whose token this page does not have:
  // a 401, or the gate's plain-text 403, makes us read the token from a fresh
  // same-origin GET of this page and try once more (no token route, SET-17).
  const csrf = () => { const m = doc.querySelector('meta[name="rh-csrf"]'); return m ? m.content : ""; };
  async function freshToken() {
    try {
      const r = await fetch(location.pathname, { credentials: "same-origin", cache: "no-store" });
      const m = new DOMParser().parseFromString(await r.text(), "text/html").querySelector('meta[name="rh-csrf"]');
      if (!m || !m.content || m.content === csrf()) return false;
      let mine = doc.querySelector('meta[name="rh-csrf"]');
      if (!mine) { mine = el("meta"); mine.name = "rh-csrf"; doc.head.append(mine); }
      mine.content = m.content;
      return true;
    } catch { return false; }
  }
  HB.api = async (method, path, body, again) => {
    const headers = { "X-RH-CSRF": csrf() };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    let r;
    try { r = await fetch(path, { method, credentials: "same-origin", headers, body: body === undefined ? undefined : JSON.stringify(body) }); }
    catch { return { status: 0, data: { error: "The server did not answer." } }; }
    const json = /json/.test(r.headers.get("content-type") || "");
    if (!again && (r.status === 401 || (r.status === 403 && !json)) && (await freshToken())) return HB.api(method, path, body, true);
    let data = {};
    try { data = json ? await r.json() : { error: (await r.text()).trim() }; } catch {}
    return { status: r.status, data };
  };
  // A settings API answer as a submit result (DESIGN.md §11.6 "Server
  // errors"). `o.reload` / `o.discard` are the two ways out of a 409.
  HB.result = (res, o) => {
    o = o || {};
    const d = res.data || {}, st = res.status;
    if (st === 200 && d.ok !== false) return { ok: true, data: d };
    if (st === 422 && d.field) return { ok: false, fields: { [d.field]: d.error } };
    if (st === 409 && d.stale) {
      const acts = [];
      if (o.reload) acts.push({ label: "Reload and keep my input", cls: "primary", run: o.reload });
      if (o.discard) acts.push({ label: "Discard my input", run: o.discard });
      return { ok: false, note: ["warn", "Not saved: the config file changed on disk since this was loaded (maybe a hand edit). Your input is still here.", acts] };
    }
    if (st === 401) return { ok: false, note: ["bad", "Not saved: your session expired. Sign in again in a new tab, then save here again; your input stays.", [{ label: "Sign in (new tab)", cls: "primary", run: () => window.open("/login?next=" + encodeURIComponent(location.pathname), "_blank", "noopener") }]] };
    if (st === 403 && d.locked) return { ok: false, note: ["lock", d.error] };
    if (st === 403) return { ok: false, note: ["bad", "Not saved: the security check failed. Copy your input, then reload the page."] };
    if (st === 0) return { ok: false, note: ["bad", "Not saved: the server did not answer. Your input is still here."] };
    return { ok: false, note: ["bad", "Not saved: " + (d.error || `the server answered HTTP ${st}.`)] };
  };

  // ---- confirm-destructive: says what goes, Cancel is the default ----
  HB.confirm = (o) => new Promise((done) => {
    const d = doc.getElementById("dlg-confirm");
    if (!d || !d.showModal) return done(window.confirm(o.title));
    const ok = d.querySelector("[data-ok]"), typed = d.querySelector("[data-typed]"), inp = typed.querySelector("input");
    d.querySelector("h2 span").textContent = o.title;
    d.querySelector("[data-body]").textContent = o.body || "";
    ok.lastChild.textContent = o.ok || "Delete";
    typed.hidden = !o.typed;
    if (o.typed) { typed.querySelector("b").textContent = o.typed; inp.value = ""; }
    const match = () => !o.typed || inp.value.trim().toLowerCase() === o.typed.toLowerCase();
    ok.disabled = !match();
    inp.oninput = () => { ok.disabled = !match(); };
    const back = doc.activeElement;
    const end = (v) => { d.oncancel = d.onclick = null; d.close(); back && back.isConnected && back.focus({ preventScroll: true }); done(v); };
    ok.onclick = () => match() && end(true);
    d.querySelector("[data-cancel]").onclick = () => end(false);
    d.oncancel = (e) => { e.preventDefault(); end(false); };
    d.onclick = (e) => { if (e.target === d) end(false); };
    d.showModal();
    (o.typed ? inp : d.querySelector("[data-cancel]")).focus();
  });

  // ---- menu: a link to the plain page without script; a popover with it ----
  function place(m, t) {
    const r = t.getBoundingClientRect();
    m.style.inset = "auto"; m.style.margin = "0";
    const w = m.offsetWidth, h = m.offsetHeight;
    let x = m.dataset.align === "start" ? r.left : r.right - w, y = r.bottom + 6;
    if (y + h > innerHeight - 8 && r.top - h - 6 > 8) y = r.top - h - 6;
    m.style.left = Math.max(8, Math.min(x, innerWidth - w - 8)) + "px"; m.style.top = Math.max(8, y) + "px";
  }
  const items = (m) => $$(".mi:not([disabled])", m);
  const pop = !!HTMLElement.prototype.showPopover;
  function opened(m, t) {
    HB.menuFor = t; t.setAttribute("aria-expanded", "true");
    place(m, t);
    const f = m.querySelector('[aria-checked="true"]') || items(m)[0]; f && f.focus();
  }
  // Light dismiss closes an open menu on pointerdown, before the click: remember.
  doc.addEventListener("pointerdown", (e) => { const t = e.target.closest && e.target.closest("[data-menu]"); if (t) { const m = doc.getElementById(t.dataset.menu); t._was = !!m && HB.menuFor === t && (pop ? m.matches(":popover-open") : m.classList.contains("open")); } }, true);
  function toggleMenu(t) {
    const m = doc.getElementById(t.dataset.menu);
    if (!m) return;
    const isOpen = t._was != null ? t._was : pop ? m.matches(":popover-open") : m.classList.contains("open");
    t._was = null;
    if (pop) { if (isOpen && HB.menuFor === t) return m.matches(":popover-open") && m.hidePopover(); if (m.matches(":popover-open")) m.hidePopover(); HB.menuFor = t; m.showPopover(); opened(m, t); }
    else { m.classList.toggle("open", !isOpen); isOpen ? t.setAttribute("aria-expanded", "false") : opened(m, t); }
  }
  doc.addEventListener("click", (e) => {
    const t = e.target.closest && e.target.closest("[data-menu]");
    if (t) { e.preventDefault(); toggleMenu(t); return; }
    const mi = e.target.closest && e.target.closest(".menu .mi");
    if (mi) { const m = mi.closest(".menu"); pop ? m.hidePopover() : m.classList.remove("open"); }
  });
  doc.addEventListener("toggle", (e) => {
    const m = e.target, t = HB.menuFor;
    if (!m.classList || !m.classList.contains("menu") || e.newState !== "closed" || !t) return;
    t.setAttribute("aria-expanded", "false");
    const a = doc.activeElement; if (a === doc.body || m.contains(a)) t.focus({ preventScroll: true });
  }, true);
  doc.addEventListener("keydown", (e) => {
    const t = e.target.closest && e.target.closest("a[data-menu]");
    if (t && e.key === " ") { e.preventDefault(); toggleMenu(t); return; }
    const m = e.target.closest && e.target.closest(".menu");
    if (!m || !/^(ArrowDown|ArrowUp|Home|End)$/.test(e.key)) return;
    e.preventDefault();
    const l = items(m), i = l.indexOf(doc.activeElement);
    const j = e.key === "Home" ? 0 : e.key === "End" ? l.length - 1 : (i + (e.key === "ArrowDown" ? 1 : -1) + l.length) % l.length;
    l[j] && l[j].focus();
  });

  // ---- list changes: a new or changed row flashes once; a removed row folds away ----
  HB.flash = (row) => { row.classList.remove("row-new"); void row.offsetWidth; row.classList.add("row-new"); setTimeout(() => row.classList.remove("row-new"), 1700); };
  HB.remove = (row) => {
    if (calm.matches || !row.animate) return row.remove();
    row.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 160, easing: "ease-in" }).onfinish = () => row.remove();
  };
})();
