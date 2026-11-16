//! The lossless patch engine on top of `toml_edit`.
//!
//! Rules that keep a hand-written file intact:
//! - A key whose value is already semantically equal to the wanted value is
//!   **not touched** (its formatting, quotes and trailing comment survive).
//! - A changed scalar keeps the old value's decor (the `# comment` after it).
//!   Keys are never re-created (`Table::insert` would reset the comment lines
//!   above a key), only their values are replaced in place.
//! - Keys the form does not manage are left alone.
//! - Arrays of tables (`[[widgets]]`, `[[bookmarks]]`, …) are reordered by
//!   moving whole entries (with their comments) and handing the entries the
//!   document positions the array already occupied, so every other table
//!   stays where it was. Comments belong to what follows them: they move with
//!   their entry. The one exception: deleting the *first* entry hands its
//!   leading comment (usually a section header) to the next entry, so a
//!   delete never loses a section comment.

use serde_json::{Map, Value as J};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, TableLike, Value};

/// Lowercase hex SHA-256 (the content hash used for stale-write checks).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut s = String::with_capacity(64);
    for b in d.as_ref() {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Every line break of `text` is CRLF (and there is at least one).
pub fn is_crlf(text: &str) -> bool {
    let lf = text.matches('\n').count();
    lf > 0 && text.matches("\r\n").count() == lf
}

/// `new` with the line breaks of `old`: a file written with CRLF only stays
/// so when its text comes back with LF (a browser's textarea gives LF) or
/// gets new lines from an edit. Anything else is returned as it is.
pub fn keep_line_ends(old: &str, new: String) -> String {
    if !is_crlf(old) || is_crlf(&new) || !new.contains('\n') {
        return new;
    }
    let mut out = String::with_capacity(new.len() + new.len() / 32);
    let mut prev = '\0';
    for c in new.chars() {
        if c == '\n' && prev != '\r' {
            out.push('\r');
        }
        out.push(c);
        prev = c;
    }
    out
}

/// 1-based line and column of a byte offset.
pub fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let mut off = offset.min(text.len());
    while !text.is_char_boundary(off) {
        off -= 1;
    }
    let before = &text[..off];
    let line = before.matches('\n').count() + 1;
    let col = before.len() - before.rfind('\n').map(|i| i + 1).unwrap_or(0) + 1;
    (line, col)
}

/// Parse for editing. Errors carry line and column.
pub fn parse(text: &str) -> Result<DocumentMut, (String, Option<(usize, usize)>)> {
    text.parse::<DocumentMut>().map_err(|e| {
        let pos = e.span().map(|s| line_col(text, s.start));
        (e.message().to_string(), pos)
    })
}

// ------------------------------------------------------------ JSON <-> TOML

/// A TOML item as JSON (for GET responses and "is it already equal?").
pub fn item_json(it: &Item) -> J {
    match it {
        Item::None => J::Null,
        Item::Value(v) => value_json(v),
        Item::Table(t) => table_json(t),
        Item::ArrayOfTables(a) => J::Array(a.iter().map(|t| table_json(t)).collect()),
    }
}

pub fn table_json(t: &dyn TableLike) -> J {
    let mut m = Map::new();
    for (k, v) in t.iter() {
        m.insert(k.to_string(), item_json(v));
    }
    J::Object(m)
}

pub fn value_json(v: &Value) -> J {
    match v {
        Value::String(s) => J::String(s.value().clone()),
        Value::Integer(i) => J::from(*i.value()),
        Value::Float(f) => serde_json::Number::from_f64(*f.value())
            .map(J::Number)
            .unwrap_or(J::Null),
        Value::Boolean(b) => J::Bool(*b.value()),
        Value::Datetime(d) => J::String(d.value().to_string()),
        Value::Array(a) => J::Array(a.iter().map(value_json).collect()),
        Value::InlineTable(t) => table_json(t),
    }
}

/// JSON to a TOML value. `null` inside arrays is refused; `null` members of
/// objects are skipped (they mean "not set").
pub fn to_toml(v: &J) -> Result<Value, String> {
    Ok(match v {
        J::Null => return Err("null is not allowed here".into()),
        J::Bool(b) => Value::from(*b),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from(i)
            } else if let Some(f) = n.as_f64() {
                Value::from(f)
            } else {
                return Err(format!("number {n} is out of range"));
            }
        }
        J::String(s) => Value::from(s.as_str()),
        J::Array(a) => {
            let mut arr = Array::new();
            for x in a {
                arr.push(to_toml(x)?);
            }
            Value::Array(arr)
        }
        J::Object(m) => {
            let mut t = InlineTable::new();
            for (k, x) in m {
                if !x.is_null() {
                    t.insert(k.as_str(), to_toml(x)?);
                }
            }
            Value::InlineTable(t)
        }
    })
}

/// How new nested values are written into a standard table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Everything inline (`status = { field = "x" }`, `ok = [1, 2]`).
    Inline,
    /// Objects with more than three entries become `[parent.key]` sub-tables
    /// and arrays of objects become `[[parent.key]]` (like the README
    /// examples for `[widgets.fields]` and `[[widgets.rows]]`).
    Tables,
}

