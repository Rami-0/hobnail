// Fake provider APIs for browser checks of provider widgets (gallery-check).
// Fictional data in the documented response shapes; no real service is ever
// contacted. Each provider lives under its own prefix, e.g. /pihole/api/...
//   node web/e2e/fake-providers.mjs [port]   (default 8082)
// Credentials are accepted as given; session logins answer with fixed
// fictional sessions so the server's session paths run.
import http from "node:http";

const port = Number(process.argv[2] ?? 8082);
const now = Math.floor(Date.now() / 1000);
const iso = (offset) => new Date((now + offset) * 1000).toISOString();
const series = (n, f) => Array.from({ length: n }, (_, i) => f(i));

const routes = {
  // Pi-hole v6
  "POST /pihole/api/auth": { session: { valid: true, sid: "fake-sid", validity: 300 } },
  "DELETE /pihole/api/auth": null,
  "GET /pihole/api/stats/summary": { queries: { total: 48213, blocked: 9874, percent_blocked: 20.48, unique_domains: 3120 }, clients: { active: 14, total: 21 } },
  "GET /pihole/api/dns/blocking": { blocking: "enabled", timer: null },
  "GET /pihole/api/history": { history: series(48, (i) => ({ timestamp: now - (47 - i) * 600, total: 800 + Math.round(400 * Math.sin(i / 5) ** 2) + (i % 7) * 30, cached: 100, blocked: 150 + (i % 5) * 20, forwarded: 500 })) },
  "GET /pihole/api/stats/top_domains": (q) => q.get("blocked") === "true"
    ? { domains: [["ads.example.net", 2910], ["tracker.example.com", 1804], ["metrics.example.org", 940], ["pixel.example.net", 512]].map(([domain, count]) => ({ domain, count })), total_queries: 48213, blocked_queries: 9874 }
    : { domains: [["api.example.com", 4210], ["cdn.example.net", 2984], ["time.example.org", 1502], ["mail.example.com", 1180], ["updates.example.net", 860]].map(([domain, count]) => ({ domain, count })), total_queries: 48213, blocked_queries: 9874 },
  "GET /pihole/api/stats/top_clients": { clients: [["192.0.2.10", "laptop", 12840], ["192.0.2.21", "living-room-tv", 8211], ["192.0.2.33", "phone", 4002], ["192.0.2.40", "nas", 2210]].map(([ip, name, count]) => ({ ip, name, count })), total_queries: 48213, blocked_queries: 9874 },
  // AdGuard Home
  "GET /adguard/control/status": { protection_enabled: true, running: true, version: "v0.107.0" },
  "GET /adguard/control/stats/config": { enabled: true, interval: 86400000, ignored: [] },
  "GET /adguard/control/stats": {
    time_units: "hours", num_dns_queries: 31902, num_blocked_filtering: 4410, num_replaced_safebrowsing: 3, num_replaced_safesearch: 0, num_replaced_parental: 0, avg_processing_time: 0.012,
    top_queried_domains: [{ "api.example.com": 3120 }, { "cdn.example.net": 2210 }, { "time.example.org": 980 }],
    top_clients: [{ "192.0.2.10": 9120 }, { "192.0.2.21": 6011 }, { "192.0.2.33": 2402 }],
    top_blocked_domains: [{ "ads.example.net": 1910 }, { "tracker.example.com": 1204 }],
    top_upstreams_responses: [], top_upstreams_avg_time: [],
    dns_queries: series(24, (i) => 900 + Math.round(600 * Math.sin(i / 4) ** 2)), blocked_filtering: series(24, (i) => 120 + (i % 6) * 15), replaced_safebrowsing: series(24, () => 0), replaced_parental: series(24, () => 0),
  },
  // Sonarr v4 / Radarr v5 (API v3)
  "GET /sonarr/api/v3/health": [],
  "GET /sonarr/api/v3/queue/status": { totalCount: 3, count: 3, unknownCount: 0, errors: false, warnings: false },
  "GET /sonarr/api/v3/wanted/missing": { page: 1, pageSize: 10, totalRecords: 14, records: [
    { id: 1, title: "The Long Night", seasonNumber: 2, episodeNumber: 4, airDateUtc: iso(-86400 * 3), series: { title: "Example Show" } },
    { id: 2, title: "Pilot", seasonNumber: 1, episodeNumber: 1, airDateUtc: iso(-86400 * 9), series: { title: "Sample Series" } },
  ] },
  "GET /sonarr/api/v3/calendar": [
    { id: 11, title: "Homecoming", seasonNumber: 3, episodeNumber: 7, airDateUtc: iso(3600 * 5), hasFile: false, monitored: true, series: { title: "Example Show" } },
    { id: 12, title: "The Return", seasonNumber: 1, episodeNumber: 2, airDateUtc: iso(86400 + 3600 * 3), hasFile: false, monitored: true, series: { title: "Sample Series" } },
    { id: 13, title: "Finale", seasonNumber: 5, episodeNumber: 10, airDateUtc: iso(86400 * 2 + 3600), hasFile: false, monitored: true, series: { title: "Another Drama" } },
  ],
  "GET /sonarr/api/v3/queue": { page: 1, pageSize: 10, totalRecords: 2, records: [
    { id: 1, title: "Example.Show.S03E06", size: 2_400_000_000, sizeleft: 600_000_000, status: "downloading", trackedDownloadState: "downloading", timeleft: "00:04:10", series: { title: "Example Show" }, episode: { title: "Crossroads", seasonNumber: 3, episodeNumber: 6 } },
    { id: 2, title: "Sample.Series.S01E01", size: 1_200_000_000, sizeleft: 1_100_000_000, status: "queued", trackedDownloadState: "downloading", timeleft: "00:21:00", series: { title: "Sample Series" }, episode: { title: "Pilot", seasonNumber: 1, episodeNumber: 1 } },
  ] },
  "GET /radarr/api/v3/health": [],
  "GET /radarr/api/v3/queue/status": { totalCount: 1, count: 1, unknownCount: 0, errors: false, warnings: false },
  "GET /radarr/api/v3/wanted/missing": { page: 1, pageSize: 10, totalRecords: 6, records: [{ id: 3, title: "Sample Movie", year: 2026 }, { id: 4, title: "Another Film", year: 2025 }] },
  "GET /radarr/api/v3/calendar": [
    { id: 21, title: "Sample Movie", year: 2026, digitalRelease: iso(86400 + 7200), inCinemas: iso(-86400 * 40), hasFile: false, monitored: true },
    { id: 22, title: "Another Film", year: 2026, inCinemas: iso(86400 * 2), hasFile: false, monitored: true },
  ],
  "GET /radarr/api/v3/queue": { page: 1, pageSize: 10, totalRecords: 1, records: [
    { id: 5, title: "Sample.Movie.2026.1080p", size: 8_000_000_000, sizeleft: 2_000_000_000, status: "downloading", trackedDownloadState: "downloading", timeleft: "00:12:00", movie: { title: "Sample Movie", year: 2026 } },
  ] },
  // qBittorrent WebUI API v2 (cookie session)
  "POST /qbittorrent/api/v2/auth/login": { status: 200, text: "Ok.", headers: { "set-cookie": "SID=fake-qbt-session; HttpOnly; path=/" } },
  "GET /qbittorrent/api/v2/transfer/info": { dl_info_speed: 12_400_000, up_info_speed: 1_100_000, dl_info_data: 52_000_000_000, up_info_data: 9_000_000_000, connection_status: "connected" },
  "GET /qbittorrent/api/v2/sync/maindata": { rid: 1, full_update: true, server_state: { free_space_on_disk: 812_000_000_000 }, torrents: { a: { state: "downloading" }, b: { state: "uploading" }, c: { state: "pausedUP" }, d: { state: "stalledUP" } } },
  "GET /qbittorrent/api/v2/torrents/info": [
    ["example-linux-2026.iso", 0.82, 6_200_000, 120_000, 95, "downloading", 4_700_000_000],
    ["sample-dataset.tar", 0.41, 4_100_000, 0, 600, "downloading", 12_000_000_000],
    ["open-movie-1080p.mkv", 1, 0, 840_000, 8640000, "uploading", 2_100_000_000],
    ["podcast-archive.zip", 0.07, 900_000, 0, 3100, "stalledDL", 800_000_000],
  ].map(([name, progress, dlspeed, upspeed, eta, state, size], i) => ({ hash: `${i}`.padStart(40, "0"), name, progress, dlspeed, upspeed, eta, state, size, total_size: size, amount_left: Math.round(size * (1 - progress)), category: "", num_seeds: 12, num_leechs: 3, ratio: 0.4 })),
  // Proxmox VE
  "GET /proxmox/api2/json/version": { data: { version: "8.3.0", release: "8.3", repoid: "fake" } },
  "GET /proxmox/api2/json/cluster/resources": (q) => ({ data: {
    node: [
      { id: "node/pve1", type: "node", node: "pve1", status: "online", cpu: 0.23, maxcpu: 16, mem: 41_000_000_000, maxmem: 68_719_476_736, disk: 34_789_235_712, maxdisk: 100_931_731_456, uptime: 1_204_000 },
      { id: "node/pve2", type: "node", node: "pve2", status: "online", cpu: 0.61, maxcpu: 8, mem: 28_000_000_000, maxmem: 34_359_738_368, disk: 61_000_000_000, maxdisk: 100_931_731_456, uptime: 604_000 },
    ],
    vm: [
      { id: "qemu/101", type: "qemu", vmid: 101, name: "web", node: "pve1", status: "running", cpu: 0.04, mem: 1_288_490_188, maxmem: 4_294_967_296, uptime: 273_600, template: 0 },
      { id: "lxc/102", type: "lxc", vmid: 102, name: "database", node: "pve1", status: "running", cpu: 0.11, mem: 4_080_218_931, maxmem: 8_589_934_592, uptime: 1_044_000, template: 0 },
      { id: "qemu/103", type: "qemu", vmid: 103, name: "home-automation", node: "pve2", status: "running", cpu: 0.02, mem: 2_100_000_000, maxmem: 4_294_967_296, uptime: 98_000, template: 0 },
      { id: "lxc/104", type: "lxc", vmid: 104, name: "media", node: "pve2", status: "running", cpu: 0.31, mem: 3_000_000_000, maxmem: 8_589_934_592, uptime: 50_000, template: 0 },
      { id: "qemu/110", type: "qemu", vmid: 110, name: "test-vm", node: "pve2", status: "stopped", template: 0 },
    ],
    storage: [
      { id: "storage/pve1/local", type: "storage", storage: "local", node: "pve1", status: "available", disk: 23_192_823_808, maxdisk: 100_931_731_456, shared: 0, plugintype: "dir" },
      { id: "storage/pve1/backups", type: "storage", storage: "backups", node: "pve1", status: "available", disk: 3_600_000_000_000, maxdisk: 4_398_046_511_104, shared: 1, plugintype: "nfs" },
    ],
  }[q.get("type")] ?? [] }),
  // Glances API v4
  "GET /glances/api/4/cpu": { total: 18.4, user: 11.2, system: 5.1, idle: 81.6, cpucore: 8 },
  "GET /glances/api/4/mem": { percent: 42.1, total: 34_359_738_368, used: 14_465_449_574, available: 19_894_288_794 },
  "GET /glances/api/4/memswap": { percent: 3.2, total: 8_589_934_592, used: 274_877_906, free: 8_315_056_686 },
  "GET /glances/api/4/load": { cpucore: 8, min1: 0.84, min5: 0.91, min15: 0.77 },
  "GET /glances/api/4/fs/top/10": [{ device_name: "/dev/nvme0n1p2", fs_type: "ext4", mnt_point: "/", percent: 38.5, size: 1_000_000_000_000, used: 385_000_000_000 }, { device_name: "/dev/sda1", fs_type: "ext4", mnt_point: "/data", percent: 81.2, size: 4_000_000_000_000, used: 3_248_000_000_000 }],
  "GET /glances/api/4/cpu/total/history/24": { total: series(24, (i) => [iso(-(24 - i) * 5), 10 + 12 * Math.sin(i / 3) ** 2 + (i % 4)]) },
  "GET /glances/api/4/mem/percent/history/24": { percent: series(24, (i) => [iso(-(24 - i) * 5), 40 + (i % 5)]) },
  "GET /glances/api/4/processlist/top/10": [["example-db", 12.1, 4.2, 1204, "postgres"], ["media-server", 8.4, 6.1, 2101, "media"], ["backup-agent", 2.0, 0.6, 2210, "root"], ["web", 1.1, 1.8, 901, "www-data"]].map(([name, cpu_percent, memory_percent, pid, username]) => ({ name, cpu_percent, memory_percent, pid, username, status: "S" })),
  "GET /glances/api/4/sensors/top/10": [{ label: "Package id 0", type: "temperature_core", unit: "C", value: 54, warning: 80, critical: 100 }, { label: "nvme", type: "temperature_core", unit: "C", value: 41, warning: 70, critical: 85 }],
};

http.createServer((req, res) => {
  const url = new URL(req.url ?? "/", "http://fake");
  const key = `${req.method} ${url.pathname}`;
  // Drain any request body before answering.
  req.resume();
  req.on("end", () => {
    if (!(key in routes)) return res.writeHead(404, { "content-type": "application/json" }).end("{}");
    const route = routes[key];
    const body = typeof route === "function" ? route(url.searchParams, req) : route;
    if (body === null) return res.writeHead(204).end();
    if (body && typeof body.text === "string" && body.status) return res.writeHead(body.status, { "content-type": "text/plain", ...body.headers }).end(body.text);
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(body));
  });
}).listen(port, "0.0.0.0", () => console.log(`fake providers on ${port}`));
