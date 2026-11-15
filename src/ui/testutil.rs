//! Test helpers: a real `AppState` with a temporary data dir and a fixed
//! container snapshot.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::model::{Container, PortMapping, Snapshot};
use crate::state::AppState;

pub fn state_with(containers: Vec<Container>, toml: &str) -> Arc<AppState> {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rh-ui-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), toml).unwrap();
    let env = crate::state::Env {
        listen: vec![],
        data_dir: dir.clone(),
        config_path: dir.join("config.toml"),
        docker_socket: String::new(),
        host_proc: "/nonexistent".into(),
        host_sys: "/nonexistent".into(),
        hostname: "testhost".into(),
        allowed_hosts: vec![],
    };
    let st = AppState::new(env);
    st.containers.send_replace(Arc::new(Snapshot {
        containers,
        fetched_at: 1,
        error: None,
    }));
    st
}

pub fn ctr(name: &str, project: Option<&str>, state: &str) -> Container {
    Container {
        id: format!(
            "{:0<64}",
            name.bytes().map(|b| format!("{b:02x}")).collect::<String>()
        )
        .chars()
        .take(64)
        .collect(),
        name: name.into(),
        image: format!("img/{name}:latest"),
        state: state.into(),
        status: if state == "running" {
            "Up 3 hours".into()
        } else {
            "Exited (1) 2 hours ago".into()
        },
        exit_code: (state == "exited").then_some(1),
        project: project.map(str::to_string),
        service: project.map(|_| name.to_string()),
        ports: vec![PortMapping {
            host_ip: "0.0.0.0".into(),
            host_port: 8080,
            container_port: 80,
            proto: "tcp".into(),
        }],
        ..Default::default()
    }
}

pub fn sample() -> Vec<Container> {
    vec![
        ctr("web", Some("shop"), "running"),
        ctr("db", Some("shop"), "exited"),
        ctr("<script>alert(1)</script>", None, "running"),
    ]
}
