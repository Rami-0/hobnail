# Host page (`/app/host`)

Everything the kernel reports about this machine, from the read-only host
`/proc` and `/sys` mounts. Live numbers refresh every 3 s while the tab is
visible; history comes from the database (1 h … 30 d, 1 h by default). The
history charts move and zoom like the History page: "Earlier", "Zoom in",
"Zoom out", "Later", or a drag across a chart, with incidents as markers.

- **CPU:** per core, user/system/iowait/irq/steal, load.
- **Pressure stall (PSI):** share of time tasks waited for CPU, memory or I/O —
  the most honest "is this machine suffering?" signal.
- **Memory:** used, buffers, cache, available, slab, shared, dirty, major faults, swap
  and **swap activity** (in/out per second).
- **Disks:** file systems (used/free only for the one holding `/data`; others
  show their size: `statvfs` on other mounts needs privileges) and per-device
  throughput, IOPS and %util.
- **Network:** per interface rates, errors, drops, link speed; bridges summed,
  container `veth` links hidden.
- **Sensors:** every hwmon temperature and fan with high/critical limits.
- **Who is using the host:** CPU and RAM by container (top 10) and the top 15
  processes mapped to their container. Process command lines require Admin/Owner. A container's name
  opens it; a logs icon beside it opens its logs, only for containers whose
  logs may be read (never for an excluded one).

Rates are averages since the previous reading (the intro says so once). Every
table fits its panel at 390 px: labels wrap, a rate's unit goes under its
number on a phone, and the process tables cut long names with "…" instead of
widening. A GPU panel appears only when the kernel lists a DRM device: the
card, its driver and, where the driver exports it (amdgpu), busy %.

Not available without privileges (deliberately not added, and not shown on
the page): S.M.A.R.T. (needs raw access to the disks under `/dev`; drive
temperatures appear under Sensors when the kernel exposes them), GPU busy %
on Intel and NVIDIA (needs perf or device access), systemd failed units
(needs the system D-Bus socket).

The page and its host topic require login and unrestricted access. Scoped
accounts cannot read host-wide data. The host topic is released on leaving
the page; open disclosures and focus survive JSON updates.
