//! Shared container diagnostics and inspection redaction.

use crate::alerts::RuleKind;
use crate::db::EventFilter;
use crate::docker::{HealthLog, Inspect};
use crate::model::Container;
use crate::util::glob_match;

const HEALTH_LOGS: usize = 5;
pub(super) const HEALTH_OUTPUT_MAX: usize = 600;

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    t.push('…');
    t
}

/// A sort key: numbers (with `%`, or a `ps` elapsed time) sort as numbers.
fn sort_key(s: &str) -> Result<f64, String> {
    let t = s.trim().trim_end_matches('%');
    if let Ok(v) = t.parse::<f64>() {
        return Ok(v);
    }
    // [[dd-]hh:]mm:ss
    let (days, rest) = match t.split_once('-') {
        Some((d, r)) if d.chars().all(|c| c.is_ascii_digit()) => {
            (d.parse::<f64>().unwrap_or(0.0), r)
        }
        _ => (0.0, t),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    if (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    {
        let v = parts
            .iter()
            .fold(0.0, |a, p| a * 60.0 + p.parse::<f64>().unwrap_or(0.0));
        return Ok(days * 86_400.0 + v);
    }
    Err(s.to_lowercase())
}

/// Sort rows by column `col`; numbers before text.
pub fn sort_rows(rows: &mut [Vec<String>], col: usize, desc: bool) {
    rows.sort_by(|a, b| {
        let ka = sort_key(a.get(col).map_or("", String::as_str));
        let kb = sort_key(b.get(col).map_or("", String::as_str));
        let o = match (ka, kb) {
            (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            (Err(x), Err(y)) => x.cmp(&y),
        };
        if desc {
            o.reverse()
        } else {
            o
        }
    });
}

/// Redact administrative details from container inspection. Command lines, health
/// check commands and their output, and host paths of mounts can carry
/// secrets (`--password=…`, tokens in URLs), so like env values they need
/// administrative access.
pub const HIDDEN: &str = "hidden: sign in to view";
pub fn redact_inspect(mut i: crate::docker::Inspect) -> crate::docker::Inspect {
    let hide = || Some(vec![HIDDEN.to_string()]);
    if i.config.cmd.is_some() {
        i.config.cmd = hide();
    }
    if i.config.entrypoint.is_some() {
        i.config.entrypoint = hide();
    }
    i.config.env = i.config.env.map(|e| {
        e.into_iter()
            .map(|kv| kv.split('=').next().unwrap_or("").to_string())
            .collect()
    });
    if let Some(h) = i.config.healthcheck.as_mut() {
        if h.test.is_some() {
            h.test = hide();
        }
    }
    if let Some(h) = i.state.health.as_mut() {
        for l in h.log.iter_mut().flatten() {
            l.output = HIDDEN.to_string();
        }
    }
    for m in &mut i.mounts {
        if m.kind == "bind" {
            m.source = HIDDEN.to_string();
        }
    }
    i
}

/// What an exit code usually means.
pub fn exit_meaning(code: i64) -> String {
    match code {
        0 => "clean exit".into(),
        1 => "application error".into(),
        2 => "misuse or bad arguments".into(),
        125 => "Docker could not run it".into(),
        126 => "command not executable".into(),
        127 => "command not found".into(),
        130 => "interrupted (SIGINT)".into(),
        134 => "aborted (SIGABRT)".into(),
        137 => "killed (SIGKILL: out of memory or docker kill)".into(),
        139 => "crashed (SIGSEGV)".into(),
        143 => "terminated (SIGTERM)".into(),
        c if c > 128 && c < 160 => format!("killed by signal {}", c - 128),
        _ => "error".into(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Why {
    /// What is wrong apart from a failing health check ("" when that is all).
    pub text: String,
    /// When it happened (unix s), for the logs link.
    pub ts: Option<i64>,
    /// A failing health check: since when, how many in a row, the probes.
    pub health: Option<HealthWhy>,
}

/// A failing health check (NAV-11). `probes` carry Docker's last checks,
/// newest first, with probe output restricted to administrators.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthWhy {
    pub since: Option<i64>,
    pub streak: i64,
    /// (start, exit code, output), newest first, at most five.
    pub probes: Vec<(Option<i64>, i64, String)>,
}

/// The first line of the newest failing health probe's output (cut to 120
/// characters), if the last probe failed and said something. Probe output
/// can carry secrets: callers must require administrative access.
pub fn probe_line(i: &Inspect) -> Option<String> {
    let logs = i.state.health.as_ref()?.log.as_deref()?;
    let last = logs.last().filter(|l| l.exit_code != 0)?;
    let line = last.output.trim().lines().next()?.trim();
    (!line.is_empty()).then(|| truncate(line, 120))
}

/// Since when `c` has been unhealthy, from the event log, or `None` when
/// the newest stored health event does not say "unhealthy". Never earlier
/// than this run of the container: the recorder stores a health event only
/// when it differs from the last one stored for the name, so a container
/// recreated (or restarted) under the same name that turns unhealthy again
/// finds the previous one's "unhealthy" event. The answer is clamped to
/// the container's start (`started`, from an inspect, or else its creation
/// time) and to any `container.start` event newer than that health event.
/// One query; shared by Overview and container details.
pub fn unhealthy_since(db: &crate::db::Db, c: &Container, started: Option<i64>) -> Option<i64> {
    // Health and start events are looked up separately: a container that is
    // recreated often has more start and die events than any fixed window
    // since its last recorded health change.
    let newest = |kind: &str| {
        db.events(&EventFilter {
            subject: Some(c.name.clone()),
            kind_prefix: Some(kind.into()),
            limit: 1,
            ..Default::default()
        })
        .into_iter()
        .next()
    };
    let health = match newest("container.health") {
        Some(h) if h.detail != "unhealthy" => return None,
        Some(h) => h.ts,
        // No health event on record: only a container that is unhealthy
        // right now has a since-time, counted from its own start.
        None if c.health.as_deref() == Some("unhealthy") => 0,
        None => return None,
    };
    let restarted = newest("container.start").map_or(0, |e| e.ts);
    let since = health
        .max(restarted)
        .max(started.unwrap_or(0))
        .max(c.created);
    (since > 0).then_some(since)
}

/// One line on what is wrong, or `None` when nothing is. `unhealthy_since`
/// is the newest "unhealthy" health event from the database.
pub fn why_red(
    c: &Container,
    ins: Option<&Inspect>,
    starts_10m: u32,
    unhealthy_since: Option<i64>,
    now: i64,
) -> Option<Why> {
    let mut parts: Vec<String> = Vec::new();
    let mut ts = None;
    let mut hw = None;
    let st = ins.map(|i| &i.state);
    let running = c.is_running();
    if st.is_some_and(|s| s.oom_killed) && !running {
        parts.push("Killed: out of memory (OOM)".into());
        ts = st.and_then(|s| crate::docker::parse_rfc3339(&s.finished_at));
    }
    let code = st.filter(|_| !running).map(|s| s.exit_code).or(c.exit_code);
    if !running && c.state != "created" {
        if let Some(code) = code.filter(|c| *c != 0) {
            parts.push(format!("Exited with code {code}: {}", exit_meaning(code)));
            ts = ts.or_else(|| st.and_then(|s| crate::docker::parse_rfc3339(&s.finished_at)));
        }
    }
    if c.state == "dead" {
        parts.push("Dead: Docker could not stop or remove it".into());
    }
    if c.state == "restarting" {
        parts.push("Restarting".into());
    }
    let health = st.and_then(|s| s.health.as_ref());
    if running && c.health.as_deref() == Some("unhealthy") {
        // Start of the trailing failing run in Docker's last five checks.
        let logs = health.and_then(|h| h.log.as_deref()).unwrap_or(&[]);
        let run: Vec<&HealthLog> = logs.iter().rev().take_while(|l| l.exit_code != 0).collect();
        let first = run
            .last()
            .and_then(|l| crate::docker::parse_rfc3339(&l.start));
        // Never before this run of the container (a failing run in the
        // log can predate a restart by a probe).
        let start = st
            .and_then(|s| crate::docker::parse_rfc3339(&s.started_at))
            .unwrap_or(0)
            .max(c.created);
        let since = unhealthy_since.or(first).map(|s| s.max(start));
        let streak = health.map_or(0, |h| h.failing_streak as i64);
        let probes = logs
            .iter()
            .rev()
            .take(HEALTH_LOGS)
            .map(|l| {
                (
                    crate::docker::parse_rfc3339(&l.start),
                    l.exit_code,
                    l.output.clone(),
                )
            })
            .collect();
        hw = Some(HealthWhy {
            since,
            streak,
            probes,
        });
        ts = ts.or(since);
    }
    if starts_10m >= 3
        || c.die_times
            .iter()
            .filter(|t| **t as i64 >= now - 600)
            .count()
            >= 3
    {
        let n = starts_10m.max(
            c.die_times
                .iter()
                .filter(|t| **t as i64 >= now - 600)
                .count() as u32,
        );
        parts.push(format!("Restart loop: {n} restarts in 10 min"));
        ts = ts.or_else(|| c.die_times.iter().max().map(|t| *t as i64));
    }
    if parts.is_empty() && hw.is_none() {
        return None;
    }
    Some(Why {
        text: parts.join(" · "),
        ts,
        health: hw,
    })
}

/// Labels Docker Compose and image builds add on their own.
pub(crate) fn is_bookkeeping_label(k: &str) -> bool {
    [
        "com.docker.compose.",
        "org.opencontainers.image.",
        "com.docker.desktop.",
    ]
    .iter()
    .any(|p| k.starts_with(p))
}

/// Container alert rules that cover `name` (same include/exclude globs the
/// engine uses), with whether each is firing for this container right now.
pub fn matching_rules<'a>(
    rules: &'a [crate::alerts::RuleConfig],
    name: &str,
) -> Vec<&'a crate::alerts::RuleConfig> {
    rules
        .iter()
        .filter(|r| r.kind == RuleKind::Container)
        .filter(|r| {
            let inc = r.containers.as_deref().unwrap_or(&[]);
            let exc = r.exclude.as_deref().unwrap_or(&[]);
            (inc.is_empty() || inc.iter().any(|p| glob_match(p, name)))
                && !exc.iter().any(|p| glob_match(p, name))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::RuleConfig;
    use crate::testutil::{sample, state_with};

    fn rules(toml: &str) -> Vec<RuleConfig> {
        crate::config::parse(toml, "t.toml").unwrap().alerts
    }

    #[test]
    fn rule_matching_uses_include_and_exclude_globs() {
        let rs = rules(
            r#"
[[alerts]]
name = "all"
type = "container"
[[alerts]]
name = "web"
type = "container"
containers = ["web-*"]
exclude = ["web-test"]
[[alerts]]
name = "disk"
type = "host"
metric = "disk"
above = 90
"#,
        );
        let names = |n: &str| -> Vec<String> {
            matching_rules(&rs, n)
                .iter()
                .map(|r| r.name.clone())
                .collect()
        };
        assert_eq!(names("web-1"), ["all", "web"]);
        assert_eq!(names("web-test"), ["all"]);
        assert_eq!(names("db"), ["all"]);
    }

    // ---- container detail ----

    const SECRET: &str = "s3cr3t-VALUE-9f8e7d";

    /// Trimmed `docker inspect` of a real container (Docker 29.5), with a
    /// secret in the environment and markup in a health-check output.
    fn fixture() -> String {
        format!(
            r#"{{"Id":"6e6562000000000000000000000000000000000000000000000000000000000000","Created":"2026-11-14T08:00:00.123456789Z",
"Path":"/entry.sh","Args":["serve"],
"State":{{"Status":"running","Running":true,"Paused":false,"Restarting":false,"OOMKilled":false,"Dead":false,
  "Pid":4242,"ExitCode":0,"Error":"","StartedAt":"2026-11-14T09:00:00.5Z","FinishedAt":"0001-01-01T00:00:00Z",
  "Health":{{"Status":"unhealthy","FailingStreak":4,"Log":[
    {{"Start":"2026-11-14T09:58:00+01:00","End":"2026-11-14T09:58:01+01:00","ExitCode":0,"Output":"ok"}},
    {{"Start":"2026-11-14T10:00:00+01:00","End":"2026-11-14T10:00:02+01:00","ExitCode":1,"Output":"<b>connection refused</b> on :8080\nmore"}},
    {{"Start":"2026-11-14T10:01:00+01:00","End":"2026-11-14T10:01:02+01:00","ExitCode":1,"Output":"<b>connection refused</b> on :8080"}}]}}}},
"Image":"sha256:abcdef0123","Name":"/web","RestartCount":3,
"HostConfig":{{"NetworkMode":"shop_default","RestartPolicy":{{"Name":"unless-stopped","MaximumRetryCount":0}},
  "Memory":536870912,"NanoCpus":1500000000,"PidsLimit":256,"Privileged":false,"ReadonlyRootfs":true,"CapAdd":["NET_ADMIN"]}},
"Mounts":[{{"Type":"bind","Source":"/srv/web","Destination":"/data","Mode":"ro","RW":false}},
          {{"Type":"volume","Name":"web_cache","Source":"/var/lib/docker/volumes/web_cache/_data","Destination":"/cache","Mode":"z","RW":true}}],
"Config":{{"User":"1000:1000","Env":["PATH=/usr/bin","DB_PASSWORD={SECRET}","EMPTY="],"Cmd":["serve","--port","8080"],
  "Image":"img/web:latest","WorkingDir":"/app","Entrypoint":["/entry.sh"],
  "Healthcheck":{{"Test":["CMD-SHELL","curl -f http://localhost:8080/ || exit 1"],"Interval":30000000000,"Timeout":5000000000,"Retries":3}}}},
"NetworkSettings":{{"Networks":{{"shop_default":{{"IPAddress":"172.18.0.5","Gateway":"172.18.0.1","MacAddress":"02:42:ac:12:00:05","GlobalIPv6Address":"","Aliases":["web"]}}}}}}}}"#
        )
    }

    fn ins() -> Inspect {
        crate::docker::parse_inspect(fixture().as_bytes()).unwrap()
    }

    #[test]
    fn viewer_inspect_hides_commands_health_and_env_values() {
        let r = redact_inspect(ins());
        let dump = format!("{r:?}");
        for secret in [SECRET, "--port", "curl -f", "/entry.sh"] {
            assert!(!dump.contains(secret), "{secret} leaked to anonymous view");
        }
        assert!(dump.contains("DB_PASSWORD"), "env keys stay visible");
        // Signed in sees the full data.
        assert!(format!("{:?}", ins()).contains("--port"));
    }

    #[test]
    fn unhealthy_since_survives_many_restarts() {
        let st = state_with(sample(), "");
        let ev = |action: &str, time: u64, detail: Option<&str>| crate::docker::Event {
            action: action.into(),
            name: Some("web".into()),
            time,
            detail: detail.map(Into::into),
        };
        let t0: u64 = 1_794_600_000;
        let mut c = sample()[0].clone();
        c.health = Some("unhealthy".into());
        c.created = t0 as i64 - 10;
        crate::db::record_docker_event(&st.db, &ev("health_status", t0, Some("unhealthy")));
        // Recreated thirty times; the recorder keeps no second "unhealthy".
        for i in 0..30u64 {
            crate::db::record_docker_event(&st.db, &ev("die", t0 + 100 * i + 50, Some("137")));
            crate::db::record_docker_event(&st.db, &ev("start", t0 + 100 * i + 60, None));
        }
        let last_start = (t0 + 100 * 29 + 60) as i64;
        c.created = last_start - 1;
        assert_eq!(unhealthy_since(&st.db, &c, None), Some(last_start));
        // Not unhealthy and nothing on record: no since-time.
        let st2 = state_with(sample(), "");
        let mut ok = sample()[0].clone();
        ok.health = Some("healthy".into());
        assert_eq!(unhealthy_since(&st2.db, &ok, None), None);
        ok.health = Some("unhealthy".into());
        ok.created = 1234;
        assert_eq!(unhealthy_since(&st2.db, &ok, None), Some(1234));
    }

    #[test]
    fn unhealthy_since_never_predates_a_recreated_container() {
        let st = state_with(sample(), "");
        let ev = |action: &str, time: u64, detail: Option<&str>| crate::docker::Event {
            action: action.into(),
            name: Some("web".into()),
            time,
            detail: detail.map(Into::into),
        };
        let t0: u64 = 1_794_600_000;
        let mut c = sample()[0].clone();
        c.health = Some("unhealthy".into());
        c.created = t0 as i64 - 10;
        crate::db::record_docker_event(&st.db, &ev("start", t0, None));
        crate::db::record_docker_event(&st.db, &ev("health_status", t0 + 60, Some("unhealthy")));
        assert_eq!(unhealthy_since(&st.db, &c, None), Some(t0 as i64 + 60));
        // Two hours later it is removed and created again, and fails again.
        let t1 = t0 + 7200;
        crate::db::record_docker_event(&st.db, &ev("die", t1 - 20, Some("137")));
        crate::db::record_docker_event(&st.db, &ev("start", t1, None));
        crate::db::record_docker_event(&st.db, &ev("health_status", t1 + 30, Some("unhealthy")));
        let health: Vec<i64> = st
            .db
            .events(&EventFilter {
                subject: Some("web".into()),
                kind_prefix: Some("container.health".into()),
                ..Default::default()
            })
            .iter()
            .map(|e| e.ts)
            .collect();
        assert_eq!(
            health,
            vec![t0 as i64 + 60],
            "the recorder skipped the second one"
        );
        c.created = t1 as i64 - 5;
        let now = t1 as i64 + 180;
        assert_eq!(unhealthy_since(&st.db, &c, None), Some(t1 as i64));
        // Without the start event (missed), the creation time still bounds it.
        let st2 = state_with(sample(), "");
        crate::db::record_docker_event(&st2.db, &ev("health_status", t0 + 60, Some("unhealthy")));
        assert_eq!(unhealthy_since(&st2.db, &c, None), Some(c.created));
        // And an inspect's StartedAt, when there is one.
        assert_eq!(
            unhealthy_since(&st2.db, &c, Some(t1 as i64 + 2)),
            Some(t1 as i64 + 2)
        );
        let w = why_red(&c, None, 0, unhealthy_since(&st.db, &c, None), now).unwrap();
        assert_eq!(w.health.unwrap().since, Some(t1 as i64));
    }

    #[test]
    fn why_is_this_red() {
        let now = 1_794_650_000;
        let mut c = sample()[0].clone();
        // Healthy and running: nothing.
        assert_eq!(why_red(&c, None, 0, None, now), None);
        // Unhealthy: since when, streak, last output.
        c.health = Some("unhealthy".into());
        let w = why_red(&c, Some(&ins()), 0, None, now).unwrap();
        let h = w.health.as_ref().expect("a failing health check");
        assert_eq!(w.text, "", "nothing else is wrong");
        assert_eq!(h.streak, 4);
        assert_eq!(h.since, Some(1_794_646_800));
        assert!(
            h.probes.len() <= 5 && h.probes[0].2.contains("<b>connection refused</b> on :8080"),
            "{h:?}"
        );
        assert_eq!(w.ts, Some(1_794_646_800)); // 10:00 +01:00, start of the failing run
                                               // The database event wins when there is one.
        let w = why_red(&c, Some(&ins()), 0, Some(now - 600), now).unwrap();
        assert_eq!(w.ts, Some(now - 600));
        // ... but never one from before this run of the container.
        let w = why_red(&c, Some(&ins()), 0, Some(now - 7200), now).unwrap();
        assert_eq!(
            w.health.unwrap().since,
            Some(1_794_646_800),
            "clamped to StartedAt"
        );
        // Exited 137 with OOMKilled.
        let mut i = ins();
        i.state.running = false;
        i.state.oom_killed = true;
        i.state.exit_code = 137;
        i.state.finished_at = "2026-11-14T10:30:00Z".into();
        let mut d = sample()[1].clone();
        d.exit_code = Some(137);
        let w = why_red(&d, Some(&i), 0, None, now).unwrap();
        assert!(
            w.text.contains("out of memory") && w.text.contains("code 137"),
            "{}",
            w.text
        );
        assert_eq!(w.ts, crate::docker::parse_rfc3339("2026-11-14T10:30:00Z"));
        // 143 and 1, from the list alone.
        d.exit_code = Some(143);
        assert!(why_red(&d, None, 0, None, now)
            .unwrap()
            .text
            .contains("SIGTERM"));
        d.exit_code = Some(1);
        assert!(why_red(&d, None, 0, None, now)
            .unwrap()
            .text
            .contains("application error"));
        d.exit_code = Some(0);
        assert_eq!(why_red(&d, None, 0, None, now), None);
        // Restart loop.
        let mut r = sample()[0].clone();
        r.die_times = vec![(now - 60) as u64, (now - 200) as u64, (now - 400) as u64];
        let w = why_red(&r, None, 2, None, now).unwrap();
        assert!(w.text.contains("Restart loop: 3 restarts in 10 min"));
        assert_eq!(w.ts, Some(now - 60));
        assert!(why_red(&r, None, 5, None, now)
            .unwrap()
            .text
            .contains("5 restarts"));
        assert_eq!(
            exit_meaning(137),
            "killed (SIGKILL: out of memory or docker kill)"
        );
        assert_eq!(exit_meaning(130 + 11), "killed by signal 13");
    }
}
