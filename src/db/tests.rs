//! Database tests (docs/DESIGN.md section 8): migrations, recording and
//! series, downsampling and gaps, rollups and retention, the event cap,
//! sessions, revisions, the JSON API.

use super::*;
use crate::model::{DiskStats, SystemStats};

const T0: i64 = 1_800_000_000; // a round-ish unix time (multiple of 600)

fn sys(cpu: f64, mem: u64) -> SystemStats {
    SystemStats {
        cpu_percent: Some(cpu),
        cpu_cores: 4,
        load: [cpu / 100.0, 0.0, 0.0],
        mem_total: 8 << 30,
        mem_used: mem,
        swap_total: 1 << 30,
        swap_used: 1 << 20,
        disks: vec![DiskStats {
            label: "/".into(),
            total: 1000,
            used: 250,
            avail: 750,
        }],
        net_rx_bps: Some(1500.0),
        net_tx_bps: None,
        net_iface: Some("eth0".into()),
        temp_c: Some(41.5),
        uptime_secs: 1,
        sampled_at: 0,
    }
}

fn ctr(cpu: f64, mem: u64) -> ContainerStats {
    ContainerStats {
        cpu_percent: cpu,
        mem_bytes: mem,
    }
}

fn count(db: &Db, sql: &str) -> i64 {
    db.with(-1, |i| i.conn.query_row(sql, [], |r| r.get(0)))
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rh-db-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.join("hobnail.db")
}

/// Fill raw host samples every `step` seconds over `[from, to)` with cpu = f(ts).
fn fill_sys(db: &Db, from: i64, to: i64, step: i64, f: impl Fn(i64) -> f64) {
    db.with((), |i| i.conn.execute_batch("BEGIN"));
    let mut t = from;
    while t < to {
        db.record_system(t, &sys(f(t), 1 << 30));
        t += step;
    }
    db.with((), |i| i.conn.execute_batch("COMMIT"));
}

#[test]
fn disabled_db_is_inert() {
    let db = Db::disabled();
    assert!(!db.is_enabled());
    db.record_system(T0, &sys(1.0, 1));
    db.record_event(T0, "app.start", "info", "", "");
    let s = db.system_series(SysMetric::Cpu, Range::H24, T0);
    assert!(s.points.is_empty());
    assert_eq!(s.step, 240);
    assert!(db.events(&EventFilter::default()).is_empty());
    assert!(!db.create_session("h", 1, 2));
    assert!(!db.kv_set("a", "b"));
    assert_eq!(db.kv_get("a"), None);
    assert_eq!(db.stats(), DbStats::default());
    db.maintain(T0);
    db.clear_history();
}

