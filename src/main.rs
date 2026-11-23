//! hobnail: a tiny home-server dashboard.
//!
//! Subcommands:
//!   serve                  run the dashboard (default)
//!   healthcheck            GET /healthz on the first RH_LISTEN address (for Docker)
//!   check-config [path]    validate a config file and exit
//!   notify-test <channel>  send a test notification to a configured channel
//!   set-password [name]    recover the owner account (password from TTY or stdin)
//!   reset-password <name>  reset an existing account password and revoke sessions
//!   list-legacy-hashes     list usernames still on the 19 MiB profile
//!   mock-receiver [addr]   local fake webhook/Slack/Telegram receiver for tests
//!   version

mod alerts;
mod api;
mod auth;
mod board_grid;
mod config;
mod db;
mod docker;
mod host;
mod http;
mod live;
mod log;
mod logs;
mod mock;
mod model;
mod providers;
mod read_model;
mod settings;
mod state;
mod system;
#[cfg(test)]
mod testutil;
mod util;
mod widgets;

use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("serve");
    let rest = args.get(1..).unwrap_or(&[]);

    // One thread for everything. A dashboard does not need a worker per core,
    // and every thread costs stack and allocator arenas. DNS lookups and the
    // like may use up to two short-lived blocking threads.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(2)
        .thread_keep_alive(Duration::from_secs(5))
        .build()
        .expect("tokio runtime");

    let code = match cmd {
        "serve" => rt.block_on(serve()),
        "healthcheck" => rt.block_on(healthcheck()),
        "check-config" => check_config(rest),
        "notify-test" => rt.block_on(alerts::cli_notify_test(rest)),
        "set-password" => auth::cli_set_password(rest, false),
        "reset-password" => auth::cli_set_password(rest, true),
        "list-legacy-hashes" => auth::cli_list_legacy_hashes(rest),
        "mock-receiver" => rt.block_on(mock::run(rest)),
        "version" | "--version" | "-V" => {
            let b = state::BuildInfo::current();
            println!(
                "hobnail {} ({} built {}) content {}",
                b.version,
                b.sha,
                b.time,
                api::CONTENT_HASH
            );
            0
        }
        _ => {
            eprintln!(
                "usage: hobnail [serve | healthcheck | check-config [path] | notify-test <channel> | set-password [username] | reset-password <username> | list-legacy-hashes | mock-receiver [addr] | version]"
            );
            2
        }
    };
    std::process::exit(code);
}

async fn serve() -> i32 {
    let t0 = std::time::Instant::now();
    let env = match state::Env::from_env() {
        Ok(e) => e,
        Err(e) => {
            error!("bad environment", error = e);
            return 2;
        }
    };
    let st = state::AppState::new(env);
    info!(
        "starting",
        version = st.build.version,
        sha = st.build.sha,
        built = st.build.time,
        data_dir = st.env.data_dir.display(),
    );

    if let Ok(users) = st.db.legacy_hash_users() {
        if !users.is_empty() {
            let message = if auth::legacy_verify_fits(auth::read_memory_limit()) {
                "legacy Argon2id hashes: first sign-in needs about 20.5 MiB; allow more than 20 MiB until each account signs in or is reset with hobnail reset-password <username>; check with hobnail list-legacy-hashes"
            } else {
                "legacy Argon2id hashes: the memory limit is too small to verify them, so these accounts cannot sign in; reset each with hobnail reset-password <username> without the limit or from a one-off container with the server stopped; check with hobnail list-legacy-hashes"
            };
            warn!(message, count = users.len());
        }
    }

    // The sign-in floor applies while legacy hashes remain (or the scan
    // fails); measure the hash speed once instead of starting from 150 ms.
    if !st
        .db
        .legacy_hash_users()
        .is_ok_and(|users| users.is_empty())
    {
        let s = st.clone();
        tokio::spawn(async move { auth::calibrate_floor(&s).await });
    }

    st.db.record_event(
        util::unix_now() as i64,
        "app.start",
        "info",
        "",
        &format!("version {} ({})", st.build.version, st.build.sha),
    );

    // Bind all listeners first so a bad address fails fast.
    let mut servers = Vec::new();
    for addr in st.env.listen.clone() {
        let s = st.clone();
        servers.push(tokio::spawn(async move {
            if let Err(e) = http::server::serve(s, addr).await {
                error!("server failed", error = e);
                std::process::exit(1);
            }
        }));
    }

    tokio::spawn(live::config_watcher(st.clone()));
    tokio::spawn(live::run(st.clone()));
    tokio::spawn(docker::run(st.clone()));
    tokio::spawn(alerts::run(st.clone()));
    tokio::spawn(db::sampler::run(st.clone()));
    tokio::spawn(logs::run(st.clone()));
    tokio::spawn(db::maintain::run(st.clone()));

    tokio::task::yield_now().await;
    info!("ready", startup_ms = t0.elapsed().as_millis());

    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("SIGTERM handler");
    tokio::select! {
        _ = term.recv() => info!("SIGTERM, shutting down"),
        _ = tokio::signal::ctrl_c() => info!("SIGINT, shutting down"),
    }
    0
}

async fn healthcheck() -> i32 {
    let addr = std::env::var("RH_LISTEN")
        .ok()
        .and_then(|s| s.split(',').next().map(|s| s.trim().to_string()))
        .and_then(|s| s.parse::<std::net::SocketAddr>().ok())
        .map(|mut a| {
            if a.ip().is_unspecified() {
                a.set_ip(std::net::Ipv4Addr::LOCALHOST.into());
            }
            a
        })
        .unwrap_or_else(|| "127.0.0.1:3200".parse().unwrap());
    let req = http::client::Request::get(format!("http://{addr}/healthz"))
        .timeout(Duration::from_secs(3));
    match http::client::send(req, &config::HttpPolicy::default()).await {
        Ok(r) if r.is_success() => 0,
        Ok(r) => {
            eprintln!("healthz returned {}", r.status);
            1
        }
        Err(e) => {
            eprintln!("healthz: {e}");
            1
        }
    }
}

fn check_config(args: &[String]) -> i32 {
    let path = args.first().cloned().unwrap_or_else(|| {
        std::env::var("RH_CONFIG").unwrap_or_else(|_| {
            format!(
                "{}/config.toml",
                std::env::var("RH_DATA_DIR").unwrap_or_else(|_| "./data".into())
            )
        })
    });
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{path}: {e}");
            return 1;
        }
    };
    match config::parse(&text, &path) {
        Ok(c) => {
            println!(
                "{path}: OK ({} groups, {} overrides, {} bookmarks, {} widgets, {} channels, {} alert rules)",
                c.groups.len(),
                c.containers.len(),
                c.bookmarks.len(),
                c.widgets.len(),
                c.channels.len(),
                c.alerts.len()
            );
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
