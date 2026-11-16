//! The config file as it is on disk now, for lists rendered on the server
//! (DESIGN.md §11.14, Y18): read with the save pipeline's own
//! `settings::read_current`, so the hash a page carries is exactly the
//! `base_hash` a save is checked against. Read per request and dropped:
//! nothing is cached.

use crate::config::Config;
use crate::state::AppState;

pub struct Disk {
    /// SHA-256 of the file's bytes ("" when it cannot be read).
    pub hash: String,
    /// The parsed file, or why it does not load.
    pub cfg: Result<Config, String>,
}

impl Disk {
    pub fn read(state: &AppState) -> Disk {
        match crate::settings::read_current(state) {
            Ok(cur) => Disk {
                cfg: crate::config::parse(&cur.text, "config.toml"),
                hash: cur.hash,
            },
            Err(e) => Disk {
                hash: String::new(),
                cfg: Err(e.body["error"]
                    .as_str()
                    .unwrap_or("cannot read the config file")
                    .to_string()),
            },
        }
    }
}
