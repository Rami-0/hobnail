// A fake Docker Engine API on a unix socket, good enough for Hobnail's
// read-only docker client (src/docker.rs, src/logs/client.rs): a big,
// deterministic homelab for browser and performance checks, without a real
// daemon. Node 26, no npm packages.
//   node web/e2e/fake-docker.mjs <socket path> [options]
// Options:
//   --n <N>          containers (default 150)
//   --stacks <S>     compose projects stack01..stackSS (default 13)
//   --loose <K>      of the N, K have no compose labels (default 0)
//   --sys <dir>      also write cgroup v2 files under <dir>/fs/cgroup/
//                    system.slice/docker-<id>.scope/ (cpu.stat, memory.current,
//                    memory.stat) every second, so point Hobnail's RH_HOST_SYS
//                    at <dir> and the live CPU / memory columns move
//   --churn <secs>   every <secs>, stop or start svc008 and send the die/stop
//                    or start event, plus a health_status event (default 0: off)
//   --hostile        add 4 containers with long / hostile names, titles, URLs
//                    and images (HTML, quotes, RTL override, javascript: links)
//   --quiet          no request log
// Names: svc001..svcNNN, service = name, project stackMM (round robin).
// About 1 in 7 is stopped (exit codes 0 / 1 / 137 / 143 / 2), a few are
// unhealthy (with a failing health log), one is starting, one restarting,
// one paused, one created. Ports: 0.0.0.0 + :: (the v4/v6 pair Docker
// reports), 127.0.0.1 only, :: only, udp, and unpublished ones.
// Endpoints (an optional /v1.NN prefix is ignored): GET /_ping, /version,
// /info, /containers/json[?all=1], /containers/<id|name>/json, .../top,
// .../logs (Tty, raw, timestamps, tail/since/until, follow), /images/<id>/json,
// /events (chunked, kept open). Anything else: 404 {"message": ...}.
// Run it in a container, e.g.
//   docker run -d --name <n> -u "$(id -u):$(id -g)" -v "$PWD":/repo:ro \
//     -v <dir>:/fake node:26-alpine node /repo/web/e2e/fake-docker.mjs \
//     /fake/docker.sock --sys /fake/sys
// (web/e2e/fake-instance.sh does that and starts Hobnail against it).
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";

const args = process.argv.slice(2);
const SOCK = args[0];
if (!SOCK || SOCK.startsWith("--")) {
  console.error("usage: node web/e2e/fake-docker.mjs <socket path> [--n 150] [--stacks 13] [--loose 0] [--sys <dir>] [--churn <secs>] [--hostile] [--quiet]");
  process.exit(2);
}
const opt = (name, def) => { const i = args.indexOf("--" + name); return i > 0 && args[i + 1] !== undefined ? args[i + 1] : def; };
const flag = (name) => args.includes("--" + name);
const N = Number(opt("n", 150));
const STACKS = Number(opt("stacks", 13));
const LOOSE = Number(opt("loose", 0));
const SYS = opt("sys", null);
const CHURN = Number(opt("churn", 0));
const HOSTILE = flag("hostile");
const LOG_FIXTURES = flag("log-fixtures");
const QUIET = flag("quiet");

const T0 = Math.floor(Date.now() / 1000);
const hex = (s) => crypto.createHash("sha256").update(s).digest("hex");
const pad = (n, w) => String(n).padStart(w, "0");
const rfc = (unix) => new Date(unix * 1000).toISOString().replace(/\.\d+Z$/, ".123456789Z");
const ZERO_TIME = "0001-01-01T00:00:00Z";
const IMAGES = ["nginx:1.27-alpine", "postgres:16-alpine", "redis:7.4", "ghcr.io/example/app:1.2.3", "grafana/grafana:11.3.0",
  "registry.example.com/team/worker:2026.11.1", "sha256:" + hex("untagged").slice(0, 64), "caddy:2", "eclipse-mosquitto:2", "busybox:1.37"];

