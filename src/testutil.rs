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
        trusted_proxies: vec![],
        max_connections_per_peer: 32,
    };
    let mut st = AppState::new(env);
    // Tests never depend on the cgroup they happen to run in.
    Arc::get_mut(&mut st)
        .expect("a fresh state")
        .credential_queue
        .memory_limit = || None;
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

// Count requested Rust allocation bytes on the measuring test thread only.
struct MeasuringAllocator;

#[global_allocator]
static ALLOCATOR: MeasuringAllocator = MeasuringAllocator;

thread_local! {
    static HEAP: std::cell::Cell<Option<(isize, isize)>> = const { std::cell::Cell::new(None) };
}

fn change_heap(delta: isize) {
    let _ = HEAP.try_with(|heap| {
        if let Some((live, peak)) = heap.get() {
            let live = live + delta;
            heap.set(Some((live, peak.max(live))));
        }
    });
}

unsafe impl std::alloc::GlobalAlloc for MeasuringAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let ptr = unsafe { std::alloc::System.alloc(layout) };
        if !ptr.is_null() {
            change_heap(layout.size() as isize);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        change_heap(-(layout.size() as isize));
        unsafe { std::alloc::System.dealloc(ptr, layout) };
    }
}

pub fn measure_allocations<T>(f: impl FnOnce() -> T) -> (T, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            HEAP.set(None);
        }
    }

    assert!(HEAP.replace(Some((0, 0))).is_none());
    let _reset = Reset;
    let result = f();
    (result, HEAP.get().unwrap().1 as usize)
}