fn new_item(v: &J, style: Style) -> Result<Item, String> {
    if style == Style::Tables {
        match v {
            J::Object(m) if m.len() > 3 => {
                let mut t = Table::new();
                // Compact, like a hand-written `[widgets.fields]` right
                // under its entry.
                t.decor_mut().set_prefix("");
                for (k, x) in m {
                    set_json(&mut t, k, Some(x), Style::Inline)?;
                }
                return Ok(Item::Table(t));
            }
            J::Array(a) if !a.is_empty() && a.iter().all(J::is_object) => {
                let mut aot = ArrayOfTables::new();
                for x in a {
                    let mut t = Table::new();
                    t.decor_mut().set_prefix("");
                    patch_object(&mut t, x.as_object().unwrap(), Style::Inline)?;
                    aot.push(t);
                }
                return Ok(Item::ArrayOfTables(aot));
            }
            _ => {}
        }
    }
    Ok(Item::Value(to_toml(v)?))
}

/// Set `key` to `v` (`None`/`null` removes it). A value that is already equal
/// is left untouched; a changed value keeps its decor; an existing sub-table
/// or array of tables is patched in place instead of replaced.
pub fn set_json(
    t: &mut dyn TableLike,
    key: &str,
    v: Option<&J>,
    style: Style,
) -> Result<(), String> {
    let v = match v {
        None | Some(J::Null) => {
            t.remove(key);
            return Ok(());
        }
        Some(v) => v,
    };
    if let Some(existing) = t.get(key) {
        if item_json(existing) == *v {
            return Ok(());
        }
    }
    match (t.get_mut(key), v) {
        (Some(Item::Table(sub)), J::Object(m)) => patch_object(sub, m, Style::Inline),
        (Some(Item::Value(Value::InlineTable(sub))), J::Object(m)) => {
            patch_object(sub, m, Style::Inline)
        }
        (Some(Item::ArrayOfTables(aot)), J::Array(items)) if items.iter().all(J::is_object) => {
            for (i, x) in items.iter().enumerate() {
                let m = x.as_object().unwrap();
                match aot.get_mut(i) {
                    Some(sub) => patch_object(sub, m, Style::Inline)?,
                    None => {
                        let mut sub = Table::new();
                        sub.decor_mut().set_prefix("");
                        patch_object(&mut sub, m, Style::Inline)?;
                        aot.push(sub);
                    }
                }
            }
            while aot.len() > items.len() {
                aot.remove(aot.len() - 1);
            }
            if aot.is_empty() {
                t.remove(key);
            }
            Ok(())
        }
        (Some(Item::Value(old)), _) => {
            let mut nv = to_toml(v)?;
            *nv.decor_mut() = old.decor().clone();
            *old = nv;
            Ok(())
        }
        (Some(other), _) => {
            *other = new_item(v, style)?;
            Ok(())
        }
        (None, _) => {
            let it = new_item(v, style)?;
            t.insert(key, it);
            Ok(())
        }
    }
}

/// Make the table hold exactly the members of `m` (others removed). Existing
/// keys keep their place; new keys are appended in `m`'s order.
pub fn patch_object(t: &mut dyn TableLike, m: &Map<String, J>, style: Style) -> Result<(), String> {
    let stale: Vec<String> = t
        .iter()
        .map(|(k, _)| k.to_string())
        .filter(|k| m.get(k).is_none_or(J::is_null))
        .collect();
    for k in stale {
        t.remove(&k);
    }
    for (k, v) in m {
        set_json(t, k, Some(v), style)?;
    }
    Ok(())
}

/// Set the `managed` keys of `t` from `m` (missing or null = remove). Keys
/// that are not managed are left alone. New keys follow `managed` order.
pub fn patch_managed(
    t: &mut Table,
    m: &Map<String, J>,
    managed: &[&str],
    style: Style,
) -> Result<(), String> {
    for k in managed {
        set_json(t, k, m.get(*k), style)?;
    }
    Ok(())
}

/// A standard (non-inline) sub-table of `parent`, created if missing. A new
/// container table is implicit (no empty `[containers]` header is printed).
pub fn table_mut<'a>(
    parent: &'a mut Table,
    key: &str,
    implicit: bool,
) -> Result<&'a mut Table, String> {
    if !parent.contains_key(key) {
        let mut t = Table::new();
        t.set_implicit(implicit);
        parent.insert(key, Item::Table(t));
    }
    match parent.get_mut(key) {
        Some(Item::Table(t)) => Ok(t),
        _ => Err(format!(
            "`{key}` is not written as a [{key}] table in the file; edit it in the Config file tab"
        )),
    }
}

/// The `[[key]]` array of tables, created if missing.
pub fn aot_mut<'a>(doc: &'a mut DocumentMut, key: &str) -> Result<&'a mut ArrayOfTables, String> {
    let root = doc.as_table_mut();
    if !root.contains_key(key) {
        root.insert(key, Item::ArrayOfTables(ArrayOfTables::new()));
    }
    match root.get_mut(key) {
        Some(Item::ArrayOfTables(a)) => Ok(a),
        _ => Err(format!(
            "`{key}` is not written as [[{key}]] entries in the file; edit it in the Config file tab"
        )),
    }
}

