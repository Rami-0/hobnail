// Logs pages only. OWNER: logs work.
// Each pane holds a bounded ring of lines (data-ring, default 5000) fed by
// /api/logs/stream (SSE). Log text only ever reaches the DOM through
// textContent / createTextNode: never as HTML.
"use strict";
(function () {
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
    var self = this;
    this.body.addEventListener("scroll", function () {
      var bottom = self.body.scrollHeight - self.body.scrollTop - self.body.clientHeight < 24;
      if (bottom && !self.follow) { self.follow = true; self.unseen = 0; }
      else if (!bottom && self.follow && !self.autoscrolling) self.follow = false;
      self.autoscrolling = false;
      self.pill();
      if (HL && S.q && !self.hlPending) {
        self.hlPending = true;
        requestAnimationFrame(function () { self.hlPending = false; highlightAll(); });
      }
    }, { passive: true });
    this.jump.addEventListener("click", function () { self.toBottom(); self.body.focus(); });
  }

  Pane.prototype.pill = function () {
    this.jump.hidden = this.follow;
    this.jn.textContent = this.unseen ? " · " + this.unseen + " new" : "";
  };

  Pane.prototype.toBottom = function () {
    this.follow = true;
    this.unseen = 0;
    this.autoscrolling = true;
    this.body.scrollTop = this.body.scrollHeight;
    this.pill();
  };

  Pane.prototype.status = function (t, cls) {
    if (!this.st) return;
    this.st.textContent = t;
    this.st.dataset.st = cls || "";
  };

  Pane.prototype.message = function (t) {
    this.msg.textContent = t || "";
    this.msg.hidden = !t;
  };

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
    if (!resume) this.message("Connecting…");
    var es = new EventSource(this.url(resume));
    this.es = es;
    var done = false;
    es.addEventListener("meta", function (e) {
      var m = JSON.parse(e.data);
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
      if (!self.lines.length) self.message(t);
    });
    es.addEventListener("end", function (e) {
      var m = JSON.parse(e.data);
      self.ended[m.c] = 1;
    });
    es.addEventListener("done", function () {
      done = true;
      es.close();
      if (!self.lines.length && !self.msg.hidden && self.msg.textContent === "Connecting…") self.message("No lines in this range.");
      var live = S.range !== "custom" || !S.to;
      self.status(live && !MERGED ? "stopped" : "complete", "off");
      if (!self.lines.length && self.msg.textContent === "Connecting…") self.message("No lines in this range.");
    });
    es.addEventListener("hello", function () {
      if (self.msg.textContent === "Connecting…") self.message("Waiting for lines…");
      if (S.range === "custom" && S.to) return;
    });
    es.onerror = function () {
      if (done) return;
      es.close();
      self.status("reconnecting", "warn");
      clearTimeout(self.retry);
      self.retry = setTimeout(function () { self.connect(true); }, 2000);
    };
  };

  Pane.prototype.reset = function () {
    if (this.es) this.es.close();
    clearTimeout(this.retry);
    this.lines = [];
    this.pending = [];
    this.box.textContent = "";
    this.counts = { e: 0, w: 0, i: 0, d: 0, o: 0 };
    this.lastT = 0;
    this.follow = true;
    this.unseen = 0;
    this.pill();
    this.connect(false);
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
    this.message("");
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
    if (this.follow) { this.autoscrolling = true; this.body.scrollTop = this.body.scrollHeight; }
    this.pill();
    if (S.target) this.seek();
    else if (this.pin && Date.now() < this.pinUntil && this.pin.isConnected) this.pin.scrollIntoView({ block: "center" });
    counter();
  };

  Pane.prototype.trim = function () {
    var over = this.lines.length - RING;
    if (over <= 0) return;
    var keep = this.lines[over].e, before = keep.offsetTop;
    for (var k = 0; k < over; k++) {
      var r = this.lines[k];
      this.counts[r.l]--;
      if (r.e.parentNode) r.e.parentNode.removeChild(r.e);
    }
    this.lines.splice(0, over);
    if (!this.follow) this.body.scrollTop -= before - keep.offsetTop;
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

  function counter() {
    var all = 0, shown = 0, pend = 0, c = { e: 0, w: 0, i: 0, d: 0, o: 0 };
    panes.forEach(function (p) {
      all += p.lines.length;
      pend += p.pending.length;
      for (var k = 0; k < LV.length; k++) c[LV[k]] += p.counts[LV[k]];
      if (S.q || S.only || LV.some(function (l) { return !S.lv[l]; })) {
        for (var i = 0; i < p.lines.length; i++) if (!p.lines[i].e.hidden) shown++;
      } else shown += p.lines.length;
    });
    var t = (shown === all ? all.toLocaleString() : shown.toLocaleString() + " of " + all.toLocaleString()) + (all === 1 ? " line" : " lines");
    if (S.q && test) {
      var hits = 0;
      panes.forEach(function (p) { for (var i = 0; i < p.lines.length; i++) if (p.lines[i].hit) hits++; });
      t += " · " + hits.toLocaleString() + " match" + (hits === 1 ? "" : "es");
    }
    if (S.paused) t += " · paused" + (pend ? ", " + pend + " waiting" : "");
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
  tog("lpause", "paused", function () {
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

  // ---------------------------------------------------------------- start
  var h = /^#t=([0-9.]+)/.exec(location.hash);
  if (h) S.target = +h[1];
  applyView();
  fit();
  compile();
  dlHref();
  panes.forEach(function (p) { p.pill(); p.connect(false); });
  window.addEventListener("pagehide", function () { panes.forEach(function (p) { if (p.es) p.es.close(); }); });
  window.__rhLogs = { panes: panes, refilter: refilterAll, state: S };
})();
