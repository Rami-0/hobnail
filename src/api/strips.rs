//! `GET /api/v1/strips` (Public, `no-store`): the 24 h health strips of the
//! listed containers and their stacks, from `db.health_strips` (cached
//! there for 30 s). Containers the lists do not show are left out.
//!
//! Cell codes (worst last): `0` no data (the app was not recording), `1`
//! not running, `2` running, `3` restarted (or exited cleanly without a
//! stop), `4` problem (crashed, killed for memory, or unhealthy). A stack's
//! strip is its containers merged: worst of, but one running member is
//! enough for "running".

use std::collections::BTreeMap;

use serde::Serialize;

use crate::db::strips::Cell;
use crate::read_model as view;
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Strips {
    /// The last cell is the one holding `now`; cell `i` covers
    /// `[to - (buckets - i) * step, to - (buckets - i - 1) * step)` with
    /// `to = (floor(now / step) + 1) * step`.
    pub now: i64,
    /// Seconds per cell.
    pub step: i64,
    pub buckets: u32,
    /// Container name → `buckets` cell codes, oldest first.
    pub containers: BTreeMap<String, Vec<u8>>,
    /// Stack (group key) → cell codes.
    pub stacks: BTreeMap<String, Vec<u8>>,
}

pub fn code(c: Cell) -> u8 {
    match c {
        Cell::Unknown => 0,
        Cell::Off => 1,
        Cell::Ok => 2,
        Cell::Warn => 3,
        Cell::Bad => 4,
    }
}

pub fn strips(state: &AppState, now: i64, visible: &super::access::ContainerVisibility) -> Strips {
    let cfg = state.config.current();
    let snap = state.snapshot();
    // Visibility only: stats and the host do not change who is listed.
    let mut groups = view::build_groups(&cfg, &snap, &Default::default(), "");
    let st = view::strips(state, now);
    let codes = |s: &crate::db::strips::Strip| s.cells.iter().map(|c| code(*c)).collect();
    let mut containers = BTreeMap::new();
    let mut stacks = BTreeMap::new();
    for g in &mut groups {
        g.cards.retain(|c| !c.container || visible.allows(&c.name));
        let mut any = false;
        for c in g.cards.iter().filter(|c| c.container) {
            any = true;
            containers.insert(c.name.clone(), codes(&st.get(&c.name)));
        }
        if any {
            stacks.insert(g.name.clone(), codes(&view::stack_strip(&st, g)));
        }
    }
    Strips {
        now,
        step: st.step,
        buckets: st.buckets() as u32,
        containers,
        stacks,
    }
}

pub async fn get(
    state: &std::sync::Arc<AppState>,
    visible: super::access::ContainerVisibility,
) -> hyper::Response<crate::http::server::Body> {
    let st = state.clone();
    let now = crate::util::unix_now() as i64;
    match tokio::task::spawn_blocking(move || strips(&st, now, &visible)).await {
        Ok(v) => super::json(hyper::StatusCode::OK, &v),
        Err(_) => super::containers::internal(),
    }
}
