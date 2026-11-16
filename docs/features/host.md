# Host page (`/host`)

Everything the kernel reports about this machine, from the read-only host
`/proc` and `/sys` mounts. Live numbers refresh every 3 s while the tab is
visible; history comes from the database (1 h … 30 d).

- **CPU:** per core, user/system/iowait/irq/steal, load.
- **Pressure stall (PSI):** share of time tasks waited for CPU, memory or I/O —
  the most honest "is this machine suffering?" signal.
- **Memory:** used, buffers, cache, available, slab, dirty, major faults, swap
  and **swap activity** (in/out per second).
- **Disks:** file systems (used/free only for the one holding `/data`; others
  show their size: `statvfs` on other mounts needs privileges) and per-device
  throughput, IOPS and %util.
- **Network:** per interface rates, errors, drops, link speed; bridges summed,
  container `veth` links hidden.
- **Sensors:** every hwmon temperature and fan with high/critical limits.
- **Who is using the host:** CPU and RAM by container and top processes mapped
  to their container. Process command lines need a sign-in. A container's name
  opens it; a logs icon beside it opens its logs, only for containers whose
  logs may be read (never for an excluded one).

Not available without privileges (deliberately not added): S.M.A.R.T., GPU busy
%, systemd failed units.