// ------------------------------------------------------------ positions

/// Positions of `t` and every nested standard table, in visit order.
fn positions(t: &Table, out: &mut Vec<Option<isize>>) {
    out.push(t.position());
    for (_, item) in t.iter() {
        match item {
            Item::Table(sub) => positions(sub, out),
            Item::ArrayOfTables(a) => a.iter().for_each(|sub| positions(sub, out)),
            _ => {}
        }
    }
}

/// Give the tables that had a position the next values from `assign`, in
/// visit order.
fn set_positions(t: &mut Table, assign: &mut impl Iterator<Item = isize>) {
    if t.position().is_some() {
        t.set_position(assign.next());
    }
    for (_, item) in t.iter_mut() {
        match item {
            Item::Table(sub) => set_positions(sub, assign),
            Item::ArrayOfTables(a) => a.iter_mut().for_each(|sub| set_positions(sub, assign)),
            _ => {}
        }
    }
}

/// Rebuild an array of tables. `plan[i]` names the old entry that goes to
/// slot `i` (`None` = a new empty table, patched by the caller). Old entries
/// not named are dropped. The kept entries reuse the document positions the
/// array already had (so the rest of the file does not move), keeping each
/// entry's own tables in their relative order.
///
/// Returns the leading comment of a dropped first entry when there is no
/// entry left to carry it.
pub fn rebuild_aot(aot: &mut ArrayOfTables, plan: &[Option<usize>]) -> Option<String> {
    let mut old: Vec<Option<Table>> = Vec::with_capacity(aot.len());
    while !aot.is_empty() {
        old.push(Some(aot.remove(0)));
    }
    let first_dropped = !old.is_empty() && !plan.contains(&Some(0));
    let mut carried = if first_dropped {
        old[0]
            .as_ref()
            .and_then(|t| t.decor().prefix())
            .and_then(|p| p.as_str())
            .filter(|p| p.contains('#'))
            .map(str::to_string)
    } else {
        None
    };
    // Pool of positions held by the kept entries, sorted.
    let mut per_entry: Vec<Vec<Option<isize>>> = Vec::new();
    let mut pool: Vec<isize> = Vec::new();
    for (i, t) in old.iter().enumerate() {
        let mut p = Vec::new();
        if let Some(t) = t {
            positions(t, &mut p);
        }
        if plan.contains(&Some(i)) {
            pool.extend(p.iter().flatten());
        }
        per_entry.push(p);
    }
    pool.sort_unstable();
    let mut next = 0;
    for slot in plan {
        let mut t = match slot {
            Some(i) => old[*i].take().unwrap_or_default(),
            None => Table::new(),
        };
        if let Some(i) = slot {
            // This entry's positions, ranked, mapped onto the next pool slice.
            let own: Vec<isize> = per_entry[*i].iter().flatten().copied().collect();
            let mut ranked = own.clone();
            ranked.sort_unstable();
            let slice = &pool[next..next + own.len()];
            next += own.len();
            let mapped: Vec<isize> = own
                .iter()
                .map(|p| slice[ranked.binary_search(p).unwrap_or(0)])
                .collect();
            set_positions(&mut t, &mut mapped.into_iter());
        }
        if let Some(c) = carried.take() {
            let own = t
                .decor()
                .prefix()
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .trim_start_matches('\n')
                .to_string();
            let joined = if c.ends_with('\n') {
                format!("{c}{own}")
            } else {
                format!("{c}\n{own}")
            };
            t.decor_mut().set_prefix(joined);
        }
        aot.push(t);
    }
    carried
}

/// When the last entry of an array was deleted, keep its leading comment
/// (a section header) by moving it to the end of the document.
pub fn keep_orphan_comment(doc: &mut DocumentMut, comment: Option<String>) {
    if let Some(c) = comment {
        let trailing = doc.trailing().as_str().unwrap_or("").to_string();
        doc.set_trailing(format!("{trailing}{c}"));
    }
}

// ------------------------------------------------------------ files

/// Write `data` to `path` atomically: a temp file in the same directory,
/// fsync, rename over the target, fsync the directory. The target's file
/// mode is kept (`default_mode` for a new file). A symlink is followed (the
/// link stays, its target is replaced). On error the temp file is removed
/// and the target is untouched.
pub fn write_atomic(path: &Path, data: &[u8], default_mode: u32) -> Result<(), String> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => std::path::PathBuf::from("."),
    };
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let mode = std::fs::metadata(&target)
        .map(|m| m.permissions().mode() & 0o7777)
        .unwrap_or(default_mode);
    let tmp = dir.join(format!(
        ".{name}.rh-tmp.{}.{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)?;
        f.write_all(data)?;
        // The umask may have narrowed `mode` at create time.
        f.set_permissions(std::fs::Permissions::from_mode(mode))?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &target)?;
        if let Ok(d) = std::fs::File::open(&dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("cannot write {}: {e}", target.display()));
    }
    Ok(())
}