// ---- the containers ----
// state: running | exited | created | restarting | paused; health: null | healthy | unhealthy | starting
function make(i) {
  const n = i + 1;
  const name = `svc${pad(n, 3)}`;
  const loose = i >= N - LOOSE;
  const project = loose ? null : `stack${pad((i % STACKS) + 1, 2)}`;
  let state = "running", health = null, exit = null;
  if (i % 7 === 3) { state = "exited"; exit = [0, 1, 137, 143, 2][Math.floor(i / 7) % 5]; }
  else if (i % 3 === 0) health = "healthy";
  if (i % 37 === 5) { state = "running"; health = "unhealthy"; exit = null; }
  if (i === 10) { state = "running"; health = "starting"; }
  if (i === 20) { state = "restarting"; exit = 1; health = null; }
  if (i === 30) { state = "paused"; health = null; }
  if (i === 45) { state = "created"; exit = null; health = null; }
  const created = T0 - 86400 * ((i % 40) + 1) - i * 61;
  const started = state === "created" ? null : T0 - 600 - (i * 7919) % (86400 * 9);
  const labels = {};
  if (project) Object.assign(labels, {
    "com.docker.compose.project": project, "com.docker.compose.service": name, "com.docker.compose.oneoff": "False",
    "com.docker.compose.container-number": "1", "com.docker.compose.project.working_dir": `/srv/${project}`,
  });
  if (i % 10 === 0) labels["hobnail.name"] = `Service ${n}`;
  if (i % 17 === 0) labels["org.opencontainers.image.source"] = "https://example.com/source";
  // ports: [IP, PrivatePort, PublicPort|undefined, Type]
  const ports = [];
  switch (i % 5) {
    case 0: ports.push(["0.0.0.0", 80, 8000 + n, "tcp"], ["::", 80, 8000 + n, "tcp"]); break;
    case 1: ports.push(["127.0.0.1", 8080, 9000 + n, "tcp"]); break;
    case 2: ports.push(["::", 443, 10000 + n, "tcp"], ["0.0.0.0", 5353, 11000 + n, "udp"]); break;
    case 3: ports.push(["", 5432, undefined, "tcp"]); break;
    case 4: ports.push(["0.0.0.0", 3000, 12000 + n, "tcp"], ["::", 3000, 12000 + n, "tcp"], ["127.0.0.1", 9090, 13000 + n, "tcp"]); break;
  }
  return { id: hex("fake-container-" + name), name, image: IMAGES[i % IMAGES.length], project, labels, state, health, exit, created, started, ports, i,
    cpuBase: i % 25 === 0 ? 70 : ((i * 7) % 40) / 10, memBase: (20 + ((i * 131) % 1980)) * 1024 * 1024 };
}
const containers = [];
for (let i = 0; i < N; i++) containers.push(make(i));
if (HOSTILE) {
  const h = [
    { name: "svc-" + "x".repeat(120), title: "A very long display name ".repeat(6).trim() },
    { name: "svc-html", title: `<img src=x onerror="alert(1)"><script>alert(2)</script> "quoted" & 'single'` },
    { name: "svc-rtl", title: "Ünïcödé ‮desrever‬ 名前 🚀", url: "javascript:alert(3)" },
    { name: "svc-long-image", title: null, image: "registry.example.com/" + "deep/".repeat(20) + "image:" + "t".repeat(100) },
  ];
  h.forEach((x, k) => {
    const c = make(N + k);
    c.name = x.name; c.id = hex("fake-container-" + x.name);
    c.labels["com.docker.compose.service"] = x.name;
    c.project = c.labels["com.docker.compose.project"] = "stack-with-a-rather-long-project-name-for-wrapping";
    if (x.title) c.labels["hobnail.name"] = x.title;
    if (x.url) c.labels["hobnail.url"] = x.url;
    if (x.image) c.image = x.image;
    containers.push(c);
  });
}
const byRef = (ref) => {
  ref = decodeURIComponent(ref);
  return containers.find((c) => c.id === ref || c.name === ref || c.name === ref.replace(/^\//, "") || (ref.length >= 4 && /^[0-9a-f]+$/.test(ref) && c.id.startsWith(ref)));
};
const running = (c) => c.state === "running" || c.state === "paused" || c.state === "restarting";
const ago = (secs) => {
  if (secs < 60) return `${secs} seconds`;
  if (secs < 3600) return `${Math.floor(secs / 60)} minutes`;
  if (secs < 86400 * 2) return `${Math.floor(secs / 3600)} hours`;
  return `${Math.floor(secs / 86400)} days`;
};
function statusText(c) {
  const now = Math.floor(Date.now() / 1000);
  switch (c.state) {
    case "exited": return `Exited (${c.exit}) ${ago(Math.max(1, now - c.started - 300))} ago`;
    case "created": return "Created";
    case "restarting": return `Restarting (${c.exit}) 5 seconds ago`;
    case "paused": return `Up ${ago(now - c.started)} (Paused)`;
    default: {
      const h = c.health === "healthy" ? " (healthy)" : c.health === "unhealthy" ? " (unhealthy)" : c.health === "starting" ? " (health: starting)" : "";
      return `Up ${ago(now - c.started)}${h}`;
    }
  }
}
function listItem(c) {
  return {
    Id: c.id, Names: ["/" + c.name], Image: c.image, ImageID: "sha256:" + hex("img-" + c.image), Command: "/entrypoint.sh",
    Created: c.created, Labels: c.labels, State: c.state, Status: statusText(c),
    Ports: running(c) ? c.ports.map(([ip, pp, pub, t]) => (pub === undefined ? { PrivatePort: pp, Type: t } : { IP: ip, PrivatePort: pp, PublicPort: pub, Type: t })) : [],
    ...(c.health ? { Health: { Status: c.health, FailingStreak: c.health === "unhealthy" ? 4 : 0 } } : {}),
    HostConfig: { NetworkMode: c.project ? `${c.project}_default` : "bridge" },
    NetworkSettings: { Networks: {} }, Mounts: [],
  };
}
function healthLog(c) {
  const now = Math.floor(Date.now() / 1000);
  const bad = c.health === "unhealthy";
  return Array.from({ length: 5 }, (_, k) => ({
    Start: rfc(now - (5 - k) * 30), End: rfc(now - (5 - k) * 30 + 1),
    ExitCode: bad ? 1 : 0,
    Output: bad ? `curl: (7) Failed to connect to localhost port 80 after 0 ms: Connection refused\n` : "ok\n",
  }));
}
function inspect(c) {
  const isRun = running(c);
  const networks = {};
  networks[c.project ? `${c.project}_default` : "bridge"] = {
    IPAddress: `172.18.${Math.floor(c.i / 250)}.${(c.i % 250) + 2}`, GlobalIPv6Address: "", Gateway: "172.18.0.1",
    MacAddress: "02:42:ac:12:00:" + pad((c.i % 99).toString(16), 2), Aliases: [c.name, c.labels["com.docker.compose.service"] || c.name],
  };
  return {
    Id: c.id, Name: "/" + c.name, Created: rfc(c.created), Image: "sha256:" + hex("img-" + c.image), RestartCount: c.state === "restarting" ? 7 : c.i % 4,
    Path: "/entrypoint.sh", Args: [],
    Config: {
      Hostname: c.id.slice(0, 12), Image: c.image, Cmd: ["serve", "--port", "80"], Entrypoint: ["/entrypoint.sh"], WorkingDir: "/app", User: c.i % 2 ? "1000:1000" : "",
      Env: ["PATH=/usr/local/bin:/usr/bin:/bin", `SERVICE_NAME=${c.name}`, "API_TOKEN=fake-not-a-secret", "TZ=UTC"],
      Tty: true, Labels: c.labels,
      Healthcheck: c.health ? { Test: ["CMD-SHELL", "curl -fsS http://localhost/ || exit 1"], Interval: 30e9, Timeout: 5e9, StartPeriod: 10e9, Retries: 3 } : undefined,
    },
    State: {
      Status: c.state, Running: isRun, Paused: c.state === "paused", Restarting: c.state === "restarting", OOMKilled: c.exit === 137 && c.i % 2 === 0, Dead: false,
      Pid: isRun ? 1000 + c.i : 0, ExitCode: c.exit ?? 0, Error: "",
      StartedAt: c.started ? rfc(c.started) : ZERO_TIME, FinishedAt: c.state === "exited" ? rfc(c.started + 300) : ZERO_TIME,
      ...(c.health ? { Health: { Status: c.health, FailingStreak: c.health === "unhealthy" ? 4 : 0, Log: healthLog(c) } } : {}),
    },
    HostConfig: {
      NetworkMode: c.project ? `${c.project}_default` : "bridge", RestartPolicy: { Name: "unless-stopped", MaximumRetryCount: 0 },
      Memory: c.i % 6 === 0 ? 512 * 1024 * 1024 : 0, MemoryReservation: 0, MemorySwap: 0, NanoCpus: c.i % 9 === 0 ? 1e9 : 0,
      CpuShares: 0, CpuQuota: 0, CpuPeriod: 0, CpusetCpus: "", PidsLimit: null, Privileged: false, ReadonlyRootfs: false, CapAdd: null,
    },
    NetworkSettings: { Networks: networks },
    Mounts: c.i % 3 === 0 ? [{ Type: "volume", Name: `${c.name}_data`, Source: `/var/lib/docker/volumes/${c.name}_data/_data`, Destination: "/data", Mode: "z", RW: true }] : [],
  };
}

// ---- cgroup files (live CPU / memory) ----
const usage = new Map(); // id -> usage_usec
let lastTick = Date.now();
function writeAtomic(file, text) { const tmp = file + ".tmp"; fs.writeFileSync(tmp, text); fs.renameSync(tmp, file); }
function writeSys() {
  if (!SYS) return;
  const now = Date.now(), dt = (now - lastTick) / 1000; lastTick = now;
  const t = now / 1000;
  for (const c of containers) {
    const dir = path.join(SYS, "fs/cgroup/system.slice", `docker-${c.id}.scope`);
    if (!running(c) || c.state === "paused") { if (!running(c)) fs.rmSync(dir, { recursive: true, force: true }); continue; }
    fs.mkdirSync(dir, { recursive: true });
    const pct = Math.max(0, c.cpuBase * (1 + 0.5 * Math.sin(t / 7 + c.i)));
    const u = (usage.get(c.id) || (c.i + 1) * 1_000_000) + Math.round((pct / 100) * 1e6 * dt);
    usage.set(c.id, u);
    const mem = Math.round(c.memBase * (1 + 0.1 * Math.sin(t / 11 + c.i)));
    writeAtomic(path.join(dir, "cpu.stat"), `usage_usec ${u}\nuser_usec ${Math.round(u * 0.7)}\nsystem_usec ${Math.round(u * 0.3)}\nnr_periods 0\nnr_throttled 0\nthrottled_usec 0\n`);
    writeAtomic(path.join(dir, "memory.current"), `${mem + 4 * 1024 * 1024}\n`);
    writeAtomic(path.join(dir, "memory.stat"), `anon ${mem}\nfile ${8 * 1024 * 1024}\ninactive_file ${4 * 1024 * 1024}\nactive_file ${4 * 1024 * 1024}\n`);
  }
}
if (SYS) { fs.mkdirSync(path.join(SYS, "fs/cgroup/system.slice"), { recursive: true }); writeSys(); setInterval(writeSys, 1000).unref(); }

// ---- events ----
const streams = new Set(); // { res, filter: Set<string>|null }
function emit(c, action, extra = {}) {
  const now = Date.now();
  const line = JSON.stringify({
    status: action.split(":")[0], id: c.id, from: c.image, Type: "container", Action: action,
    Actor: { ID: c.id, Attributes: { name: c.name, image: c.image, ...c.labels, ...extra } },
    scope: "local", time: Math.floor(now / 1000), timeNano: now * 1e6,
  }) + "\n";
  for (const s of streams) if (!s.filter || s.filter.has(action.split(":")[0])) s.res.write(line);
}
if (CHURN > 0) {
  const victim = containers[7]; // svc008
  let k = 0;
  setInterval(() => {
    k++;
    if (running(victim)) {
      victim.state = "exited"; victim.exit = 137; emit(victim, "kill", { signal: "15" }); emit(victim, "die", { exitCode: "137" }); emit(victim, "stop");
    } else {
      victim.state = "running"; victim.exit = null; victim.started = Math.floor(Date.now() / 1000); emit(victim, "start");
    }
    const sick = containers[5]; // svc006: unhealthy <-> healthy
    if (sick) { sick.health = k % 2 ? "healthy" : "unhealthy"; emit(sick, `health_status: ${sick.health}`); }
  }, CHURN * 1000).unref();
}

// ---- logs ----
const LEVELS = ["INFO", "INFO", "INFO", "DEBUG", "WARN", "INFO", "ERROR"];
const logLine = (c, unix, k) => {
  const sample = k % 10 === 4 ? '<script id="hostile-log">globalThis.logPwned=1</script><img id="hostile-img" src=x onerror=alert(1)> & hostile text'
    : k % 10 === 5 ? '\x1b[1;31mERROR ANSI red\x1b[0m plain https://example.com/log'
    : k % 10 === 6 ? JSON.stringify({msg: "JSON request", request: k, ok: true, nested: {safe: "<b>text</b>"}})
    : `${LEVELS[k % LEVELS.length]} ${c.name}: request ${k} handled in ${(k * 37) % 900} ms`;
  return `${new Date(unix * 1000).toISOString().replace("Z", "000000Z")} ${sample}\n`;
};
const logConnections = [];
const logReport = path.join(path.dirname(SOCK), "log-connections.json");
const saveLogReport = () => fs.writeFileSync(logReport, JSON.stringify(logConnections));

function logs(c, q, res) {
  if (LOG_FIXTURES && c.name === "svc149") return json(res, 404, { message: "Container disappeared" });
  if (LOG_FIXTURES && c.name === "svc148") return json(res, 500, { message: "Fixture log read failure" });
  const now = Math.floor(Date.now() / 1000);
  const since = q.get("since") ? parseFloat(q.get("since")) : 0, until = q.get("until") ? parseFloat(q.get("until")) : Infinity;
  let lines = [];
  for (let k = 0; k < 5000; k++) { const at = now - (5000 - k) * 2; if (at >= since && at <= until) lines.push(logLine(c, at, k)); }
  const tail = q.get("tail");
  if (tail && tail !== "all") lines = Number(tail) > 0 ? lines.slice(-Number(tail)) : [];
  res.writeHead(200, { "Content-Type": "application/vnd.docker.raw-stream", "Api-Version": "1.47" });
  res.write(lines.join(""));
  if (q.get("follow") === "1" && running(c)) {
    const connection = { name: c.name, opened: Date.now(), closed: null };
    logConnections.push(connection); saveLogReport();
    let k = 5000;
    const burst = setTimeout(() => {
      if (c.name === "svc001") { connection.burstAt = Date.now(); saveLogReport(); res.write(Array.from({ length: 1500 }, (_, i) => logLine(c, Date.now() / 1000 + i / 10000, k++)).join("")); }
    }, 6000);
    const t = setInterval(() => res.write(logLine(c, Math.floor(Date.now() / 1000), k++)), 2000);
    res.on("close", () => { clearInterval(t); clearTimeout(burst); connection.closed = Date.now(); saveLogReport(); });
  } else res.end();
}

// ---- the server ----
const json = (res, status, body) => {
  const b = JSON.stringify(body);
  res.writeHead(status, { "Content-Type": "application/json", "Api-Version": "1.47", "Content-Length": Buffer.byteLength(b) });
  res.end(b);
};
const server = http.createServer((req, res) => {
  const u = new URL(req.url, "http://docker");
  const p = u.pathname.replace(/^\/v1\.\d+/, "");
  if (!QUIET) console.log(`${new Date().toISOString()} ${req.method} ${req.url}`);
  if (req.method !== "GET" && req.method !== "HEAD") return json(res, 405, { message: "fake-docker is read-only" });
  let m;
  if (p === "/_ping") { res.writeHead(200, { "Content-Type": "text/plain", "Api-Version": "1.47" }); return res.end("OK"); }
  if (p === "/version") return json(res, 200, { Version: "29.0.0-fake", ApiVersion: "1.47", MinAPIVersion: "1.24", Os: "linux", Arch: "amd64", KernelVersion: "6.0.0-fake" });
  if (p === "/info") return json(res, 200, { Containers: containers.length, ContainersRunning: containers.filter(running).length, Name: "fake-docker", ServerVersion: "29.0.0-fake" });
  if (p === "/containers/json") {
    const all = ["1", "true"].includes(u.searchParams.get("all") || "");
    return json(res, 200, containers.filter((c) => all || running(c)).map(listItem));
  }
  if ((m = p.match(/^\/containers\/([^/]+)\/(json|top|logs)$/))) {
    const c = byRef(m[1]);
    if (!c) return json(res, 404, { message: `No such container: ${decodeURIComponent(m[1])}` });
    if (m[2] === "json") {
      if (LOG_FIXTURES && c.name === "svc149") return json(res, 404, { message: "Container disappeared before inspect" });
      return json(res, 200, inspect(c));
    }
    if (m[2] === "top") {
      if (!running(c)) return json(res, 409, { message: `container ${c.id} is not running` });
      const custom = (u.searchParams.get("ps_args") || "").includes("%cpu");
      return json(res, 200, custom
        ? { Titles: ["PID", "USER", "%CPU", "%MEM", "RSS", "ELAPSED", "COMMAND"], Processes: [[String(1000 + c.i), "root", "0.3", "1.2", "20480", "01:02:03", "/entrypoint.sh serve --port 80"], [String(2000 + c.i), "app", "1.1", "3.4", "61440", "01:02:01", "worker --threads 4"]] }
        : { Titles: ["UID", "PID", "PPID", "C", "STIME", "TTY", "TIME", "CMD"], Processes: [["root", String(1000 + c.i), "1", "0", "10:00", "?", "00:00:01", "/entrypoint.sh serve"]] });
    }
    return logs(c, u.searchParams, res);
  }
  if ((m = p.match(/^\/images\/(.+)\/json$/))) {
    const ref = decodeURIComponent(m[1]);
    const c = containers.find((x) => "sha256:" + hex("img-" + x.image) === ref || x.image === ref);
    if (!c) return json(res, 404, { message: `No such image: ${ref}` });
    const repo = c.image.startsWith("sha256:") ? [] : [c.image.replace(/:[^:/]+$/, "") + "@sha256:" + hex("digest-" + c.image)];
    return json(res, 200, { Id: ref, RepoTags: c.image.startsWith("sha256:") ? [] : [c.image], RepoDigests: repo });
  }
  if (p === "/events") {
    let filter = null;
    try {
      const f = JSON.parse(u.searchParams.get("filters") || "{}");
      if (Array.isArray(f.event)) filter = new Set(f.event);
    } catch {}
    res.writeHead(200, { "Content-Type": "application/json", "Api-Version": "1.47" });
    res.flushHeaders();
    const s = { res, filter };
    streams.add(s);
    res.on("close", () => streams.delete(s));
    return;
  }
  json(res, 404, { message: "page not found" });
});
server.keepAliveTimeout = 5000;
try { fs.unlinkSync(SOCK); } catch {}
fs.mkdirSync(path.dirname(SOCK), { recursive: true });
server.listen(SOCK, () => {
  try { fs.chmodSync(SOCK, 0o666); } catch {}
  const run = containers.filter(running).length;
  console.log(`fake-docker: ${containers.length} containers (${run} running, ${containers.length - run} not), ${STACKS} stacks, on ${SOCK}${SYS ? `, cgroup files under ${SYS}` : ""}${CHURN ? `, churn every ${CHURN}s` : ""}`);
});
function stop() {
  for (const s of streams) s.res.end();
  server.close();
  try { fs.unlinkSync(SOCK); } catch {}
  if (SYS) try { fs.rmSync(path.join(SYS, "fs"), { recursive: true, force: true }); } catch {}
  process.exit(0);
}
process.on("SIGTERM", stop);
process.on("SIGINT", stop);

// Reachable only inside the fake instance's shared network namespace.
http.createServer((req, res) => {
  if (req.url === "/fixture/health") return res.end("ok");
  if (req.url === "/fixture/session") {
    res.setHeader("Content-Type", "application/json");
    return res.end(JSON.stringify({ owner_exists: true }));
  }
  res.writeHead(404).end();
}).listen(8081, "127.0.0.1");
