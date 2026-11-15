//! The history sampler: one background task that samples the host and every
//! running container on a fixed interval (`[history] sample_interval`,
//! default 15 s, at least 5 s), with or without a viewer, and writes one
//! small batch to the database.
//!
//! It owns its own `system::Sampler` and `docker::StatsSampler` instead of
//! sharing the ones in `AppState`. CPU and network figures are rates over the
//! time since the previous reading, so two loops reading one sampler would
//! each see a shorter, noisier window, and the history would depend on
//! whether somebody is watching. With separate instances every stored point
//! is an average over exactly one sample interval, and the live 3 s view is
//! untouched. The extra cost while someone watches is a handful of small
//! file reads every 15 s.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::state::{AppState, Env};

pub const MIN_INTERVAL: Duration = Duration::from_secs(5);
pub const MAX_INTERVAL: Duration = Duration::from_secs(300);

pub struct Samplers {
    sys: crate::system::Sampler,
    ctr: crate::docker::StatsSampler,
}

impl Samplers {
    pub fn new(env: &Env) -> Self {
        Self {
            sys: crate::system::Sampler::new(&env.host_proc, &env.host_sys),
            ctr: crate::docker::StatsSampler::new(&env.host_sys),
        }
    }

    /// Sample once and write the rows. The first call only primes the rate
    /// counters: CPU and network are stored as missing, containers skipped.
    pub fn tick(&mut self, state: &AppState, cfg: &Config, now: i64) {
        let s = self.sys.sample(&cfg.system);
        state.db.record_system(now, &s);

        let snap = state.snapshot();
        let stats = self.ctr.sample(&snap.containers);
        let rows: Vec<_> = snap
            .containers
            .iter()
            .filter_map(|c| stats.get(&c.id).map(|s| (c.name.as_str(), s)))
            .collect();
        state.db.record_containers(now, &rows);
        // Wave 1 hooks (each owned by its own module).
        super::ctrx::on_tick(state, now);
        crate::host::on_tick(state, now);
    }
}

pub fn interval(cfg: &Config) -> Duration {
    cfg.history
        .sample_interval
        .0
        .clamp(MIN_INTERVAL, MAX_INTERVAL)
}

pub async fn run(state: Arc<AppState>) {
    if !state.db.is_enabled() {
        crate::warn!("history sampler not started: no database");
        return;
    }
    let mut samplers = Samplers::new(&state.env);
    let mut next = Instant::now();
    let mut was_enabled = true;
    loop {
        let cfg = state.config.current();
        let step = interval(&cfg);
        if cfg.history.enabled {
            if !was_enabled {
                // Re-prime: a rate over the paused time would be meaningless.
                samplers = Samplers::new(&state.env);
            }
            // A few file reads and one small transaction: inline is fine.
            samplers.tick(&state, &cfg, crate::util::unix_now() as i64);
        }
        was_enabled = cfg.history.enabled;
        // The interval is re-read every round, so a config reload applies at once.
        next += step;
        let t = Instant::now();
        if next < t {
            next = t + step; // fell behind (suspend, slow disk): do not burst
        }
        tokio::time::sleep_until(next.into()).await;
    }
}
