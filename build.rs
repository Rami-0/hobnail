//! Build script for embedding the web client and recording the build content hash.
//!
//! - Walks `web/dist` (or `$RH_WEB_DIST`) and writes `$OUT_DIR/web_assets.rs`,
//!   a sorted table of every file (raw bytes and, when present, the sibling
//!   `<file>.gz` written by `web/scripts/compress.mjs`), all `include_bytes!`.
//!   Served by `src/http/assets.rs`. No runtime compression, no crate.
//! - No `index.html` in the dist: an empty table (so `cargo test` works
//!   without a client build), or a panic when `RH_REQUIRE_WEB_DIST=1` (the
//!   image build sets it).
//! - `RH_CONTENT_HASH` (computed by the Dockerfile over the build inputs) is
//!   compiled in; it changing re-runs this script and rebuilds the crate, so
//!   an mtime-only cargo check cannot ship a stale binary (plan 4.4).
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RH_CONTENT_HASH");
    println!("cargo:rerun-if-env-changed=RH_WEB_DIST");
    println!("cargo:rerun-if-env-changed=RH_REQUIRE_WEB_DIST");
    let h = std::env::var("RH_CONTENT_HASH").unwrap_or_else(|_| "dev".into());
    println!("cargo:rustc-env=RH_CONTENT_HASH={h}");

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let dist = match std::env::var("RH_WEB_DIST") {
        Ok(d) if !d.is_empty() => manifest.join(d),
        _ => manifest.join("web/dist"),
    };
    // Watch the dir even when it does not exist yet: a later client build
    // creating it re-runs this script.
    println!("cargo:rerun-if-changed={}", dist.display());
    let require = std::env::var("RH_REQUIRE_WEB_DIST").as_deref() == Ok("1");

    let mut files = Vec::new();
    if dist.is_dir() {
        walk(&dist, &dist, &mut files);
    }
    files.sort();
    let has_index = files.iter().any(|(rel, _)| rel == "index.html");
    if !has_index {
        if require {
            panic!(
                "RH_REQUIRE_WEB_DIST=1 but {} has no index.html: build the client first \
                 (scripts/web.sh run build, or the Dockerfile's web stage)",
                dist.display()
            );
        }
        files.clear();
    }

    let mut out = String::from("&[\n");
    for (rel, abs) in &files {
        let gz = PathBuf::from(format!("{}.gz", abs.display()));
        let gz = if gz.is_file() {
            println!("cargo:rerun-if-changed={}", gz.display());
            format!("Some(include_bytes!({:?}))", gz.display().to_string())
        } else {
            "None".into()
        };
        let _ = writeln!(
            out,
            "    Asset {{ path: {rel:?}, ctype: {:?}, raw: include_bytes!({:?}), gz: {gz}, immutable: {} }},",
            content_type(rel),
            abs.display().to_string(),
            rel.starts_with("assets/"),
        );
    }
    out.push_str("]\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("web_assets.rs");
    std::fs::write(&dest, out).expect("write web_assets.rs");
}

/// Collect `(path relative to dist with '/' separators, absolute path)` for
/// every file except the `.gz` copies; emits rerun-if-changed for every dir
/// and file so adding, removing or rewriting one re-runs the script.
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let rd = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
    for e in rd {
        let e = e.expect("dir entry");
        let p = e.path();
        let ft = e.file_type().expect("file type");
        if ft.is_dir() {
            walk(root, &p, out);
        } else if ft.is_file() || (ft.is_symlink() && p.is_file()) {
            if p.extension().is_some_and(|x| x == "gz") {
                continue;
            }
            println!("cargo:rerun-if-changed={}", p.display());
            let rel = p
                .strip_prefix(root)
                .expect("under root")
                .components()
                .map(|c| c.as_os_str().to_str().expect("UTF-8 file name"))
                .collect::<Vec<_>>()
                .join("/");
            let abs = std::fs::canonicalize(&p).expect("canonicalize");
            out.push((rel, abs));
        }
    }
}

fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match ext.to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}
