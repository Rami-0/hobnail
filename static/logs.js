// Logs pages only. OWNER: logs work.
// Each pane holds a bounded ring of lines (data-ring, default 5000) fed by
// /api/logs/stream (SSE). Log text only ever reaches the DOM through
// textContent / createTextNode: never as HTML.
"use strict";
(function () {
  // ---------------------------------------------------------------- /logs list
  // Filter as you type (the server filters `?q=` without the script), Enter
  // opens the first match, "Side by side" for two to four ticked.
  var pick = document.getElementById("lpick");
  if (pick) {
    var pq = document.getElementById("lpq"), go = document.getElementById("lpick-go");
    var rows = [].slice.call(pick.querySelectorAll("li[data-s]"));
    var groups = [].slice.call(pick.querySelectorAll(".lpick-g"));
    var filt = function () {
      var v = pq.value.trim(), terms = v.toLowerCase().split(/\s+/).filter(Boolean), n = 0;
      rows.forEach(function (li) {
        var on = terms.every(function (t) { return li.dataset.s.indexOf(t) >= 0; });
        li.hidden = !on;
        if (on) n++;
      });
      groups.forEach(function (g) { g.hidden = !g.querySelector("li[data-s]:not([hidden])"); });
      document.getElementById("lpick-n").textContent = (n === rows.length ? rows.length : n + " of " + rows.length) + " containers";
      document.getElementById("lpick-none").hidden = n > 0;
      addr();
    };
    // The address keeps the filter (`q=`) and the ticks (`sel=`, which the
    // server ticks again), so a reload or a shared link keeps both.
    var addr = function () {
      var v = pq.value.trim(), sel = [].map.call(pick.querySelectorAll("input[name=c]:checked"), function (b) { return encodeURIComponent(b.value); });
      var u = location.origin + location.pathname + (v || sel.length ? "?" : "") + (v ? "q=" + encodeURIComponent(v) : "") + (v && sel.length ? "&" : "") + (sel.length ? "sel=" + sel.join(",") : "");
      try { if (u !== location.href.split("#")[0]) history.replaceState(history.state, "", u + location.hash); } catch (e) {}
    };
    // LG-05: the button counts the ticks and stays off below two; at four the
    // other boxes are off and the hint says why.
    var hint = document.getElementById("lpick-hint"), hint0 = hint.textContent;
    var ticks = function () {
      var n = pick.querySelectorAll("input[name=c]:checked").length;
      go.disabled = n < 2;
      go.title = n < 2 ? "Tick two to four containers" : "Show " + n + " side by side";
      document.getElementById("lpick-k").textContent = n ? " · " + n : "";
      hint.textContent = n >= 4 ? "Four at a time: untick one to choose another." : hint0;
      [].forEach.call(pick.querySelectorAll("input[name=c]:not(:checked)"), function (b) { b.disabled = n >= 4; });
      addr();
    };
    // A short address (`/logs?c=a,b`) instead of the form's `go=1&c=a&c=b`.
    document.getElementById("lpick-c").addEventListener("submit", function (e) {
      var v = [].map.call(pick.querySelectorAll("input[name=c]:checked"), function (b) { return encodeURIComponent(b.value); });
      if (!v.length) return;
      e.preventDefault();
      location.href = "/logs?c=" + v.join(",");
    });
    pq.addEventListener("input", filt);
    pq.addEventListener("keydown", function (e) {
      if (e.key !== "Enter") return;
      e.preventDefault();
      var a = pick.querySelector("li[data-s]:not([hidden]) a.row-link");
      if (a) location.href = a.href;
    });
    pick.addEventListener("change", ticks);
    ticks();
    // Back to this page: the browser restores the ticks and the filter text
    // after the script ran (or keeps the whole page); recount both.
    window.addEventListener("pageshow", function () { filt(); ticks(); });
    return;
  }
  var root = document.getElementById("lv");
  if (!root) return;
  var RING = +root.dataset.ring || 5000;
  var TAIL = +root.dataset.tail || 500;
  var MERGED = !!root.dataset.merged;
  var $ = function (id) { return document.getElementById(id); };
  var LV = ["e", "w", "i", "d", "o"], LVW = { e: "error", w: "warning", i: "info", d: "debug", o: "" };
  var S = {
    q: "", re: false, cs: false, only: false, ctx: 0,
    lv: { e: 1, w: 1, i: 1, d: 1, o: 1 },
    wrap: true, time: true, paused: false,
    range: "", from: null, to: null, target: null
  };
  var test = null; // compiled search: function(plain) -> bool
  var hlRe = null; // global regex for highlighting
  var seq = 0;
  var HL = typeof CSS !== "undefined" && CSS.highlights && typeof Highlight !== "undefined";

  function store(k, v) { try { localStorage.setItem("rh-logs-" + k, v); } catch (e) {} }
  function load(k) { try { return localStorage.getItem("rh-logs-" + k); } catch (e) { return null; } }
  if (load("wrap") === "0") S.wrap = false;
  if (load("time") === "0") S.time = false;
  // LG-13: the search lives in the address (`search=`, `re=1`, `case=1`), so
  // a reload or a shared link keeps it. Other parameters are left as they are.
  function param(k) { var m = new RegExp("[?&]" + k + "=([^&]*)").exec(location.search); return m ? decodeURIComponent(m[1].replace(/\+/g, " ")) : ""; }
  // A regular expression from the address is not run on its own (a shared
  // link could hold one that takes the tab down): its text comes back as
  // plain text and the reader turns ".*" on to use it.
  S.q = param("search"); S.cs = param("case") === "1";
  var reFromLink = !!S.q && param("re") === "1";
  function keepSearch() {
    var rest = location.search.replace(/^\?/, "").split("&").filter(function (x) { return x && !/^(search|re|case)=/.test(x); });
    if (S.q) rest.push("search=" + encodeURIComponent(S.q));
    if (S.q && S.re) rest.push("re=1");
    if (S.q && S.cs) rest.push("case=1");
    var u = location.pathname + (rest.length ? "?" + rest.join("&") : "") + location.hash;
    if (u !== location.pathname + location.search + location.hash) try { history.replaceState(history.state, "", u); } catch (e) {}
  }

  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  // FNV-1a, same as logs::pages::hue.
  function hue(name) {
    var h = 2166136261;
    for (var i = 0; i < name.length; i++) h = Math.imul(h ^ name.charCodeAt(i), 16777619) >>> 0;
    return h % 360;
  }
  var STACK = root.dataset.stack || "";
  function shortName(n) {
    if (STACK && n.indexOf(STACK) === 0 && /[-_]/.test(n.charAt(STACK.length)) && n.length > STACK.length + 1) return n.slice(STACK.length + 1);
    return n;
  }
  // LG-23: names in a stack often differ only at the end ("web-app-1",
  // "web-app-2"), so a long chip keeps both ends: "web-ap…pp-2".
  var CHIP = window.innerWidth <= 699 ? 12 : 20;
  function chipName(n) {
    n = shortName(n);
    return n.length <= CHIP ? n : n.slice(0, Math.ceil(CHIP / 2) - 1) + "…" + n.slice(n.length - Math.floor(CHIP / 2));
  }

  // ---------------------------------------------------------------- ANSI
  // SGR subset: 16/256/truecolor fg and bg, bold, dim, italic, underline.
  var CTRL = /[\x00-\x08\x0b-\x1f\x7f]/g;
  function c256(n) {
    if (n < 16) return { k: n };
    if (n < 232) {
      n -= 16;
      var v = function (x) { return x ? 55 + x * 40 : 0; };
      return { r: "rgb(" + v(Math.floor(n / 36)) + "," + v(Math.floor(n / 6) % 6) + "," + v(n % 6) + ")" };
    }
    var g = 8 + (n - 232) * 10;
    return { r: "rgb(" + g + "," + g + "," + g + ")" };
  }
  function ansi(m) {
    var segs = [], st = {}, i = 0, re = /\x1b\[([0-9;:?]*)([@-~])|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)?|\x1b./g, r;
    var push = function (t) {
      t = t.replace(CTRL, "");
      if (t) segs.push({ t: t, f: st.f, b: st.b, bo: st.bo, di: st.di, it: st.it, un: st.un });
    };
    while ((r = re.exec(m))) {
      push(m.slice(i, r.index));
      i = re.lastIndex;
      if (r[2] !== "m") continue;
      var p = (r[1] || "0").split(/[;:]/).map(Number);
      for (var k = 0; k < p.length; k++) {
        var n = p[k];
        if (n === 0 || isNaN(n)) st = {};
        else if (n === 1) st.bo = 1;
        else if (n === 2) st.di = 1;
        else if (n === 3) st.it = 1;
        else if (n === 4) st.un = 1;
        else if (n === 22) st.bo = st.di = 0;
        else if (n === 23) st.it = 0;
        else if (n === 24) st.un = 0;
        else if (n >= 30 && n <= 37) st.f = { k: n - 30 };
        else if (n >= 90 && n <= 97) st.f = { k: n - 82 };
        else if (n >= 40 && n <= 47) st.b = { k: n - 40 };
        else if (n >= 100 && n <= 107) st.b = { k: n - 92 };
        else if (n === 39) st.f = null;
        else if (n === 49) st.b = null;
        else if (n === 38 || n === 48) {
          var c = null;
          if (p[k + 1] === 5) { c = c256(p[k + 2] | 0); k += 2; }
          else if (p[k + 1] === 2) { c = { r: "rgb(" + (p[k + 2] | 0) + "," + (p[k + 3] | 0) + "," + (p[k + 4] | 0) + ")" }; k += 4; }
          if (n === 38) st.f = c; else st.b = c;
        }
      }
    }
    push(m.slice(i));
    return segs;
  }
  function segNode(s) {
    if (!s.f && !s.b && !s.bo && !s.di && !s.it && !s.un) return document.createTextNode(s.t);
    var e = el("span", "", s.t), c = [];
    if (s.f) { if (s.f.k != null) c.push("af" + s.f.k); else e.style.color = s.f.r; }
    if (s.b) {
      if (s.b.k != null) c.push("ab" + s.b.k); else e.style.backgroundColor = s.b.r;
      // A background without a foreground: pick a readable text color.
      if (!s.f) {
        var dark;
        if (s.b.k != null) dark = [0, 1, 4, 5, 8].indexOf(s.b.k) >= 0;
        else { var v = s.b.r.match(/\d+/g).map(Number); dark = v[0] * 0.299 + v[1] * 0.587 + v[2] * 0.114 < 140; }
        e.style.color = dark ? "#f9fafb" : "#111827";
      }
    }
    if (s.bo) c.push("abo");
    if (s.di) c.push("adi");
    if (s.it) c.push("ait");
    if (s.un) c.push("aun");
    e.className = c.join(" ");
    return e;
  }

  // ---------------------------------------------------------------- time
  function p2(n) { return n < 10 ? "0" + n : "" + n; }
  var today = new Date().toDateString(), year = new Date().getFullYear();
  var MON = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  function hms(d) { return p2(d.getHours()) + ":" + p2(d.getMinutes()) + ":" + p2(d.getSeconds()); }
  // LG-27: another day reads "26 Oct" (and the year when it is not this one).
  function day(d) { return d.getDate() + " " + MON[d.getMonth()] + (d.getFullYear() !== year ? " " + d.getFullYear() : ""); }
  function when(ms) { var d = new Date(ms); return (d.toDateString() !== today ? day(d) + " " : "") + hms(d); }
  function timeEl(ms) {
    var d = new Date(ms);
    var t = hms(d);
    if (d.toDateString() !== today) t = day(d) + " " + t;
    var e = el("time", "lt", t);
    e.appendChild(el("span", "lms", "." + ("00" + d.getMilliseconds()).slice(-3)));
    return e;
  }

  // ---------------------------------------------------------------- search
  function compile() {
    var q = S.q, box = $("lq"), err = null;
    test = null; hlRe = null;
    if (q) {
      try {
        var src = S.re ? q : q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
        var flags = S.cs ? "" : "i";
        var r1 = new RegExp(src, flags);
        hlRe = new RegExp(src, flags + "g");
        test = function (s) { return r1.test(s); };
        if (!S.re && !S.cs) {
          var lq = q.toLowerCase();
          test = function (s) { return s.toLowerCase().indexOf(lq) >= 0; };
        } else if (!S.re) {
          test = function (s) { return s.indexOf(q) >= 0; };
        }
      } catch (e) { err = e.message; }
    }
    box.setAttribute("aria-invalid", err ? "true" : "false");
    // The engine's message already starts with "Invalid regular expression".
    box.title = err ? (/^invalid regular expression/i.test(err) ? err : "Invalid regular expression: " + err) : "";
    var m = $("lqerr");
    if (!m) { m = el("span", "lqerr"); m.id = "lqerr"; m.setAttribute("role", "status"); box.parentNode.appendChild(m); }
    m.textContent = err ? "Invalid regex" : "";
  }

  // ---------------------------------------------------------------- panes
  function Pane(node) {
    this.node = node;
    this.q = node.dataset.q;
    this.body = node.querySelector(".lbody");
    this.box = node.querySelector(".lines");
    this.msg = node.querySelector(".lmsg");
    this.jump = node.querySelector(".ljump");
    this.jn = node.querySelector(".ljn");
    this.st = node.querySelector(".lph-s");
    this.lines = [];
    this.names = {};
    this.ended = {};
    this.counts = { e: 0, w: 0, i: 0, d: 0, o: 0 };
    this.per = {}; // lines per container in the first load (LG-25)
    this.marked = false;
    this.follow = true;
    this.unseen = 0;
    this.pending = [];
    this.lastT = 0;
    this.lastMatch = -1e9;
    this.es = null;
    this.raf = 0;
    this.lastTop = 0;
    var self = this;
    // Following = the newest line is in view. Any move up stops it, however
    // small or slow (growth below never moves scrollTop; the script's own
    // moves set lastTop first, so they never count). Only a move down into
    // the bottom band, "Jump to latest" or End starts it again. A clamp after
    // the content shrank (scrollTop falls, but the view is at the very
    // bottom) is not a move up.
    // Scrolled up, the reader's line is kept (`at`, `restore`): scroll
    // anchoring is off (`overflow-anchor:none`), the script does it.
    this.body.addEventListener("scroll", function () {
      var b = self.body;
      // The heights changed and the view moved with them (a clamp) before
      // the observer saw it: not the reader's move. Put the line back.
      var kept = !self.follow && self.at && b.scrollHeight !== self.atH && self.restore(self.at);
      if (!kept) {
        var top = b.scrollTop, dist = b.scrollHeight - top - b.clientHeight;
        if (top < self.lastTop - 1 && dist > 1) self.follow = false;
        else if (dist <= 24 && top > self.lastTop && !self.follow) { self.follow = true; self.unseen = 0; }
        self.lastTop = top;
        self.at = self.mark();
        self.atH = b.scrollHeight;
      }
      self.pill();
      if (HL && S.q && !self.hlPending) {
        self.hlPending = true;
        requestAnimationFrame(function () { self.hlPending = false; highlightAll(); });
      }
    }, { passive: true });
    this.jump.addEventListener("click", function () { self.toBottom(); self.body.focus({ preventScroll: true }); });
    this.older = node.querySelector(".lolder");
    this.older.addEventListener("click", function () { self.loadOlder(); });
    this.chips = node.querySelector(".lchipd");
    // LG-19: the chip list of a stack is open on wide screens only.
    if (this.chips && window.innerWidth > 699) this.chips.open = true;
    // LG-23: the chip column is as wide as the longest (shortened) name.
    var w = 0;
    [].forEach.call(node.querySelectorAll(".lcn[data-name]"), function (c) { w = Math.max(w, chipName(c.dataset.name).length); });
    if (w) node.style.setProperty("--lcw", w + "ch");
    // Whatever changes the height (a batch, Wrap, Time, a filter, a resize,
    // a JSON line opened) is answered after layout and before paint:
    // following, the bottom is set again; scrolled up, the reader's line is
    // put back where it was. One observer per pane; it fires only on a change.
    if (window.ResizeObserver) {
      var ro = this.ro = new ResizeObserver(function () { if (self.follow) self.stick(); else self.restore(self.at); });
      ro.observe(this.box);
      ro.observe(this.body);
    }
  }

  // A pane closed in place (LG-07): nothing of it keeps running.
  Pane.prototype.close = function () {
    if (this.es) this.es.close();
    if (this.oes) this.oes.close();
    clearTimeout(this.retry); clearTimeout(this.quiet);
    if (this.raf) cancelAnimationFrame(this.raf);
    if (this.ro) this.ro.disconnect();
    this.closed = true;
  };

  Pane.prototype.stick = function () {
    this.body.scrollTop = this.body.scrollHeight;
    this.lastTop = this.body.scrollTop;
  };

  Pane.prototype.pill = function () {
    this.jump.hidden = this.follow;
    this.jn.textContent = this.unseen ? " · " + this.unseen + " new" : "";
  };

  // Scrolled up: the reader's line (the first one that starts in view, or
  // the one line filling it) and its offset, taken on each scroll, so a
  // change of heights can put it back. Following: nothing to keep.
  Pane.prototype.mark = function () {
    if (this.follow) return null;
    var top = this.body.scrollTop, L = this.lines, lo = 0, hi = L.length;
    while (lo < hi) {
      var mid = (lo + hi) >> 1, j = mid;
      while (j < hi && L[j].e.hidden) j++;
      if (j === hi) hi = mid;
      else if (L[j].e.offsetTop < top) lo = j + 1;
      else hi = mid;
    }
    while (lo < L.length && L[lo].e.hidden) lo++;
    var cut = lo - 1;
    while (cut >= 0 && L[cut].e.hidden) cut--;
    var r = lo < L.length && L[lo].e.offsetTop < top + this.body.clientHeight ? L[lo] : cut >= 0 ? L[cut] : null;
    return r ? { e: r.e, off: r.e.offsetTop - top, h: r.e.offsetHeight || 1 } : null;
  };
  // The script moved the view itself (a seek, the trim): not a move by the
  // reader, and the new place is the one to keep.
  Pane.prototype.moved = function () {
    this.lastTop = this.body.scrollTop;
    this.at = this.mark();
    this.atH = this.body.scrollHeight;
  };
  // (A line cut by the top edge keeps the same share of it above the edge.)
  Pane.prototype.restore = function (m) {
    if (this.follow) return false;
    // The line is gone (trimmed) or filtered out: keep the place as it is now.
    if (!m || !m.e.isConnected || m.e.hidden) { this.moved(); return false; }
    var off = m.off < 0 ? m.off * m.e.offsetHeight / m.h : m.off;
    this.body.scrollTop = m.e.offsetTop - off;
    this.lastTop = this.body.scrollTop;
    this.atH = this.body.scrollHeight;
    return true;
  };


  Pane.prototype.toBottom = function () {
    this.follow = true;
    this.unseen = 0;
    this.stick();
    this.pill();
  };

  Pane.prototype.status = function (t, cls) {
    if (!this.st) return;
    this.st.textContent = t;
    this.st.dataset.st = cls || "";
  };

  // What the pane says instead of lines: `wait` a skeleton on a busy pane
  // (it shows after 150 ms, so a fast start shows nothing), else the empty
  // state (title, one sentence, at most one action). `msg.dataset.t` is
  // the title, for the checks below.
  Pane.prototype.message = function (t, o) {
    o = o || {};
    var m = this.msg;
    m.textContent = "";
    m.dataset.t = t || "";
    m.hidden = !t;
    this.body.setAttribute("aria-busy", t && o.wait ? "true" : "false");
    if (!t) return;
    if (o.wait) {
      var r = el("div", "sk-rows");
      for (var i = 0; i < 5; i++) r.appendChild(el("span", "sk"));
      m.appendChild(r);
      m.appendChild(el("span", "sk-t", t));
      return;
    }
    var e = el("div", "empty compact" + (o.tone ? " " + o.tone : "")), ic = el("span", "empty-ic");
    var s = document.createElementNS("http://www.w3.org/2000/svg", "svg"), u = document.createElementNS(s.namespaceURI, "use");
    s.setAttribute("class", "i"); s.setAttribute("aria-hidden", "true"); u.setAttribute("href", "#i-" + (o.icon || "logs")); s.appendChild(u); ic.appendChild(s);
    e.appendChild(ic); e.appendChild(el("h3", "", t));
    if (o.body) e.appendChild(el("p", "", o.body));
    if (o.act) { var a = el("div", "acts"), b = el("button", "btn sm", o.act[0]); b.type = "button"; b.addEventListener("click", o.act[1]); a.appendChild(b); e.appendChild(a); }
    m.appendChild(e);
  };
  Pane.prototype.waiting = function () { return /^(Connecting…|Waiting for lines…)$/.test(this.msg.dataset.t || ""); };

  Pane.prototype.url = function (resume) {
    var u = "/api/logs/stream?" + this.q, now = Date.now() / 1000;
    if (resume && this.lastT) return u + "&since=" + (this.lastT / 1000 + 0.000001).toFixed(6) + "&tail=" + RING;
    if (S.target) return u + "&since=" + Math.max(0, S.target / 1000 - 120).toFixed(3) + "&tail=" + RING;
    switch (S.range) {
      case "": return u + "&tail=" + TAIL;
      case "start": return u + "&since=start&tail=" + RING;
      case "custom":
        return u + (S.from ? "&since=" + S.from : "") + (S.to ? "&until=" + S.to : "") + "&tail=" + RING;
      default: return u + "&since=" + S.range + "&tail=" + RING;
    }
  };

  Pane.prototype.connect = function (resume) {
    var self = this;
    if (this.closed) return;
    if (this.es) this.es.close();
    if (!resume) this.message("Connecting…", { wait: true });
    var es = new EventSource(this.url(resume));
    this.es = es;
    var done = false;
    es.addEventListener("meta", function (e) {
      var m = JSON.parse(e.data);
      self.down = false;
      self.names[m.c] = m.name;
      if (!MERGED) self.status(m.running ? "live" : "stopped", m.running ? "ok" : "off");
      else self.status("live", "ok");
    });
    es.addEventListener("l", function (e) { self.add(JSON.parse(e.data)); });
    es.addEventListener("err", function (e) {
      var m = JSON.parse(e.data);
      var t = m.kind === "notfound" ? "Container not found." :
        m.kind === "unreachable" ? "Docker is unreachable. " + m.msg : m.msg;
      self.status(m.kind === "notfound" ? "not found" : "error", "bad");
      if (!self.lines.length) self.message(m.kind === "notfound" ? "Container not found" : "Could not read the logs", { tone: "bad", icon: "alert", body: m.kind === "notfound" ? "It may have been removed or renamed." : t });
    });
    var cut = false;
    es.addEventListener("end", function (e) {
      var m = JSON.parse(e.data);
      self.ended[m.c] = 1;
      if (m.cut) cut = true;
    });
    es.addEventListener("done", function () {
      done = true;
      es.close();
      var live = S.range !== "custom" || !S.to;
      // A closed range with more lines than the server reads at once: say
      // where it stops instead of calling it complete.
      if (cut) {
        self.cut = true;
        self.status("first " + RING.toLocaleString() + " lines", "warn");
        var cm = el("div", "lmark lcut", "Showing the first " + RING.toLocaleString() + " lines of this range. Narrow the range to see the rest.");
        cm.setAttribute("role", "note");
        // Lines still waiting to be drawn go in front of it (see `add`).
        self.cutEl = cm;
        self.box.appendChild(cm);
        if (self.follow) self.stick();
      } else self.status(live && !MERGED ? "stopped" : "complete", "off");
      // LG-15: a range without lines says so, whatever the pane said before.
      if (!self.lines.length && !self.pending.length && (self.waiting() || self.msg.hidden || self.msg.dataset.t === "No lines yet")) self.message("No lines in this range", { icon: "history", body: "Pick a longer range, or Start for everything Docker kept." });
      counter();
    });
    es.addEventListener("hello", function () {
      self.retryMs = 2000;
      if (self.down) { self.down = false; counter(); }
      if (/^(Connecting…|Disconnected, retrying)$/.test(self.msg.dataset.t)) self.message("Waiting for lines…", { wait: true });
      // Connected: what Docker kept arrives right behind `hello`. A quiet
      // container has none, so after a moment the pane says so plainly,
      // not busy, instead of a skeleton that never ends.
      clearTimeout(self.quiet);
      self.quiet = setTimeout(function () {
        if (self.es === es && !done && !self.lines.length && !self.pending.length && self.waiting()) self.message("No lines yet", { icon: "logs", body: "Waiting for the container to log." });
        if (self.es === es) { self.markStarts(); if (findOnLoad) { findOnLoad = false; var fm = matches(); showMatch(fm[fm.length - 1] || null, true); counter(); } }
      }, 1200);
    });
    es.onerror = function () {
      if (done) return;
      es.close();
      self.status("reconnecting", "warn");
      // ROB-13: the tail stopped; the counter says so on every pane layout,
      // and an empty pane says it instead of waiting.
      self.down = true;
      counter();
      if (!self.lines.length) self.message("Disconnected, retrying", { tone: "bad", icon: "alert", body: "The log stream stopped. It starts again by itself when the server answers." });
      clearTimeout(self.retry);
      // 2 s, then twice as long each time, up to 30 s (a connection that
      // says hello starts again from 2 s): no tight loop on a refusing proxy.
      var wait = self.retryMs || 2000;
      self.retryMs = Math.min(30000, wait * 2);
      self.retry = setTimeout(function () { self.connect(true); }, wait);
    };
  };

  Pane.prototype.reset = function () {
    this.cut = false; this.cutEl = null;
    if (this.es) this.es.close();
    clearTimeout(this.retry);
    clearTimeout(this.quiet);
    this.lines = [];
    this.pending = [];
    this.box.textContent = "";
    this.counts = { e: 0, w: 0, i: 0, d: 0, o: 0 };
    this.lastT = 0;
    this.follow = true;
    this.unseen = 0;
    this.per = {}; this.marked = false; this.noOlder = false;
    if (this.oes) { this.oes.close(); this.oes = null; this.loading = false; }
    this.pill();
    this.connect(false);
    counter();
  };

  function mk(x) {
    var rec = { t: x.t, c: x.c, s: x.s || 1, l: x.l || "o", m: x.m, x: x.x, n: ++seq, e: null };
    rec.p = x.m.indexOf("\x1b") >= 0 ? null : x.m.replace(CTRL, "");
    if (rec.p === null) { rec.segs = ansi(x.m); rec.p = rec.segs.map(function (s) { return s.t; }).join(""); }
    return rec;
  }

  Pane.prototype.add = function (arr) {
    for (var k = 0; k < arr.length; k++) {
      var rec = mk(arr[k]);
      if (rec.t > this.lastT) this.lastT = rec.t;
      if (!this.marked) this.per[rec.c] = (this.per[rec.c] || 0) + 1;
      this.pending.push(rec);
    }
    if (this.pending.length > RING) this.pending.splice(0, this.pending.length - RING);
    if (!this.raf && !S.paused) {
      var self = this;
      this.raf = requestAnimationFrame(function () { self.raf = 0; self.flush(); });
    }
    if (S.paused) counter();
  };

  Pane.prototype.flush = function () {
    var batch = this.pending;
    if (!batch.length || S.paused) return;
    this.pending = [];
    if (this.msg.dataset.t !== "No lines match") this.message("");
    var frag = document.createDocumentFragment(), tail = [];
    var last = this.lines.length ? this.lines[this.lines.length - 1].t : -Infinity;
    for (var k = 0; k < batch.length; k++) {
      var r = batch[k];
      this.counts[r.l]++;
      r.e = this.render(r);
      this.vis(r);
      if (r.t >= last || !MERGED) {
        frag.appendChild(r.e);
        this.lines.push(r);
        last = r.t;
        tail.push(r);
      } else {
        // Out of order (another container of the stack): insert by time.
        if (frag.childNodes.length) { this.box.appendChild(frag); frag = document.createDocumentFragment(); }
        var lo = 0, hi = this.lines.length;
        while (lo < hi) { var mid = (lo + hi) >> 1; if (this.lines[mid].t <= r.t) lo = mid + 1; else hi = mid; }
        this.box.insertBefore(r.e, this.lines[lo] ? this.lines[lo].e : null);
        this.lines.splice(lo, 0, r);
      }
    }
    this.box.appendChild(frag);
    if (this.cutEl) this.box.appendChild(this.cutEl);
    if (!this.follow) this.unseen += batch.length;
    this.trim();
    if (HL && S.q) highlightAll();
    if (this.follow) this.stick();
    this.pill();
    this.olderOk();
    if (S.target) this.seek();
    else if (this.pin && Date.now() < this.pinUntil && this.pin.isConnected) { this.pin.scrollIntoView({ block: "center" }); this.moved(); }
    counter();
  };

  Pane.prototype.trim = function () {
    var over = this.lines.length - RING;
    if (over <= 0) return;
    // Scrolled up: keep the lines in view where they are (set absolutely, so
    // it holds whether or not scroll anchoring already moved the view).
    // Following: no measuring (it would force a layout per batch); the
    // bottom is re-set.
    var j = over;
    while (!this.follow && j < this.lines.length - 1 && this.lines[j].e.hidden) j++;
    var keep = this.lines[j].e, rel = this.follow ? 0 : keep.offsetTop - this.body.scrollTop;
    for (var k = 0; k < over; k++) {
      var r = this.lines[k];
      this.counts[r.l]--;
      if (r.e.parentNode) r.e.parentNode.removeChild(r.e);
      if (r.mk && r.mk.parentNode) r.mk.parentNode.removeChild(r.mk);
    }
    this.lines.splice(0, over);
    if (!this.follow) { this.body.scrollTop = keep.offsetTop - rel; this.moved(); }
  };

  // ---- LG-21: "Load older lines" at the top of a live tail: the lines
  // before the oldest one held, from a stream that ends (`until=`), put in
  // front without moving the reader. Only while the ring has room.
  Pane.prototype.olderOk = function () {
    this.older.hidden = !(S.range === "" && this.lines.length && !this.noOlder && RING - this.lines.length >= 50);
  };
  // The window before the oldest line is guessed from the pace of the lines
  // held (about twice `want` lines); the server reads it from its start and
  // stops at CAP, and says so (`cut` on its end event). A cut window would
  // leave a gap in front of the oldest line: never prepend it, ask for a
  // narrower one (up to six times), then refuse. Too few lines: a wider one,
  // three times.
  var CAP = 5000;
  Pane.prototype.loadOlder = function (span, tries) {
    var self = this, first = this.lines[0], last = this.lines[this.lines.length - 1];
    if (!first || (this.loading && !tries)) return;
    var want = Math.min(TAIL, RING - this.lines.length), got = [], lab = this.older.lastChild, cut = false;
    tries = tries || 0;
    span = span || Math.min(86400, Math.max(60, (last.t - first.t) / 1000 / this.lines.length * want * 2));
    this.loading = true; this.older.disabled = true; lab.textContent = "Loading older lines…";
    var until = first.t / 1000, since = Math.max(0, until - span);
    var es = this.oes = new EventSource("/api/logs/stream?" + this.q + "&since=" + since.toFixed(3) + "&until=" + until.toFixed(3) + "&tail=" + CAP);
    var end = function (ok) {
      es.close(); self.oes = null;
      got = got.filter(function (x) { return x.t < first.t; });
      if (ok && cut && tries < 6) return self.loadOlder(span / 8, tries + 1);
      if (ok && !cut && tries < 3 && got.length < want && since > 0) return self.loadOlder(span * 8, tries + 1);
      self.loading = false; self.older.disabled = false; lab.textContent = "Load older lines";
      if (ok && cut) say("Too many lines just before these to load them here. Pick a custom range instead.");
      else if (ok) self.prepend(got, want, got.length < want);
      else say("Could not load older lines.");
    };
    es.addEventListener("end", function (e) { if (JSON.parse(e.data).cut) cut = true; });
    es.addEventListener("l", function (e) { got = got.concat(JSON.parse(e.data)); });
    es.addEventListener("done", function () { end(true); });
    es.onerror = function () { end(false); };
  };
  Pane.prototype.prepend = function (arr, want, all) {
    var room = Math.min(want, RING - this.lines.length);
    var recs = arr.map(mk).sort(function (a, b) { return a.t - b.t; });
    if (recs.length > room) recs = recs.slice(recs.length - room);
    // Fewer than asked for, even in a wide window: that was all there is.
    if (all) this.noOlder = true;
    if (!recs.length) { this.olderOk(); say("No older lines."); return; }
    var frag = document.createDocumentFragment();
    for (var k = 0; k < recs.length; k++) { var r = recs[k]; this.counts[r.l]++; r.e = this.render(r); frag.appendChild(r.e); }
    this.box.insertBefore(frag, this.box.firstChild);
    this.lines = recs.concat(this.lines);
    this.refilter();
    if (HL && S.q) highlightAll();
    this.olderOk();
    counter();
  };
  // ---- LG-25: in a stack, a container that sent a full tail has older lines
  // than the merged view shows: mark where its history starts.
  Pane.prototype.markStarts = function () {
    if (this.marked) return;
    this.marked = true;
    if (!MERGED || S.range !== "") return;
    var seen = {};
    for (var k = 0; k < this.lines.length; k++) {
      var r = this.lines[k];
      if (seen[r.c] || (this.per[r.c] || 0) < TAIL) continue;
      seen[r.c] = 1;
      var nm = this.names[r.c] || "?", m = el("div", "lmark", nm + ": older lines not loaded (the last " + TAIL.toLocaleString() + " per container)");
      m.title = nm;
      r.mk = m;
      this.box.insertBefore(m, r.e);
    }
  };

  Pane.prototype.render = function (r) {
    var d = el("div", "ll lv-" + r.l + (r.s === 2 ? " se" : ""));
    d.dataset.t = r.t;
    // The level gutter (LG-22: its own class) says its level in words too
    // (A11Y-08), not by colour and a hover title only.
    var g = el("span", "lgut"), lw = LVW[r.l] + (r.s === 2 ? (LVW[r.l] ? " (stderr)" : "stderr") : "");
    if (lw) { g.title = lw; g.setAttribute("role", "img"); g.setAttribute("aria-label", lw); }
    d.appendChild(g);
    if (r.t) d.appendChild(timeEl(r.t));
    if (MERGED) {
      var nm = this.names[r.c] || "?";
      var chip = el("span", "lcn", chipName(nm));
      chip.style.setProperty("--h", hue(nm));
      chip.title = nm;
      d.appendChild(chip);
    }
    var m = el("span", "lm");
    var p = r.p, j = null;
    if (p.charCodeAt(0) === 123 && p.charCodeAt(p.length - 1) === 125) {
      try { j = JSON.parse(p); } catch (e) { j = null; }
    }
    if (j && typeof j === "object" && !Array.isArray(j)) {
      var det = el("details", "ljson"), sum = el("summary");
      var mk = ["msg", "message", "event", "MESSAGE"].filter(function (k) { return typeof j[k] === "string"; })[0];
      // LG-28: a message when there is one; otherwise the summary is the
      // fields alone (no raw JSON above them), or the raw text when no field
      // is a plain value.
      if (mk) sum.appendChild(el("span", "lj-m", j[mk]));
      var keys = Object.keys(j).filter(function (k) { return k !== mk && j[k] !== null && typeof j[k] !== "object"; }).slice(0, 6);
      if (!mk && !keys.length) sum.appendChild(el("span", "lj-m", p.slice(0, 200)));
      for (var i = 0; i < keys.length; i++) {
        var kv = el("span", "lj-kv");
        kv.appendChild(el("i", "", keys[i] + "="));
        kv.appendChild(document.createTextNode(String(j[keys[i]]).slice(0, 80)));
        sum.appendChild(kv);
      }
      det.appendChild(sum);
      det.addEventListener("toggle", function () {
        if (det.open && !det.querySelector("pre")) det.appendChild(el("pre", "lj-pre", JSON.stringify(j, null, 2)));
      });
      m.appendChild(det);
    } else if (r.segs) {
      for (var s = 0; s < r.segs.length; s++) m.appendChild(segNode(r.segs[s]));
    } else {
      m.textContent = p;
    }
    if (r.x) m.appendChild(el("span", "ltr", " [line truncated at 16 KiB]"));
    d.appendChild(m);
    return d;
  };

  // Visibility of one line (levels, "only matching" with context).
  Pane.prototype.vis = function (r) {
    var hit = !!(test && test(r.p));
    r.hit = hit;
    var show = !!S.lv[r.l];
    if (show && S.only && test) {
      if (hit) {
        this.lastMatch = r.n;
        if (S.ctx) {
          for (var k = this.lines.length - 1; k >= 0 && r.n - this.lines[k].n <= S.ctx; k--) this.lines[k].e.hidden = !S.lv[this.lines[k].l];
        }
      } else show = r.n - this.lastMatch <= S.ctx;
    }
    r.e.hidden = !show;
    r.e.classList.toggle("hit", hit && !HL);
  };

  Pane.prototype.refilter = function () {
    var L = this.lines, n = L.length, k, r;
    this.lastMatch = -1e9;
    if (!(S.only && test && S.ctx)) {
      for (k = 0; k < n; k++) this.vis(L[k]);
      return;
    }
    // Context: mark lines within ctx of a match.
    var near = new Uint8Array(n);
    for (k = 0; k < n; k++) {
      r = L[k];
      r.hit = test(r.p);
      if (r.hit) for (var j = Math.max(0, k - S.ctx); j <= Math.min(n - 1, k + S.ctx); j++) near[j] = 1;
    }
    for (k = 0; k < n; k++) {
      r = L[k];
      r.e.hidden = !(S.lv[r.l] && near[k]);
      r.e.classList.toggle("hit", r.hit && !HL);
      if (r.hit) this.lastMatch = r.n;
    }
  };

  Pane.prototype.seek = function () {
    for (var k = 0; k < this.lines.length; k++) {
      var r = this.lines[k];
      if (r.t >= S.target - 0.5) {
        // LG-30: the nearest line is not the moment asked for: say so.
        if (r.t - S.target > 5000) say("No line at " + when(S.target) + ". The next one is at " + when(r.t) + ".");
        r.e.classList.add("lhl");
        this.follow = false;
        r.e.scrollIntoView({ block: "center" });
        this.moved();
        // Lines still arriving move it: keep it in view for a moment.
        this.pin = r.e;
        this.pinUntil = Date.now() + 4000;
        S.target = null;
        this.pill();
        return;
      }
    }
  };

  var panes = [].map.call(root.querySelectorAll(".lpane"), function (n) { return new Pane(n); });

  // ---------------------------------------------------------------- highlight
  function highlightAll() {
    if (!HL) return;
    if (!hlRe) { CSS.highlights.delete("rh-log"); return; }
    var ranges = [], cap = 20000;
    panes.forEach(function (p) {
      // Only lines in (or near) the viewport: cheap however many match.
      var top = p.body.scrollTop - 400, bot = p.body.scrollTop + p.body.clientHeight + 400;
      var lo = 0, hi = p.lines.length;
      while (lo < hi) {
        var mid = (lo + hi) >> 1, j = mid;
        while (j < hi && p.lines[j].e.hidden) j++;
        if (j === hi) hi = mid;
        else if (p.lines[j].e.offsetTop + p.lines[j].e.offsetHeight < top) lo = j + 1;
        else hi = mid;
      }
      for (var k = Math.max(0, lo - 50); k < p.lines.length && ranges.length < cap; k++) {
        var r = p.lines[k];
        if (r.e.offsetTop > bot && !r.e.hidden) break;
        if (!r.hit || r.e.hidden) continue;
        var m = r.e.querySelector(".lm");
        if (!m) continue;
        var w = document.createTreeWalker(m, NodeFilter.SHOW_TEXT), nodes = [], node, total = 0;
        while ((node = w.nextNode())) {
          if (node.parentNode.closest("pre,.ltr,i")) continue;
          nodes.push([node, total]); total += node.length;
        }
        var txt = nodes.map(function (x) { return x[0].data; }).join("");
        hlRe.lastIndex = 0;
        var mm, guard = 0;
        while ((mm = hlRe.exec(txt)) && guard++ < 50) {
          if (!mm[0].length) { hlRe.lastIndex++; continue; }
          var a = mm.index, b = a + mm[0].length, rg = document.createRange(), set = 0;
          for (var i = 0; i < nodes.length; i++) {
            var st = nodes[i][1], en = st + nodes[i][0].length;
            if (!set && a < en) { rg.setStart(nodes[i][0], a - st); set = 1; }
            if (set && b <= en) { rg.setEnd(nodes[i][0], b - st); ranges.push(rg); break; }
          }
        }
      }
    });
    var hl = new Highlight();
    for (var i = 0; i < ranges.length; i++) hl.add(ranges[i]);
    CSS.highlights.set("rh-log", hl);
  }

  // A short note (a toast when the page has them).
  // (The page's own script may run after this one: then once it has.)
  function say(t) {
    var done = false, go = function () { if (!done && window.HB && HB.toast) { done = true; HB.toast(t, { kind: "info" }); } };
    go();
    if (!done) { document.addEventListener("DOMContentLoaded", go); setTimeout(go, 1000); }
  }

  var lastQ = null, findOnLoad = !!S.q; // a search from the address goes to its newest match once the lines are in
  function refilterAll() {
    var t0 = performance.now();
    compile();
    panes.forEach(function (p) { p.refilter(); });
    var ms = performance.now() - t0;
    // Highlight after layout, only what is on screen.
    if (HL) requestAnimationFrame(highlightAll);
    root.dataset.ft = ms.toFixed(1);
    // LG-13: a new search goes to the newest match (when it is not in view
    // already) and is kept in the address.
    var q = S.q + "\n" + S.re + S.cs;
    if (q !== lastQ) {
      lastQ = q;
      keepSearch();
      var m = matches();
      showMatch(m.length ? m[m.length - 1] : null, true);
    } else if (cm && (cm.e.hidden || !cm.e.isConnected || !cm.hit)) showMatch(null);
    counter();
  }

  // Back to every line: search cleared, every level on, "only matching" off.
  function clearFilters() {
    S.q = ""; $("lq").value = ""; S.only = false; S.ctx = 0; $("lshow").value = "";
    LV.forEach(function (l) { S.lv[l] = 1; });
    [].forEach.call(root.querySelectorAll(".lchip"), function (b) { b.setAttribute("aria-pressed", "true"); });
    applyView(); refilterAll();
  }

  // ---------------------------------------------------------------- matches
  // LG-13: the visible matching lines of every pane, oldest first (by time
  // across panes), and the current one (`cm`, outlined).
  var cm = null;
  function matches() {
    var out = [];
    if (!(S.q && test)) return out;
    panes.forEach(function (p) { for (var i = 0; i < p.lines.length; i++) { var r = p.lines[i]; if (r.hit && !r.e.hidden) out.push(r); } });
    if (panes.length > 1) out.sort(function (a, b) { return a.t - b.t || a.n - b.n; });
    return out;
  }
  function paneOf(e) { for (var i = 0; i < panes.length; i++) if (panes[i].box.contains(e)) return panes[i]; return null; }
  // Into view when not fully in view already (`move`); a move by the script,
  // so following stops only when the view really moved.
  function showMatch(r, move) {
    if (cm && cm.e) cm.e.classList.remove("lcur");
    cm = r;
    if (!r) return;
    r.e.classList.add("lcur");
    var p = paneOf(r.e);
    if (!move || !p) return;
    var b = p.body, br = b.getBoundingClientRect(), lr = r.e.getBoundingClientRect();
    if (lr.top >= br.top && lr.bottom <= br.bottom) return;
    var top = b.scrollTop + lr.top - br.top - Math.max(0, (b.clientHeight - lr.height) / 2);
    p.follow = false;
    b.scrollTop = Math.max(0, top);
    p.moved();
    p.pill();
    if (HL) requestAnimationFrame(highlightAll);
  }
  // -1: the older match (Enter), +1: the newer one (Shift+Enter); wraps.
  function step(dir) {
    var m = matches();
    if (!m.length) return;
    var i = m.indexOf(cm);
    if (i < 0) i = dir < 0 ? m.length : -1;
    showMatch(m[(i + dir + m.length) % m.length], true);
    counter();
  }

  function counter() {
    var all = 0, shown = 0, pend = 0, down = false, c = { e: 0, w: 0, i: 0, d: 0, o: 0 };
    var filtered = S.q || S.only || LV.some(function (l) { return !S.lv[l]; });
    panes.forEach(function (p) {
      all += p.lines.length;
      pend += p.pending.length;
      down = down || p.down;
      for (var k = 0; k < LV.length; k++) c[LV[k]] += p.counts[LV[k]];
      var n = p.lines.length;
      if (filtered) { n = 0; for (var i = 0; i < p.lines.length; i++) if (!p.lines[i].e.hidden) n++; }
      shown += n;
      // LG-14: lines there, all hidden by the filters: say so, with the way back.
      var none = p.msg.dataset.t === "No lines match";
      if (p.lines.length && !n && !none) p.message("No lines match", { icon: "search", body: "The search or the level filters hide every line.", act: ["Clear filters", clearFilters] });
      else if (none && (n || !p.lines.length)) p.message("");
    });
    var t = (shown === all ? all.toLocaleString() : shown.toLocaleString() + " of " + all.toLocaleString()) + (all === 1 ? " line" : " lines");
    var qn = "", hits = 0;
    if (S.q && test) {
      panes.forEach(function (p) { for (var i = 0; i < p.lines.length; i++) if (p.lines[i].hit) hits++; });
      t += " · " + hits.toLocaleString() + " match" + (hits === 1 ? "" : "es");
      var m = matches();
      if (cm && !cm.e.isConnected) cm = null;
      qn = m.length ? (cm ? m.indexOf(cm) + 1 : "–") + "/" + m.length : "0/0";
      hits = m.length;
    }
    $("lqn").textContent = qn;
    $("lprev").disabled = $("lnext").disabled = !hits;
    if (S.paused) t += " · paused" + (pend ? ", " + pend + " waiting" : "");
    if (down) t += " · disconnected, retrying";
    if (panes.some(function (p) { return p.cut; })) t += " · first " + RING.toLocaleString() + " of the range";
    // On a phone the counter is in the Filters panel; a paused or broken
    // tail shows it anyway.
    root.classList.toggle("lstate", !!(S.paused || down));
    $("lcount").textContent = $("lcount").title = t;
    for (var k = 0; k < LV.length; k++) {
      var b = root.querySelector('[data-n="' + LV[k] + '"]');
      if (b) b.textContent = c[LV[k]].toLocaleString();
    }
    // The Filters button counts what hides lines (LG-19).
    var f = LV.filter(function (l) { return !S.lv[l]; }).length + (S.only ? 1 : 0) + (S.range ? 1 : 0), fn = $("lfn");
    fn.textContent = f; fn.hidden = !f;
  }

  // ---------------------------------------------------------------- toolbar
  function press(id, on) { var b = $(id); if (b) b.setAttribute("aria-pressed", on ? "true" : "false"); }
  function applyView() {
    root.classList.toggle("nowrap", !S.wrap);
    root.classList.toggle("notime", !S.time);
    press("lwrap", S.wrap); press("ltime", S.time); press("lpause", S.paused);
    press("lre", S.re); press("lcase", S.cs);
    $("lpause").querySelector("span").textContent = S.paused ? "Resume" : "Pause";
  }

  var debounce = 0;
  $("lq").value = S.q;
  $("lq").addEventListener("input", function () {
    clearTimeout(debounce);
    debounce = setTimeout(function () { S.q = $("lq").value; refilterAll(); }, 120);
  });
  $("lq").addEventListener("keydown", function (e) {
    if (e.key === "Escape" && this.value) { this.value = ""; S.q = ""; refilterAll(); e.stopPropagation(); }
    if (e.key === "Enter") {
      e.preventDefault();
      clearTimeout(debounce);
      // A changed query searches; the same one steps (LG-13).
      if (S.q !== this.value) { S.q = this.value; refilterAll(); } else step(e.shiftKey ? 1 : -1);
    }
  });
  $("lprev").addEventListener("click", function () { step(-1); });
  $("lnext").addEventListener("click", function () { step(1); });
  function tog(id, key, after) {
    $(id).addEventListener("click", function () { S[key] = !S[key]; applyView(); (after || refilterAll)(); });
  }
  tog("lre", "re"); tog("lcase", "cs");
  tog("lwrap", "wrap", function () { store("wrap", S.wrap ? "1" : "0"); });
  tog("ltime", "time", function () { store("time", S.time ? "1" : "0"); });
  // LG-29: a paused pane says so in its header, not "live".
  tog("lpause", "paused", function () {
    panes.forEach(function (p) {
      if (!p.st) return;
      if (S.paused) { p.stWas = [p.st.textContent, p.st.dataset.st]; if (p.stWas[1] === "ok") p.status("paused", "warn"); }
      else if (p.stWas && p.st.textContent === "paused") p.status(p.stWas[0], p.stWas[1]);
    });
    if (!S.paused) panes.forEach(function (p) { p.flush(); });
    counter();
  });
  // LG-31: "Only matching" and its context lines are one control.
  $("lshow").addEventListener("change", function () { S.only = this.value !== ""; S.ctx = +this.value || 0; refilterAll(); });
  [].forEach.call(root.querySelectorAll(".lchip"), function (b) {
    b.addEventListener("click", function () {
      var l = b.dataset.lv;
      S.lv[l] = S.lv[l] ? 0 : 1;
      b.setAttribute("aria-pressed", S.lv[l] ? "true" : "false");
      refilterAll();
    });
  });
  // LG-19: on a phone the rest of the toolbar opens under "Filters".
  $("lfilt").addEventListener("click", function () {
    var o = !root.classList.contains("fopen");
    root.classList.toggle("fopen", o);
    this.setAttribute("aria-expanded", o ? "true" : "false");
    fit();
  });

  // LG-32: the button says what it downloads.
  function dlHref() {
    var q = panes.length === 1 ? panes[0].q : "c=" + panes.map(function (p) { return p.q.replace(/^c=/, ""); }).join(",");
    var r = S.range;
    var tail = r === "" ? "&tail=" + RING : "";
    var range = r === "" ? "" : r === "custom" ? (S.from ? "&since=" + S.from : "") + (S.to ? "&until=" + S.to : "") : "&since=" + r;
    var what = r === "" ? "the last " + RING.toLocaleString() + " lines" : r === "custom" ? "the custom range" : $("lrange").selectedOptions[0].textContent.toLowerCase();
    var a = $("ldl");
    a.href = "/api/logs/download?" + q + range + tail;
    a.title = "Download " + what + " as plain text, every line (the search and level filters are not applied)";
  }
  function localToUnix(v) { if (!v) return null; var t = new Date(v).getTime(); return isNaN(t) ? null : Math.floor(t / 1000); }
  function toLocal(ms) { var d = new Date(ms); return d.getFullYear() + "-" + p2(d.getMonth() + 1) + "-" + p2(d.getDate()) + "T" + p2(d.getHours()) + ":" + p2(d.getMinutes()); }
  $("lrange").addEventListener("change", function () {
    S.range = this.value;
    $("lcustom").hidden = S.range !== "custom";
    fit();
    if (S.range === "custom") {
      if (!$("lfrom").value) $("lfrom").value = toLocal(Date.now() - 3600000);
      $("lfrom").focus(); counter(); return;
    }
    dlHref();
    panes.forEach(function (p) { p.reset(); });
  });
  // LG-16: a range that cannot hold lines is refused on the field.
  function rangeErr(t, field) {
    var e = $("lrerr");
    e.textContent = t || ""; e.hidden = !t;
    ["lfrom", "lto"].forEach(function (id) { $(id).setAttribute("aria-invalid", t && $(id) === field ? "true" : "false"); });
    if (field) field.focus();
    fit();
  }
  $("lapply").addEventListener("click", function () {
    var f = localToUnix($("lfrom").value), to = localToUnix($("lto").value), now = Date.now() / 1000;
    if (!f && !to) return rangeErr("Choose a From time, a To time or both.", $("lfrom"));
    if (f && f > now) return rangeErr("From is in the future: choose a moment that has passed.", $("lfrom"));
    if (f && to && to <= f) return rangeErr("To must be later than From.", $("lto"));
    rangeErr("");
    S.from = f; S.to = to;
    dlHref();
    panes.forEach(function (p) { p.reset(); });
  });

  // ---------------------------------------------------------------- line actions
  // LG-20: copy and permalink are real buttons. They show on hover, on the
  // line that was tapped or clicked, and on the line picked with the keys:
  // Enter in a pane picks the current match or the first line in view and
  // focuses "Copy"; Arrow Up / Down move to the line above / below; Esc goes
  // back to the pane.
  function copy(text) {
    if (navigator.clipboard && window.isSecureContext) return navigator.clipboard.writeText(text);
    var ta = el("textarea");
    ta.value = text; ta.style.position = "fixed"; ta.style.opacity = "0";
    document.body.appendChild(ta); ta.select();
    try { document.execCommand("copy"); } catch (e) {}
    document.body.removeChild(ta);
    return Promise.resolve();
  }
  var bar = el("div", "lact");
  bar.setAttribute("role", "group"); bar.setAttribute("aria-label", "Line actions");
  var bCopy = el("button", "btn sm", "Copy"), bLink = el("button", "btn sm", "Link");
  bCopy.type = bLink.type = "button";
  bCopy.title = "Copy this line"; bLink.title = "Copy a link to this line";
  bCopy.setAttribute("aria-label", "Copy line"); bLink.setAttribute("aria-label", "Copy a link to this line");
  bar.appendChild(bCopy); bar.appendChild(bLink);
  var cur = null;
  function findRec(lineEl) {
    for (var i = 0; i < panes.length; i++) {
      var L = panes[i].lines;
      for (var k = L.length - 1; k >= 0; k--) if (L[k].e === lineEl) return L[k];
    }
    return null;
  }
  function attach(l) {
    if (cur && cur !== l) cur.classList.remove("lsel");
    cur = l;
    if (bar.parentNode !== l) l.appendChild(bar);
  }
  function pickLine(l, focus) {
    attach(l);
    l.classList.add("lsel");
    if (focus) (bar.contains(document.activeElement) ? document.activeElement : bCopy).focus({ preventScroll: true });
  }
  root.addEventListener("mouseover", function (e) {
    var l = e.target.closest && e.target.closest(".ll");
    if (!l || l === cur || bar.contains(e.target) || (cur && cur.classList.contains("lsel"))) return;
    attach(l);
  });
  root.addEventListener("click", function (e) {
    var l = e.target.closest && e.target.closest(".ll");
    if (!l || e.target.closest("a,button,summary,input,select,pre") || (window.getSelection && String(getSelection()))) return;
    if (l.classList.contains("lsel")) { l.classList.remove("lsel"); return; }
    pickLine(l, false);
  });
  // The first line that is in view (the one at the top edge counts).
  function firstInView(p) {
    var t = p.body.getBoundingClientRect().top;
    for (var i = 0; i < p.lines.length; i++) { var e = p.lines[i].e; if (!e.hidden && e.getBoundingClientRect().bottom > t + 2) return e; }
    return null;
  }
  panes.forEach(function (p) {
    p.body.addEventListener("keydown", function (e) {
      if (e.key !== "Enter" || e.target !== p.body) return;
      var l = cm && p.box.contains(cm.e) ? cm.e : cur && cur.classList.contains("lsel") && p.box.contains(cur) ? cur : firstInView(p);
      if (l) { e.preventDefault(); pickLine(l, true); }
    });
  });
  bar.addEventListener("keydown", function (e) {
    if (!cur) return;
    var p = paneOf(cur);
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); cur.classList.remove("lsel"); if (p) p.body.focus({ preventScroll: true }); return; }
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    e.preventDefault();
    var n = cur;
    do n = e.key === "ArrowUp" ? n.previousElementSibling : n.nextElementSibling; while (n && (n.hidden || !n.classList.contains("ll")));
    if (!n || !p) return;
    pickLine(n, true);
    var br = p.body.getBoundingClientRect(), lr = n.getBoundingClientRect();
    if (lr.top < br.top) p.body.scrollTop -= br.top - lr.top;
    else if (lr.bottom > br.bottom) p.body.scrollTop += lr.bottom - br.bottom;
  });
  function flash(b, t) { var o = b.textContent; b.textContent = t; setTimeout(function () { b.textContent = o; }, 1200); }
  bCopy.addEventListener("click", function (e) {
    e.stopPropagation();
    var r = cur && findRec(cur);
    if (r) copy(r.p).then(function () { flash(bCopy, "Copied"); });
  });
  bLink.addEventListener("click", function (e) {
    e.stopPropagation();
    var r = cur && findRec(cur);
    if (!r) return;
    var u = location.origin + location.pathname + location.search + "#t=" + r.t.toFixed(3);
    copy(u).then(function () { flash(bLink, "Copied"); });
    history.replaceState(history.state, "", "#t=" + r.t.toFixed(3));
  });

  // ---------------------------------------------------------------- split view
  // LG-07: closing a pane happens in place (the others keep their lines and
  // their place); the link is the same view without it, for a new tab or
  // without the script.
  root.addEventListener("click", function (e) {
    var a = e.target.closest && e.target.closest(".lclose");
    if (!a || panes.length < 2 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || e.button) return;
    e.preventDefault();
    var node = a.closest(".lpane"), i = 0;
    while (i < panes.length && panes[i].node !== node) i++;
    if (i === panes.length) return;
    panes[i].close();
    node.parentNode.removeChild(node);
    panes.splice(i, 1);
    if (cm && !cm.e.isConnected) cm = null;
    var names = panes.map(function (p) { return p.node.dataset.c; }), enc = encodeURIComponent;
    var list = function (except) { return names.filter(function (n) { return n !== except; }).map(enc).join(","); };
    panes.forEach(function (p) { p.node.querySelector(".lclose").href = names.length > 1 ? "/logs?c=" + list(p.node.dataset.c) : "/logs"; });
    var wrap = root.querySelector(".lpanes");
    wrap.style.setProperty("--n", panes.length);
    wrap.classList.toggle("multi", panes.length > 1); wrap.classList.toggle("one", panes.length < 2);
    var add = $("ladd");
    if (add) { add.href = "/logs?sel=" + list(); add.hidden = false; }
    var h1 = document.querySelector(".crumb h1");
    if (h1 && names.length === 1) h1.textContent = names[0];
    try { history.replaceState(history.state, "", "/logs?c=" + list() + location.hash); } catch (err) {}
    keepSearch();
    dlHref(); fit(); counter();
    panes[Math.min(i, panes.length - 1)].body.focus({ preventScroll: true });
  });

  // ---------------------------------------------------------------- keys
  document.addEventListener("keydown", function (e) {
    var t = e.target, typing = t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
    if (e.key === "/" && !typing && !e.ctrlKey && !e.metaKey) {
      e.preventDefault(); e.stopImmediatePropagation();
      $("lq").focus(); $("lq").select();
    } else if (e.key === "End" && !typing) {
      panes.forEach(function (p) { p.toBottom(); });
    }
  }, true);

  // Fill the window with the log panes, no page scroll (LG-33). On a phone
  // a single pane ends above the bottom bar (LG-19); stacked panes keep
  // their own height and the page scrolls between them.
  function fit() {
    var foot = root.querySelector(".lfoot"), phone = window.innerWidth <= 699;
    var bb = document.querySelector(".bbar"), bbH = phone && bb && bb.offsetHeight ? bb.offsetHeight : 0;
    panes.forEach(function (p) {
      if (phone && panes.length > 1) { p.body.style.height = ""; return; }
      var top = p.body.getBoundingClientRect().top + window.scrollY;
      var h = window.innerHeight - top - (foot ? foot.offsetHeight + 10 : 0) - bbH - (phone ? 8 : 18);
      p.body.style.height = Math.max(phone ? 240 : 280, Math.floor(h)) + "px";
    });
    if (phone && panes.length > 1) return;
    // Whatever the page adds below (margins, the room kept for the bottom
    // bar) is taken off again, once.
    // (Measured on `main`: a long sidebar is not ours to fit.)
    var main = root.closest("main"), over = main ? Math.ceil(main.getBoundingClientRect().bottom + window.scrollY - window.innerHeight) : 0;
    if (over > 0) panes.forEach(function (p) { p.body.style.height = Math.max(phone ? 240 : 280, p.body.offsetHeight - over) + "px"; });
    // The height in between may have clamped a pane's scroll: not the
    // reader's move. Put each pane back now, before that scroll event runs.
    panes.forEach(function (p) { if (p.follow) p.stick(); else p.restore(p.at); });
  }
  window.addEventListener("resize", fit);
  // The toolbar grew or lost a row (wider counts, the custom range): fit again.
  if (window.ResizeObserver) {
    var th = 0;
    new ResizeObserver(function (en) { var h = Math.round(en[0].contentRect.height); if (th && h !== th) fit(); th = h; }).observe(root.querySelector(".ltool"));
  }
  var lvl =root.querySelector(".lvl-chart");
  if (lvl) lvl.addEventListener("toggle", fit);
  panes.forEach(function (p) { if (p.chips) p.chips.addEventListener("toggle", fit); });

  // ---------------------------------------------------------------- start
  var h = /^#t=([0-9.]+)/.exec(location.hash);
  if (h) S.target = +h[1];
  // LG-30: a moment still to come is the live tail, said once.
  function future() {
    if (S.target && S.target > Date.now() + 5000) { say("That moment is still to come: showing the live tail."); S.target = null; }
  }
  future();
  // A link to a moment on this same page ("Logs around then" above the
  // Logs tab, or another permalink pasted into the address) reloads the
  // panes around it instead of doing nothing.
  window.addEventListener("hashchange", function () {
    var m = /^#t=([0-9.]+)/.exec(location.hash);
    if (!m) return;
    S.target = +m[1];
    future();
    panes.forEach(function (p) { p.reset(); });
  });
  applyView();
  fit();
  compile();
  if (reFromLink) say("The search from the link is applied as plain text. Turn on .* to use it as a regular expression.");
  lastQ = S.q + "\n" + S.re + S.cs;
  dlHref();
  panes.forEach(function (p) { p.pill(); p.connect(false); });
  window.addEventListener("pagehide", function () { panes.forEach(function (p) { if (p.es) p.es.close(); }); });
  window.__rhLogs = { panes: panes, refilter: refilterAll, state: S, step: step, current: function () { return cm; } };
})();
