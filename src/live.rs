//! The refresh loop for what viewers see. It does nothing while nobody has
//! the page open: it parks on `state.wake` until the first SSE client
//! connects. Alerting has its own background task (`alerts::run`), which
//! keeps running with no viewers, but only for checks that rules use.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::state::AppState;

const TICK: Duration = Duration::from_secs(1);
const STATS_EVERY: Duration = Duration::from_secs(5);

/// Watch the config file; push a page reload to viewers when it changes.
pub async fn config_watcher(state: Arc<AppState>) {
    let mut iv = tokio::time::interval(Duration::from_secs(2));
    iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        iv.tick().await;
        if state.config.poll() {
            state.widgets.lock().unwrap().clear();
            state.changed("reload");
        }
    }
}

pub async fn run(state: Arc<AppState>) {
    let in_flight: Arc<Mutex<HashSet<String>>> = Arc::default();
    let mut last_sys: Option<Instant> = None;
    let mut last_stats: Option<Instant> = None;
    loop {
        if state.viewers() == 0 {
            // Park. Wake when a viewer connects (or every minute, harmlessly).
            let _ = tokio::time::timeout(Duration::from_secs(60), state.wake.notified()).await;
            continue;
        }
        let cfg = state.config.current();
        let now = Instant::now();

        if cfg.system.enabled && last_sys.is_none_or(|t| now - t >= cfg.system.refresh.0) {
            last_sys = Some(now);
            refresh_system(&state);
        }

        if cfg.page.show_stats && last_stats.is_none_or(|t| now - t >= STATS_EVERY) {
            last_stats = Some(now);
            refresh_container_stats(&state);
        }

        let unix_now = crate::util::unix_now();
        for w in cfg.widgets.iter().filter(|_| state.widget_viewers() > 0) {
            let due = {
                let cache = state.widgets.lock().unwrap();
                cache
                    .get(&w.id)
                    .is_none_or(|o| o.fetched_at + w.refresh.0.as_secs().max(1) <= unix_now)
            };
            if !due || !in_flight.lock().unwrap().insert(w.id.clone()) {
                continue;
            }
            let (st, w, fl, run_config) =
                (state.clone(), w.clone(), in_flight.clone(), cfg.clone());
            tokio::spawn(async move {
                let out = crate::widgets::run(&w, &run_config.http).await;
                {
                    let mut cache = st.widgets.lock().unwrap();
                    if Arc::ptr_eq(&run_config, &st.config.current()) {
                        cache.insert(w.id.clone(), out);
                    }
                }
                fl.lock().unwrap().remove(&w.id);
                st.changed(&format!("w-{}", w.id));
            });
        }
        tokio::time::sleep(TICK).await;
    }
}

pub fn refresh_system(state: &AppState) {
    let cfg = state.config.current();
    {
        let mut g = state.system.lock().unwrap();
        let s = g.0.sample(&cfg.system);
        g.1 = Some(s);
    }
    state.changed("sys");
}

pub fn refresh_container_stats(state: &AppState) {
    let snap = state.snapshot();
    {
        let mut g = state.container_stats.lock().unwrap();
        let m = g.0.sample(&snap.containers);
        g.1 = m;
    }
    state.changed("groups");
}
