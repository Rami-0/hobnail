//! "Send test" for a saved channel, through the alerts channel sender. The
//! error text never contains a secret (see `alerts::channels`).

use crate::config::Config;
use crate::state::AppState;

pub async fn send_test(state: &AppState, cfg: &Config, name: &str) -> Result<(), String> {
    crate::alerts::send_test(cfg, name, &state.env.hostname).await
}
