//! The config file as it is on disk now, for lists rendered on the server
//! (DESIGN.md §11.14, Y18): read with the save pipeline's own
//! `settings::read_current`, so the hash a page carries is exactly the
//! `base_hash` a save is checked against. Read per request and dropped:
//! nothing is cached.

use serde_json::{Map, Value as J};

use crate::config::Config;
use crate::state::AppState;

pub struct Disk {
    /// SHA-256 of the file's bytes ("" when it cannot be read).
    pub hash: String,
    /// The parsed file, or why it does not load.
    pub cfg: Result<Config, String>,
    /// The file as JSON, values exactly as written (`"30s"` stays a string,
    /// keys left out stay out): what the forms show. `Null` when it does
    /// not parse.
    pub doc: J,
}

impl Disk {
    pub fn read(state: &AppState) -> Disk {
        match crate::settings::read_current(state) {
            Ok(cur) => Disk {
                cfg: crate::config::parse(&cur.text, "config.toml"),
                doc: crate::settings::parse_doc_text(&cur.text)
                    .map(|d| crate::settings::item_json(d.as_item()))
                    .unwrap_or(J::Null),
                hash: cur.hash,
            },
            Err(e) => Disk {
                hash: String::new(),
                doc: J::Null,
                cfg: Err(e.body["error"]
                    .as_str()
                    .unwrap_or("cannot read the config file")
                    .to_string()),
            },
        }
    }
}

impl Disk {
    /// `[channels]` as written: name → table.
    pub fn channels(&self) -> Map<String, J> {
        match self.doc.get("channels") {
            Some(J::Object(m)) => m.clone(),
            _ => Map::new(),
        }
    }

    /// One `[[array]]` as written (`widgets`, `alerts`).
    pub fn list(&self, key: &str) -> Vec<Map<String, J>> {
        match self.doc.get(key) {
            Some(J::Array(a)) => a.iter().filter_map(|x| x.as_object().cloned()).collect(),
            _ => Vec::new(),
        }
    }
}