#[test]
fn migrations_are_idempotent_and_pragmas_set() {
    let path = tmp("migrate");
    {
        let db = Db::open(&path).unwrap();
        db.kv_set("k", "v");
        db.record_event(T0, "app.start", "info", "", "first");
        db.with((), |i| {
            assert_eq!(schema::version(&i.conn).unwrap(), schema::MIGRATIONS.len());
            let mode: String = i.conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
            assert_eq!(mode, "wal");
            let av: i64 = i.conn.query_row("PRAGMA auto_vacuum", [], |r| r.get(0))?;
            assert_eq!(av, 2, "incremental");
            let cache: i64 = i.conn.query_row("PRAGMA cache_size", [], |r| r.get(0))?;
            assert_eq!(cache, -2048);
            let fk: i64 = i.conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
            assert_eq!(fk, 1);
            let sync: i64 = i.conn.query_row("PRAGMA synchronous", [], |r| r.get(0))?;
            assert_eq!(sync, 1, "NORMAL");
            let ts: i64 = i.conn.query_row("PRAGMA temp_store", [], |r| r.get(0))?;
            assert_eq!(ts, 2, "MEMORY");
            // Running the migrations again on a current file is a no-op.
            schema::migrate(&mut i.conn).unwrap();
            assert_eq!(schema::version(&i.conn).unwrap(), schema::MIGRATIONS.len());
            Ok(())
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        // The WAL holds the same data: same mode (it is still there while open).
        let db = Db::open(&path).unwrap();
        db.kv_set("k2", "v");
        let wal = path.with_file_name("hobnail.db-wal");
        let mode = std::fs::metadata(&wal).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // Reopen: same schema version, data kept.
    let db = Db::open(&path).unwrap();
    assert_eq!(db.kv_get("k").as_deref(), Some("v"));
    assert_eq!(db.events(&EventFilter::default()).len(), 1);
    assert!(db.stats().size_bytes > 0);
    drop(db);
    // A file from a newer build is refused, not damaged.
    {
        let c = Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", 99).unwrap();
    }
    let e = Db::open(&path).err().unwrap();
    assert!(e.contains("newer"), "{e}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn bad_path_fails_cleanly() {
    let e = Db::open(Path::new("/proc/definitely/not/here.db"));
    assert!(e.is_err());
    assert!(!open_or_disabled(Path::new("/proc/definitely/not/here.db")).is_enabled());
}

#[test]
fn record_and_read_system_series() {
    let db = Db::open_in_memory().unwrap();
    let now = T0 + 3600;
    fill_sys(&db, T0, now, 15, |t| ((t - T0) / 15 % 10) as f64 + 0.25);
    let s = db.system_series(SysMetric::Cpu, Range::H1, now);
    assert_eq!(s.step, 15);
    assert_eq!(s.points.len(), 240);
    assert!(s.points.windows(2).all(|w| w[0].0 < w[1].0), "oldest first");
    assert_eq!(s.points[0], (T0, 0.25));
    assert_eq!(s.points[3], (T0 + 45, 3.25));
    // Other columns round-trip through their scaling.
    let mem = db.system_series(SysMetric::Mem, Range::H1, now);
    assert_eq!(mem.points[0].1, (1u64 << 30) as f64);
    assert_eq!(
        db.system_series(SysMetric::Disk, Range::H1, now).points[0].1,
        25.0
    );
    assert_eq!(
        db.system_series(SysMetric::Temp, Range::H1, now).points[0].1,
        41.5
    );
    assert_eq!(
        db.system_series(SysMetric::NetRx, Range::H1, now).points[0].1,
        1500.0
    );
    // A metric that was never known (tx: None) has no points at all.
    assert!(db
        .system_series(SysMetric::NetTx, Range::H1, now)
        .points
        .is_empty());
    // 6 h from raw: 60 s buckets, averaged.
    let s6 = db.system_series(SysMetric::Cpu, Range::H6, now);
    assert_eq!(s6.step, 60);
    assert_eq!(s6.points.len(), 60);
    assert_eq!(s6.points[0], (T0, 1.75)); // avg of 0.25 1.25 2.25 3.25
    let mx = db.system_series_agg(SysMetric::Cpu, Range::H6, now, Agg::Max);
    assert_eq!(mx.points[0], (T0, 3.25));
}

#[test]
fn every_metric_and_range_queries() {
    let db = Db::open_in_memory().unwrap();
    let now = T0 + 3600;
    fill_sys(&db, T0, now, 15, |_| 3.0);
    let c = ctr(1.0, 2);
    db.record_containers(now - 15, &[("a", &c)]);
    db.maintain(now - 60);
    for r in Range::ALL {
        for agg in [Agg::Avg, Agg::Max] {
            for m in SysMetric::ALL {
                let s = db.system_series_agg(m, r, now, agg);
                assert_eq!(s.points.is_empty(), m == SysMetric::NetTx, "{m:?} {r:?}");
                let body = api::answer(
                    &db,
                    "/api/history/system",
                    &format!("metric={}&range={}", m.as_str(), r.as_str()),
                    now,
                    true,
                );
                assert!(body.is_ok(), "{m:?} {r:?}");
            }
            for m in [CtrMetric::Cpu, CtrMetric::Mem] {
                let s = db.container_series_agg("a", m, r, now, agg);
                assert_eq!(s.points.len(), 1, "{m:?} {r:?} {agg:?}");
            }
        }
    }
}

#[test]
fn downsampling_keeps_every_range_under_400_points() {
    for r in Range::ALL {
        let (base, bucket) = series::bucket_for(r);
        assert_eq!(bucket % base, 0, "{r:?}");
        assert!(r.secs() / bucket <= series::MAX_POINTS, "{r:?}");
        assert!(r.secs() / bucket >= 200, "{r:?} has enough detail");
    }
    // 8 days of 1-minute data, rolled up, then every range is <= ~400 points.
    let db = Db::open_in_memory().unwrap();
    let now = T0 + 8 * 86_400;
    fill_sys(&db, T0, now, 60, |t| (t % 7) as f64);
    db.maintain(now);
    for r in Range::ALL {
        let s = db.system_series(SysMetric::Cpu, r, now);
        assert!(
            !s.points.is_empty() && s.points.len() <= 401,
            "{r:?}: {}",
            s.points.len()
        );
        assert!(s.points.windows(2).all(|w| w[1].0 - w[0].0 >= s.step));
    }
    // Container series are downsampled the same way.
    let c = ctr(10.0, 1);
    db.record_containers(now + 30, &[("web", &c)]);
    let s = db.container_series("web", CtrMetric::Cpu, Range::D30, now + 31);
    assert_eq!(s.step, 6600);
    assert_eq!(s.points.len(), 1);
}

#[test]
fn gaps_stay_gaps() {
    let db = Db::open_in_memory().unwrap();
    let now = T0 + 3600;
    // Up for 10 minutes, down for 20, up for 30.
    fill_sys(&db, T0, T0 + 600, 15, |_| 5.0);
    fill_sys(&db, T0 + 1800, now, 15, |_| 7.0);
    let s = db.system_series(SysMetric::Cpu, Range::H1, now);
    assert_eq!(s.points.len(), 40 + 120);
    assert!(s
        .points
        .iter()
        .all(|(t, _)| *t < T0 + 600 || *t >= T0 + 1800));
    // No invented values: everything is one of the two levels.
    assert!(s.points.iter().all(|(_, v)| *v == 5.0 || *v == 7.0));
    let gap = s.points.windows(2).map(|w| w[1].0 - w[0].0).max().unwrap();
    assert_eq!(gap, 1800 - 600 + 15);
    // The gap survives rollups too.
    db.maintain(now);
    let s = db.system_series(SysMetric::Cpu, Range::H24, now);
    assert!(s
        .points
        .iter()
        .all(|(t, _)| *t < T0 + 600 || *t >= T0 + 1680));
}

#[test]
fn rollups_avg_max_and_stitching() {
    let db = Db::open_in_memory().unwrap();
    // One minute with cpu 10, 20, 30, 40 and container mem 100..400.
    for (n, v) in [10.0, 20.0, 30.0, 40.0].iter().enumerate() {
        let t = T0 + n as i64 * 15;
        db.record_system(t, &sys(*v, 1000 * (n as u64 + 1)));
        let c = ctr(*v / 10.0, 100 * (n as u64 + 1));
        db.record_containers(t, &[("db", &c)]);
    }
    let now = T0 + 7200;
    db.maintain(now);
    db.with((), |i| {
        let (avg, max): (i64, i64) = i.conn.query_row(
            "SELECT cpu, cpu_x FROM sys_roll WHERE res = 60 AND ts = ?1",
            [T0],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!((avg, max), (2500, 4000)); // scaled by 100
        let (avg, max): (i64, i64) = i.conn.query_row(
            "SELECT mem, mem_x FROM ctr_roll WHERE res = 600 AND ts = ?1",
            [T0],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!((avg, max), (250, 400));
        assert_eq!(i.w60, now / 60 * 60);
        assert_eq!(i.w600, now / 600 * 600);
        Ok(())
    });
    // Raw is still there (younger than 6 h): 24 h reads the rollup up to the
    // watermark and raw after it, nothing twice.
    let fresh = now + 30;
    db.record_system(fresh, &sys(90.0, 1));
    let s = db.system_series(SysMetric::Cpu, Range::H24, fresh);
    assert_eq!(s.step, 240);
    assert_eq!(s.points.len(), 2, "{:?}", s.points);
    assert_eq!(s.points[0], (T0 / 240 * 240, 25.0));
    assert_eq!(s.points[1].1, 90.0);
    let m = db.system_series_agg(SysMetric::Cpu, Range::H24, fresh, Agg::Max);
    assert_eq!(m.points[0].1, 40.0);
    let c = db.container_series("db", CtrMetric::Mem, Range::D30, fresh);
    assert_eq!(c.points, vec![(T0 / 6600 * 6600, 250.0)]);
    let c = db.container_series_agg("db", CtrMetric::Cpu, Range::D7, fresh, Agg::Max);
    assert_eq!(c.points[0].1, 4.0);
    // Watermarks survive a restart (stored in kv).
    assert_eq!(db.kv_get("hist.w60"), Some((now / 60 * 60).to_string()));
}

#[test]
fn averages_are_weighted_by_sample_count() {
    let db = Db::open_in_memory().unwrap();
    // One rolled-up minute (4 samples of 10) and, in the same 24h bucket,
    // 4 raw samples of 30 after the watermark: the mean is 20, not 26.
    fill_sys(&db, T0, T0 + 60, 15, |_| 10.0);
    db.maintain(T0 + 60);
    fill_sys(&db, T0 + 60, T0 + 120, 15, |_| 30.0);
    let s = db.system_series(SysMetric::Cpu, Range::H24, T0 + 120);
    assert_eq!(s.points, vec![(T0, 20.0)]);
    // 1 min -> 10 min: a minute with 4 samples of 10 and one with a single
    // sample of 40 average to 16, not 25.
    let db = Db::open_in_memory().unwrap();
    fill_sys(&db, T0, T0 + 60, 15, |_| 10.0);
    db.record_system(T0 + 60, &sys(40.0, 1));
    let c10 = ctr(10.0, 100);
    let c40 = ctr(40.0, 400);
    for t in [T0, T0 + 15, T0 + 30, T0 + 45] {
        db.record_containers(t, &[("a", &c10)]);
    }
    db.record_containers(T0 + 60, &[("a", &c40)]);
    db.maintain(T0 + 600);
    let n: i64 = count(&db, "SELECT n FROM sys_roll WHERE res = 600");
    assert_eq!(n, 5);
    let s = db.system_series(SysMetric::Cpu, Range::D30, T0 + 600);
    assert_eq!(s.points.len(), 1);
    assert_eq!(s.points[0].1, 16.0);
    let c = db.container_series("a", CtrMetric::Mem, Range::D30, T0 + 600);
    assert_eq!(c.points[0].1, 160.0);
    let m = db.container_series_agg("a", CtrMetric::Mem, Range::D30, T0 + 600, Agg::Max);
    assert_eq!(m.points[0].1, 400.0);
}

#[test]
fn upgrade_from_v1() {
    let path = tmp("upgrade");
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(schema::MIGRATIONS[0]).unwrap();
        c.pragma_update(None, "user_version", 1).unwrap();
        c.execute(
            "INSERT INTO sys_roll (res, ts, cpu, cpu_x) VALUES (60, ?1, 500, 700)",
            [T0],
        )
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    db.with((), |i| {
        assert_eq!(schema::version(&i.conn).unwrap(), schema::MIGRATIONS.len());
        Ok(())
    });
    assert_eq!(count(&db, "SELECT n FROM sys_roll"), 1, "old rows weigh 1");
    db.with((), |i| {
        i.w60 = T0 + 60;
        Ok(())
    });
    let s = db.system_series(SysMetric::Cpu, Range::H24, T0 + 120);
    assert_eq!(s.points, vec![(T0, 5.0)]);
    drop(db);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn maintain_runs_at_most_hourly() {
    let db = Db::open_in_memory().unwrap();
    assert!(db.maintain_due(T0));
    db.maintain(T0);
    db.maintain(T0 + 10);
    db.maintain(T0 + 3599);
    assert!(!db.maintain_due(T0 + 3599));
    assert_eq!(db.with(0, |i| Ok(i.maintain_runs)), 1);
    db.maintain(T0 + 3600);
    assert_eq!(db.with(0, |i| Ok(i.maintain_runs)), 2);
    // Weekly vacuum stamp.
    assert_eq!(db.kv_get("hist.vacuum"), Some(T0.to_string()));
    db.maintain(T0 + 3 * 86_400);
    assert_eq!(db.kv_get("hist.vacuum"), Some(T0.to_string()));
    db.maintain(T0 + 8 * 86_400);
    assert_eq!(
        db.kv_get("hist.vacuum"),
        Some((T0 + 8 * 86_400).to_string())
    );
}

#[test]
fn retention() {
    let db = Db::open_in_memory().unwrap();
    // 100 days of hourly-ish samples is too many to insert raw; insert raw
    // samples at both ends and rollup rows directly in between.
    let now = T0 + 100 * 86_400;
    let c = ctr(1.0, 1);
    for t in [T0, now - 7 * 3600, now - 5 * 3600, now - 60] {
        db.record_system(t, &sys(1.0, 1));
        db.record_containers(t, &[("old", &c)]);
    }
    db.with((), |i| {
        for (res, age) in [
            (60, 8 * 86_400),
            (60, 6 * 86_400),
            (600, 91 * 86_400),
            (600, 89 * 86_400),
        ] {
            i.conn.execute(
                "INSERT INTO sys_roll (res, ts, cpu, cpu_x) VALUES (?1, ?2, 1, 1)",
                [res, now - age],
            )?;
        }
        Ok(())
    });
    db.record_event(now - 181 * 86_400, "container.start", "info", "a", "");
    db.record_event(now - 179 * 86_400, "container.start", "info", "a", "");
    db.create_session("expired", T0, T0 + 10);
    db.create_session("live", now, now + 10);
    db.maintain(now);

    let raw: Vec<i64> = db.with(Vec::new(), |i| {
        let mut st = i.conn.prepare("SELECT ts FROM sys_samples ORDER BY ts")?;
        let v: rusqlite::Result<Vec<i64>> = st.query_map([], |r| r.get(0))?.collect();
        v
    });
    assert_eq!(raw, vec![now - 5 * 3600, now - 60], "raw kept 6 h");
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM ctr_samples"),
        2,
        "container raw kept 6 h"
    );
    let oldest60 = count(&db, "SELECT MIN(ts) FROM sys_roll WHERE res = 60");
    assert!(oldest60 >= now - 7 * 86_400, "1-minute rollups kept 7 d");
    assert_eq!(
        count(
            &db,
            &format!(
                "SELECT COUNT(*) FROM sys_roll WHERE res = 60 AND ts = {}",
                now - 6 * 86_400
            )
        ),
        1
    );
    let oldest600 = count(&db, "SELECT MIN(ts) FROM sys_roll WHERE res = 600");
    assert!(
        oldest600 >= now - 90 * 86_400,
        "10-minute rollups kept 90 d"
    );
    assert_eq!(
        count(
            &db,
            &format!(
                "SELECT COUNT(*) FROM sys_roll WHERE res = 600 AND ts = {}",
                now - 89 * 86_400
            )
        ),
        1
    );
    assert_eq!(count(&db, "SELECT COUNT(*) FROM events"), 1, "events 180 d");
    assert_eq!(db.session_count(now), 1);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM sessions"), 1);

    // A container whose data all expired disappears from ctr_names.
    let later = now + 100 * 86_400;
    db.maintain(later);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM ctr_names"), 0);
    assert!(db
        .container_series("old", CtrMetric::Cpu, Range::H1, later)
        .points
        .is_empty());
    // And comes back when it is sampled again.
    db.record_containers(later, &[("old", &c)]);
    assert_eq!(
        db.container_series("old", CtrMetric::Cpu, Range::H1, later + 1)
            .points
            .len(),
        1
    );
}

#[test]
fn unrolled_raw_is_never_pruned() {
    let db = Db::open_in_memory().unwrap();
    db.record_system(T0, &sys(1.0, 1));
    // Pretend the rollup has not covered T0 yet (watermark before it).
    db.with((), |i| {
        i.w60 = T0 - 60;
        i.w600 = T0 - 600;
        super::maintain::prune(i, T0 + 86_400)
    });
    assert_eq!(count(&db, "SELECT COUNT(*) FROM sys_samples"), 1);
}

#[test]
fn event_cap_and_filters() {
    assert_eq!(EVENTS_MAX, 100_000);
    let db = Db::open_in_memory().unwrap();
    db.with((), |i| {
        i.events_max = 100;
        Ok(())
    });
    db.with((), |i| i.conn.execute_batch("BEGIN"));
    for n in 0..2500 {
        let kind = if n % 2 == 0 {
            "container.start"
        } else {
            "alert.fired"
        };
        db.record_event(T0 + n, kind, "info", &format!("c{}", n % 5), "");
    }
    db.with((), |i| i.conn.execute_batch("COMMIT"));
    // Enforced every 1000 inserts on the way...
    assert!(count(&db, "SELECT COUNT(*) FROM events") <= 100 + 1000);
    // ...and exactly by maintenance.
    db.maintain(T0 + 10_000);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM events"), 100);
    let all = db.events(&EventFilter {
        limit: 500,
        ..Default::default()
    });
    assert_eq!(all.len(), 100);
    assert_eq!(all[0].ts, T0 + 2499, "newest first, oldest dropped");
    // Paging.
    let page = db.events(&EventFilter {
        limit: 10,
        before_id: Some(all[9].id),
        ..Default::default()
    });
    assert_eq!(page[0].id, all[10].id);
    // Subject and kind prefix.
    let c1 = db.events(&EventFilter {
        subject: Some("c1".into()),
        kind_prefix: Some("alert.".into()),
        ..Default::default()
    });
    assert!(!c1.is_empty());
    assert!(c1
        .iter()
        .all(|e| e.subject == "c1" && e.kind == "alert.fired"));
    assert_eq!(
        db.events(&EventFilter::default()).len(),
        50,
        "default limit"
    );
    // Time bounds: only events inside [since, until], newest first.
    let win = db.events(&EventFilter {
        limit: 500,
        since: Some(T0 + 2450),
        until: Some(T0 + 2459),
        ..Default::default()
    });
    assert_eq!(
        win.iter().map(|e| e.ts - T0).collect::<Vec<_>>(),
        (2450..=2459).rev().collect::<Vec<_>>()
    );
    let open_end = db.events(&EventFilter {
        limit: 500,
        since: Some(T0 + 2490),
        ..Default::default()
    });
    assert_eq!(open_end.len(), 10);
    // The bounded query walks the rowid (no index on `ts`, and no new one
    // without a migration): SQLite's plan says so. A History window is
    // therefore still read with the paged loop (`ui::pages::history::scan`).
    let plan: Vec<String> = db.with(Vec::new(), |i| {
        let mut st = i.conn.prepare(
            "EXPLAIN QUERY PLAN SELECT id FROM events WHERE ts >= 1 AND ts <= 2 ORDER BY id DESC LIMIT 5",
        )?;
        let rows = st.query_map([], |r| r.get::<_, String>(3))?;
        rows.collect()
    });
    assert!(
        plan.iter().all(|p| !p.contains("USING INDEX")),
        "an index on ts appeared: use it for History ({plan:?})"
    );
    // Restart counts.
    let rc = db.restart_counts(0);
    assert_eq!(rc.iter().map(|(_, n)| n).sum::<u32>(), 50);
    // Levels are normalised and long text clipped on a char boundary.
    db.record_event(T0, "x", "bogus", "s", &"é".repeat(1000));
    let e = &db.events(&EventFilter::default())[0];
    assert_eq!(e.level, "info");
    assert!(e.detail.len() <= 1024);
}

#[test]
fn docker_events_are_recorded() {
    let db = Db::open_in_memory().unwrap();
    let ev = |action: &str, detail: Option<&str>| crate::docker::Event {
        action: action.into(),
        name: Some("web".into()),
        time: T0 as u64,
        detail: detail.map(String::from),
    };
    record_docker_event(&db, &ev("start", None));
    record_docker_event(&db, &ev("die", Some("137")));
    record_docker_event(&db, &ev("stop", None));
    record_docker_event(&db, &ev("oom", None));
    record_docker_event(&db, &ev("health_status", Some("unhealthy")));
    record_docker_event(&db, &ev("health_status", Some("unhealthy"))); // repeat
    record_docker_event(&db, &ev("health_status", Some("healthy")));
    record_docker_event(&db, &ev("create", None)); // not kept
    let mut no_name = ev("start", None);
    no_name.name = None;
    record_docker_event(&db, &no_name);
    let evs = db.events(&EventFilter {
        subject: Some("web".into()),
        ..Default::default()
    });
    let got: Vec<_> = evs
        .iter()
        .rev()
        .map(|e| (e.kind.as_str(), e.level.as_str(), e.detail.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("container.start", "info", ""),
            ("container.die", "warn", "exit code 137"),
            ("container.stop", "info", ""),
            ("container.oom", "error", "killed: out of memory"),
            ("container.health", "warn", "unhealthy"),
            ("container.health", "info", "healthy"),
        ]
    );
    assert_eq!(db.restart_counts(T0), vec![("web".to_string(), 1)]);
}

#[test]
fn sessions() {
    let db = Db::open_in_memory().unwrap();
    assert!(db.create_session("h1", 100, 200));
    assert!(db.account_session("h1", 150, 210).is_some(), "valid");
    // Moved by < 60 s: no write.
    assert_eq!(
        count(&db, "SELECT expires FROM sessions WHERE token_hash = 'h1'"),
        200
    );
    assert!(db.account_session("h1", 150, 400).is_some());
    assert_eq!(
        count(&db, "SELECT expires FROM sessions WHERE token_hash = 'h1'"),
        400,
        "slid"
    );
    assert!(
        db.account_session("h1", 400, 900).is_none().is_some(),
        "expired at 400"
    );
    assert!(db.account_session("nope", 1, 2).is_none().is_some());
    db.create_session("h2", 100, 1000);
    db.create_session("h3", 100, 1000);
    db.sign_out_other_sessions(1, "h2", None).unwrap();
    assert!(db.account_session("h2", 101, 101).is_some());
    assert!(db.account_session("h3", 101, 101).is_none().is_some());
    db.delete_session("h2");
    assert!(db.account_session("h2", 101, 101).is_none().is_some());
    // At most MAX_SESSIONS are kept; the oldest go.
    for n in 0..30 {
        db.create_session(&format!("s{n}"), 1000 + n, 9999);
    }
    assert_eq!(db.session_count(1100), MAX_SESSIONS as u64);
    assert!(db.account_session("s0", 1100, 1100).is_none().is_some());
    assert!(db.account_session("s29", 1100, 1100).is_some());
    db.sign_out_other_sessions(1, "", None).unwrap();
    assert_eq!(db.session_count(0), 0);
}

#[test]
fn revisions_and_kv() {
    let db = Db::open_in_memory().unwrap();
    let first = db.add_revision(T0, "admin", "first", "a = 1\n").unwrap();
    for n in 0..210 {
        db.add_revision(T0 + n, "admin", "n", &format!("n = {n}\n"));
    }
    let revs = db.revisions(1000);
    assert_eq!(revs.len(), REVISIONS_KEEP as usize);
    assert_eq!(revs[0].note, "n");
    assert!(revs[0].id > revs[1].id, "newest first");
    assert_eq!(revs[0].bytes, "n = 209\n".len());
    assert_eq!(db.revision_text(first), None, "oldest dropped");
    assert_eq!(db.revision_text(revs[0].id).as_deref(), Some("n = 209\n"));
    assert_eq!(db.add_revision(T0, "a", "b", &"x".repeat(3 << 20)), None);
    assert!(db.kv_set("a", "1"));
    assert!(db.kv_set("a", "2"));
    assert_eq!(db.kv_get("a").as_deref(), Some("2"));
    db.kv_delete("a");
    assert_eq!(db.kv_get("a"), None);
}

#[test]
fn clear_history_keeps_settings() {
    let db = Db::open_in_memory().unwrap();
    fill_sys(&db, T0, T0 + 120, 15, |_| 1.0);
    let c = ctr(1.0, 1);
    db.record_containers(T0, &[("a", &c)]);
    db.record_event(T0, "app.start", "info", "", "");
    db.add_revision(T0, "admin", "", "x = 1");
    db.kv_set("auth.password", "h");
    db.create_session("s", T0, T0 + 100_000);
    db.maintain(T0 + 3600);
    db.clear_history();
    let st = db.stats();
    assert_eq!((st.sys_rows, st.ctr_rows, st.event_rows), (0, 0, 0));
    assert_eq!(st.revisions, 1);
    assert_eq!(st.oldest_ts, None);
    assert_eq!(db.kv_get("auth.password").as_deref(), Some("h"));
    assert!(db.account_session("s", T0 + 1, T0 + 1).is_some());
    // Recording works again afterwards.
    db.record_containers(T0 + 5000, &[("a", &c)]);
    assert_eq!(
        db.container_series("a", CtrMetric::Cpu, Range::H1, T0 + 5001)
            .points
            .len(),
        1
    );
}

#[test]
fn stats_counts() {
    let db = Db::open_in_memory().unwrap();
    fill_sys(&db, T0, T0 + 60, 15, |_| 1.0);
    db.record_event(T0, "app.start", "info", "", "");
    let st = db.stats();
    assert_eq!(st.sys_rows, 4);
    assert_eq!(st.event_rows, 1);
    assert_eq!(st.oldest_ts, Some(T0));
    assert!(st.size_bytes > 0);
}

// ---- JSON API ----

#[test]
fn api_series_and_errors() {
    use hyper::StatusCode;
    let db = Db::open_in_memory().unwrap();
    let now = T0 + 120;
    fill_sys(&db, T0, now, 15, |_| 12.3456);
    let c = ctr(50.0, 2048);
    db.record_containers(T0, &[("my app", &c)]);

    let body = api::answer(&db, "/api/history/system", "metric=cpu&range=1h", now, true).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["range"], "1h");
    assert_eq!(v["step"], 15);
    assert_eq!(v["points"].as_array().unwrap().len(), 8);
    assert_eq!(v["points"][0][0], T0);
    assert_eq!(v["points"][0][1], 12.35); // stored with 2 decimals
                                          // Default range is 1h.
    let body = api::answer(&db, "/api/history/system", "metric=mem", now, true).unwrap();
    assert!(body.starts_with("{\"range\":\"1h\",\"step\":15,\"points\":[["));
    let body = api::answer(
        &db,
        "/api/history/container/my%20app",
        "metric=mem&range=6h",
        now,
        true,
    )
    .unwrap();
    assert_eq!(
        body,
        format!("{{\"range\":\"6h\",\"step\":60,\"points\":[[{T0},2048]]}}")
    );
    let body = api::answer(
        &db,
        "/api/history/container/nobody",
        "metric=cpu",
        now,
        true,
    )
    .unwrap();
    assert_eq!(body, "{\"range\":\"1h\",\"step\":15,\"points\":[]}");

    for (path, q) in [
        ("/api/history/system", ""),
        ("/api/history/system", "metric=bogus"),
        ("/api/history/system", "metric=cpu&range=2h"),
        ("/api/history/system", "metric=cpu&agg=min"),
        ("/api/history/system", "metric=cpu&x=%zz"),
        ("/api/history/container/a", "metric=load1"),
        ("/api/history/container/", "metric=cpu"),
        ("/api/history/container/a%2Fb", "metric=cpu"),
        ("/api/history/container/a%0Ab", "metric=cpu"),
        ("/api/events", "limit=0"),
        ("/api/events", "limit=501"),
        ("/api/events", "limit=x"),
        ("/api/events", "before=-1"),
        ("/api/events", "kind=DROP%20TABLE"),
    ] {
        let e = api::answer(&db, path, q, now, true).unwrap_err();
        assert_eq!(e.0, StatusCode::BAD_REQUEST, "{path}?{q}");
    }
    assert_eq!(
        api::answer(&db, "/api/history/nope", "metric=cpu", now, true)
            .unwrap_err()
            .0,
        StatusCode::NOT_FOUND
    );
}

#[test]
fn api_events() {
    let db = Db::open_in_memory().unwrap();
    for n in 0..5 {
        db.record_event(T0 + n, "container.die", "warn", "web", "exit code \"1\"");
    }
    db.record_event(T0 + 9, "auth.login", "info", "admin", "");
    let body = api::answer(&db, "/api/events", "limit=2&subject=web", T0, true).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let evs = v["events"].as_array().unwrap();
    assert_eq!(evs.len(), 2);
    assert_eq!(evs[0]["kind"], "container.die");
    assert_eq!(evs[0]["detail"], "exit code \"1\"");
    assert_eq!(evs[0]["ts"], T0 + 4);
    let before = evs[1]["id"].as_i64().unwrap();
    let body = api::answer(
        &db,
        "/api/events",
        &format!("before={before}&kind=container."),
        T0,
        true,
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["events"].as_array().unwrap().len(), 3);
    let body = api::answer(&db, "/api/events", "kind=auth.", T0, true).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["events"][0]["subject"], "admin");
    // Anonymous viewers never see auth.* events (failed logins carry IPs).
    let body = api::answer(&db, "/api/events", "kind=auth.", T0, false).unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["events"].as_array().unwrap().len(), 0);
    let body = api::answer(&db, "/api/events", "limit=500", T0, false).unwrap();
    assert!(!body.contains("\"auth."), "{body}");
}

#[test]
fn query_parsing() {
    assert_eq!(
        api::parse_query("a=1&b=x%20y+z&&c").unwrap(),
        vec![
            ("a".into(), "1".into()),
            ("b".into(), "x y z".into()),
            ("c".into(), String::new())
        ]
    );
    assert!(api::parse_query("a=%G1").is_none());
    assert!(api::parse_query("a=%+1").is_none());
    assert!(api::parse_query("a=%ff").is_none(), "invalid utf-8");
    assert_eq!(api::percent_decode("%E2%9C%93").as_deref(), Some("✓"));
}

#[test]
fn latest_containers_returns_newest_sample_per_name() {
    let db = Db::open_in_memory().unwrap();
    let a = crate::model::ContainerStats {
        cpu_percent: 1.25,
        mem_bytes: 100,
    };
    let b = crate::model::ContainerStats {
        cpu_percent: 7.5,
        mem_bytes: 200,
    };
    db.record_containers(1000, &[("web", &a), ("db", &a)]);
    db.record_containers(1015, &[("web", &b)]);
    let m = db.latest_containers(1010);
    assert_eq!(m.get("web"), Some(&(7.5, 200)));
    assert_eq!(m.get("db"), None, "older than `since`");
    let m = db.latest_containers(0);
    assert_eq!(m.get("db"), Some(&(1.25, 100)));
    assert!(Db::disabled().latest_containers(0).is_empty());
}

#[test]
fn rename_reuses_legacy_database_without_losing_password_or_history() {
    let current = tmp("branding");
    let dir = current.parent().unwrap();
    let legacy = dir.join("rust-homepage.db");
    assert_eq!(default_path(dir), current);
    {
        let db = Db::open(&legacy).unwrap();
        db.kv_set("auth.password", "existing-password");
        db.record_system(T0, &sys(42.0, 1 << 30));
    }
    assert_eq!(default_path(dir), legacy);
    {
        let db = Db::open(&default_path(dir)).unwrap();
        assert_eq!(
            db.kv_get("auth.password").as_deref(),
            Some("existing-password")
        );
        assert!(!db
            .system_series(SysMetric::Cpu, Range::H1, T0 + 1)
            .points
            .is_empty());
    }
    assert!(
        !current.exists(),
        "upgrade must not create a separate database"
    );
    std::fs::write(&current, []).unwrap();
    assert_eq!(default_path(dir), current, "new filename has priority");
    std::fs::remove_dir_all(dir).unwrap();
}

/// The seek History's scan starts from: the newest id at or before a time,
/// one lookup on `events_ts` (gaps from the cap included).
#[test]
fn event_id_at_or_before_seeks_by_id() {
    let db = Db::open_in_memory().unwrap();
    assert_eq!(db.event_id_at_or_before(100), None, "empty table");
    for i in 0..1000 {
        db.record_event(1_000 + 2 * i, "container.die", "warn", "web", "");
    }
    // Gaps in the ids, as the cap leaves them.
    db.with((), |i| {
        i.conn
            .execute_batch("DELETE FROM events WHERE id % 7 = 0 OR (id > 400 AND id < 450)")
    });
    let all = db.events(&EventFilter {
        limit: 500,
        ..Default::default()
    });
    let newest = all[0].id;
    assert_eq!(db.event_id_at_or_before(999), None, "every event is newer");
    assert_eq!(db.event_id_at_or_before(i64::MAX), Some(newest));
    let want = |t: i64| -> Option<i64> {
        db.with(None, |i| {
            i.conn
                .query_row("SELECT MAX(id) FROM events WHERE ts <= ?1", [t], |r| {
                    r.get(0)
                })
        })
    };
    for t in [
        1_000, 1_001, 1_012, 1_013, 1_800, 1_850, 1_900, 2_500, 2_998, 2_999,
    ] {
        assert_eq!(db.event_id_at_or_before(t), want(t), "t = {t}");
    }
}

/// Schema v6 on a database that a v5 build wrote, with data in it: the
/// migration keeps every row, adds the two event indexes, and the event
/// reads give the same answers as before.
#[test]
fn upgrade_from_v5_adds_event_indexes() {
    let path = tmp("upgrade-v6");
    {
        let mut c = Connection::open(&path).unwrap();
        let tx = c.transaction().unwrap();
        for sql in &schema::MIGRATIONS[..5] {
            tx.execute_batch(sql).unwrap();
        }
        tx.pragma_update(None, "user_version", 5).unwrap();
        for i in 0..300i64 {
            let kind = ["container.start", "container.die", "auth.login"][(i % 3) as usize];
            tx.execute(
                "INSERT INTO events (ts, kind, level, subject, detail) VALUES (?1, ?2, 'info', ?3, '')",
                rusqlite::params![T0 + i * 60, kind, format!("c{}", i % 4)],
            )
            .unwrap();
        }
        tx.execute(
            "INSERT INTO sys_roll (res, ts, cpu, cpu_x) VALUES (60, ?1, 500, 700)",
            [T0],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    let db = Db::open(&path).unwrap();
    db.with((), |i| {
        assert_eq!(schema::version(&i.conn).unwrap(), 6);
        let idx: Vec<String> = i
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'events' ORDER BY name")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(idx, ["events_kind_ts", "events_subject", "events_ts"]);
        Ok(())
    });
    assert_eq!(count(&db, "SELECT COUNT(*) FROM events"), 300);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM sys_roll"), 1);
    // 100 starts, 25 per container; the last 10 minutes hold 4 of them.
    let all = db.restart_counts(0);
    assert_eq!(all.len(), 4);
    assert!(all.iter().all(|(_, n)| *n == 25));
    let recent: u32 = db.restart_counts(T0 + 288 * 60).iter().map(|x| x.1).sum();
    assert_eq!(recent, 4);
    let w = db.events(&EventFilter {
        since: Some(T0 + 100 * 60),
        until: Some(T0 + 109 * 60),
        limit: 500,
        ..Default::default()
    });
    assert_eq!(w.len(), 10);
    assert_eq!(w[0].ts, T0 + 109 * 60, "newest first");
    drop(db);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// The event reads that run per page use an index, never a full scan.
#[test]
fn event_reads_use_the_v6_indexes() {
    let db = Db::open_in_memory().unwrap();
    let plan = |sql: &str| -> String {
        db.with(String::new(), |i| {
            let mut st = i.conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
            let rows: Vec<String> = st
                .query_map([], |r| r.get::<_, String>(3))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(rows.join("; "))
        })
    };
    let rc = plan(
        "SELECT subject, COUNT(*) AS n FROM events WHERE kind = 'container.start' AND ts >= 5 GROUP BY subject ORDER BY n DESC, subject",
    );
    assert!(rc.contains("COVERING INDEX events_kind_ts"), "{rc}");
    let seek = plan("SELECT id FROM events WHERE ts <= 5 ORDER BY ts DESC, id DESC LIMIT 1");
    assert!(seek.contains("events_ts"), "{seek}");
    let ret = plan("SELECT COUNT(*) FROM events WHERE ts < 5");
    assert!(ret.contains("events_ts"), "{ret}");
}

/// Giving the page cache back keeps the data and the next reads working.
#[test]
fn release_memory_keeps_reads_working() {
    let db = Db::open_in_memory().unwrap();
    db.record_system(T0, &sys(10.0, 1 << 30));
    db.record_event(T0, "container.start", "info", "web", "");
    db.release_memory();
    db.record_system(T0 + 15, &sys(20.0, 1 << 30));
    db.release_memory();
    assert_eq!(count(&db, "SELECT COUNT(*) FROM sys_samples"), 2);
    assert_eq!(db.restart_counts(0), vec![("web".to_string(), 1)]);
}
