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
      try { history.replaceState(history.state, "", v ? "?q=" + encodeURIComponent(v) : location.pathname); } catch (e) {}
    };
    var ticks = function () {
      var n = pick.querySelectorAll("input[name=c]:checked").length;
      go.disabled = n < 2;
      go.title = n < 2 ? "Tick two to four containers" : "Show " + n + " side by side";
      [].forEach.call(pick.querySelectorAll("input[name=c]:not(:checked)"), function (b) { b.disabled = n >= 4; });
    };
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
  var LV = ["e", "w", "i", "d", "o"];
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
  var today = new Date().toDateString();
  function timeEl(ms) {
    var d = new Date(ms);
    var t = p2(d.getHours()) + ":" + p2(d.getMinutes()) + ":" + p2(d.getSeconds());
    if (d.toDateString() !== today) t = (d.getMonth() + 1) + "-" + p2(d.getDate()) + " " + t;
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
    box.title = err ? "Invalid regular expression: " + err : "";
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
    // Whatever changes the height (a batch, Wrap, Time, a filter, a resize,
    // a JSON line opened) is answered after layout and before paint:
    // following, the bottom is set again; scrolled up, the reader's line is
    // put back where it was. One observer per pane; it fires only on a change.
    if (window.ResizeObserver) {
      var ro = new ResizeObserver(function () { if (self.follow) self.stick(); else self.restore(self.at); });
      ro.observe(this.box);
      ro.observe(this.body);
    }
  }

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
    es.addEventListener("end", function (e) {
      var m = JSON.parse(e.data);
      self.ended[m.c] = 1;
    });
    es.addEventListener("done", function () {
      done = true;
      es.close();
      var live = S.range !== "custom" || !S.to;
      self.status(live && !MERGED ? "stopped" : "complete", "off");
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
    this.pill();
    this.connect(false);
    counter();
  };

  Pane.prototype.add = function (arr) {
    for (var k = 0; k < arr.length; k++) {
      var x = arr[k];
      var rec = { t: x.t, c: x.c, s: x.s || 1, l: x.l || "o", m: x.m, x: x.x, n: ++seq, e: null };
      rec.p = x.m.indexOf("\x1b") >= 0 ? null : x.m.replace(CTRL, "");
      if (rec.p === null) { rec.segs = ansi(x.m); rec.p = rec.segs.map(function (s) { return s.t; }).join(""); }
      if (rec.t > this.lastT) this.lastT = rec.t;
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
    if (!this.follow) this.unseen += batch.length;
    this.trim();
    if (HL && S.q) highlightAll();
    if (this.follow) this.stick();
    this.pill();
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
    }
    this.lines.splice(0, over);
    if (!this.follow) { this.body.scrollTop = keep.offsetTop - rel; this.moved(); }
  };

  Pane.prototype.render = function (r) {
    var d = el("div", "ll lv-" + r.l + (r.s === 2 ? " se" : ""));
    d.dataset.t = r.t;
    var g = el("span", "lg");
    g.title = { e: "error", w: "warning", i: "info", d: "debug", o: "" }[r.l] + (r.s === 2 ? " (stderr)" : "");
    d.appendChild(g);
    if (r.t) d.appendChild(timeEl(r.t));
    if (MERGED) {
      var nm = this.names[r.c] || "?";
      var chip = el("span", "lcn", shortName(nm));
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
      var msg = j.msg || j.message || j.event || j.MESSAGE;
      sum.appendChild(el("span", "lj-m", typeof msg === "string" ? msg : p.slice(0, 200)));
      var keys = Object.keys(j).filter(function (k) { return k !== "msg" && k !== "message"; }).slice(0, 6);
      for (var i = 0; i < keys.length; i++) {
        var v = j[keys[i]];
        if (v !== null && typeof v === "object") continue;
        var kv = el("span", "lj-kv");
        kv.appendChild(el("i", "", keys[i] + "="));
        kv.appendChild(document.createTextNode(String(v).slice(0, 80)));
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

  function refilterAll() {
    var t0 = performance.now();
    compile();
    panes.forEach(function (p) { p.refilter(); });
    var ms = performance.now() - t0;
    // Highlight after layout, only what is on screen.
    if (HL) requestAnimationFrame(highlightAll);
    root.dataset.ft = ms.toFixed(1);
    counter();
  }

  // Back to every line: search cleared, every level on, "only matching" off.
  function clearFilters() {
    S.q = ""; $("lq").value = ""; S.only = false;
    LV.forEach(function (l) { S.lv[l] = 1; });
    [].forEach.call(root.querySelectorAll(".lchip"), function (b) { b.setAttribute("aria-pressed", "true"); });
    applyView(); refilterAll();
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
    if (S.q && test) {
      var hits = 0;
      panes.forEach(function (p) { for (var i = 0; i < p.lines.length; i++) if (p.lines[i].hit) hits++; });
      t += " · " + hits.toLocaleString() + " match" + (hits === 1 ? "" : "es");
    }
    if (S.paused) t += " · paused" + (pend ? ", " + pend + " waiting" : "");
    if (down) t += " · disconnected, retrying";
    $("lcount").textContent = t;
    for (var k = 0; k < LV.length; k++) {
      var b = root.querySelector('[data-n="' + LV[k] + '"]');
      if (b) b.textContent = c[LV[k]].toLocaleString();
    }
  }

  // ---------------------------------------------------------------- toolbar
  function press(id, on) { var b = $(id); if (b) b.setAttribute("aria-pressed", on ? "true" : "false"); }
  function applyView() {
    root.classList.toggle("nowrap", !S.wrap);
    root.classList.toggle("notime", !S.time);
    press("lwrap", S.wrap); press("ltime", S.time); press("lpause", S.paused);
    press("lre", S.re); press("lcase", S.cs); press("lonly", S.only);
    $("lpause").querySelector("span").textContent = S.paused ? "Resume" : "Pause";
  }

  var debounce = 0;
  $("lq").addEventListener("input", function () {
    clearTimeout(debounce);
    debounce = setTimeout(function () { S.q = $("lq").value; refilterAll(); }, 120);
  });
  $("lq").addEventListener("keydown", function (e) {
    if (e.key === "Escape" && this.value) { this.value = ""; S.q = ""; refilterAll(); e.stopPropagation(); }
    if (e.key === "Enter") { clearTimeout(debounce); S.q = this.value; refilterAll(); }
  });
  function tog(id, key, after) {
    $(id).addEventListener("click", function () { S[key] = !S[key]; applyView(); (after || refilterAll)(); });
  }
  tog("lre", "re"); tog("lcase", "cs"); tog("lonly", "only");
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
  $("lctx").addEventListener("change", function () { S.ctx = +this.value; refilterAll(); });
  [].forEach.call(root.querySelectorAll(".lchip"), function (b) {
    b.addEventListener("click", function () {
      var l = b.dataset.lv;
      S.lv[l] = S.lv[l] ? 0 : 1;
      b.setAttribute("aria-pressed", S.lv[l] ? "true" : "false");
      refilterAll();
    });
  });

  function dlHref() {
    var q = panes.length === 1 ? panes[0].q : "c=" + panes.map(function (p) { return p.q.replace(/^c=/, ""); }).join(",");
    var r = S.range;
    var tail = r === "" ? "&tail=" + RING : "";
    var range = r === "" ? "" : r === "custom" ? (S.from ? "&since=" + S.from : "") + (S.to ? "&until=" + S.to : "") : "&since=" + r;
    $("ldl").href = "/api/logs/download?" + q + range + tail;
  }
  function localToUnix(v) { if (!v) return null; var t = new Date(v).getTime(); return isNaN(t) ? null : Math.floor(t / 1000); }
  $("lrange").addEventListener("change", function () {
    S.range = this.value;
    $("lcustom").hidden = S.range !== "custom";
    if (S.range === "custom") { $("lfrom").focus(); return; }
    dlHref();
    panes.forEach(function (p) { p.reset(); });
  });
  $("lapply").addEventListener("click", function () {
    S.from = localToUnix($("lfrom").value);
    S.to = localToUnix($("lto").value);
    dlHref();
    panes.forEach(function (p) { p.reset(); });
  });

  // ---------------------------------------------------------------- line actions
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
  var bCopy = el("button", "btn sm", "Copy"), bLink = el("button", "btn sm", "Link");
  bCopy.type = bLink.type = "button";
  bCopy.title = "Copy this line"; bLink.title = "Copy a link to this line";
  bar.appendChild(bCopy); bar.appendChild(bLink);
  var cur = null;
  function findRec(lineEl) {
    for (var i = 0; i < panes.length; i++) {
      var L = panes[i].lines;
      for (var k = L.length - 1; k >= 0; k--) if (L[k].e === lineEl) return L[k];
    }
    return null;
  }
  root.addEventListener("mouseover", function (e) {
    var l = e.target.closest && e.target.closest(".ll");
    if (!l || l === cur || bar.contains(e.target)) return;
    cur = l;
    l.appendChild(bar);
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
    history.replaceState(null, "", "#t=" + r.t.toFixed(3));
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

  // Fill the viewport with the log panes on wide screens (no page scroll).
  function fit() {
    var foot = root.querySelector(".lfoot");
    panes.forEach(function (p) {
      if (window.innerWidth <= 700) { p.body.style.height = ""; return; }
      var top = p.body.getBoundingClientRect().top + window.scrollY;
      var h = window.innerHeight - top - (foot ? foot.offsetHeight + 10 : 0) - 18;
      p.body.style.height = Math.max(280, h) + "px";
    });
  }
  window.addEventListener("resize", fit);
  var lvl = root.querySelector(".lvl-chart");
  if (lvl) lvl.addEventListener("toggle", fit);

  // ---------------------------------------------------------------- start
  var h = /^#t=([0-9.]+)/.exec(location.hash);
  if (h) S.target = +h[1];
  // A link to a moment on this same page ("Logs around then" above the
  // Logs tab) reloads the panes around it instead of doing nothing.
  window.addEventListener("hashchange", function () {
    var m = /^#t=([0-9.]+)/.exec(location.hash);
    if (!m) return;
    S.target = +m[1];
    panes.forEach(function (p) { p.reset(); });
  });
  applyView();
  fit();
  compile();
  dlHref();
  panes.forEach(function (p) { p.pill(); p.connect(false); });
  window.addEventListener("pagehide", function () { panes.forEach(function (p) { if (p.es) p.es.close(); }); });
  window.__rhLogs = { panes: panes, refilter: refilterAll, state: S };
})();
