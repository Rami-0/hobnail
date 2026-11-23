//! Shared data model. Produced by `docker` and `system`, consumed by `ui`,
//! `alerts` and `live`. Keep this file small and stable: several modules
//! depend on it.

use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// One container as read from `GET /containers/json?all=1`.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Container {
    /// Full 64-char id.
    pub id: String,
    /// Name without the leading `/`.
    pub name: String,
    pub image: String,
    /// Docker `State`: running, exited, paused, restarting, created, dead.
    pub state: String,
    /// Docker human `Status`, e.g. "Up 3 hours (healthy)", "Exited (1) 2 minutes ago".
    pub status: String,
    /// healthy / unhealthy / starting, when the container has a healthcheck.
    pub health: Option<String>,
    /// Exit code for exited containers (parsed from `Status`).
    pub exit_code: Option<i64>,
    /// `com.docker.compose.project` label.
    pub project: Option<String>,
    /// `com.docker.compose.service` label.
    pub service: Option<String>,
    pub labels: BTreeMap<String, String>,
    pub ports: Vec<PortMapping>,
    /// Unix seconds.
    pub created: i64,
    /// Unix seconds of recent `die` events seen on the events stream (last hour).
    pub die_times: Vec<u64>,
    /// Docker networks it is attached to, with its address on each.
    pub networks: Vec<NetworkAddr>,
}

/// A container's address on one Docker network.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct NetworkAddr {
    pub name: String,
    /// IPv4 address; empty when Docker reports none (host or none mode).
    pub ip: String,
}

impl Container {
    pub fn is_running(&self) -> bool {
        self.state == "running"
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct PortMapping {
    /// Host address the port is published on ("0.0.0.0", "127.0.0.1", "100.x", "::").
    pub host_ip: String,
    pub host_port: u16,
    pub container_port: u16,
    /// "tcp" / "udp".
    pub proto: String,
}

/// The last container list, with when it was fetched and any error.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    pub containers: Vec<Container>,
    /// Unix seconds; 0 = never fetched.
    pub fetched_at: u64,
    pub error: Option<String>,
}

/// Live resource usage of one container (from cgroup v2 files).
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
pub struct ContainerStats {
    /// Percent of one CPU core (100.0 = one full core), like `docker stats`.
    pub cpu_percent: f64,
    pub mem_bytes: u64,
}

pub type StatsMap = HashMap<String, ContainerStats>;

/// Host stats shown by the system widgets.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SystemStats {
    /// 0..=100 across all cores; None until two samples exist.
    pub cpu_percent: Option<f64>,
    pub cpu_cores: u32,
    pub load: [f64; 3],
    pub mem_total: u64,
    /// total - available, like `free`.
    pub mem_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disks: Vec<DiskStats>,
    /// Bytes per second since the previous sample; None on the first sample.
    pub net_rx_bps: Option<f64>,
    pub net_tx_bps: Option<f64>,
    /// Interface used for the network numbers.
    pub net_iface: Option<String>,
    /// CPU package temperature in °C.
    pub temp_c: Option<f64>,
    pub uptime_secs: u64,
    /// Unix seconds when sampled.
    pub sampled_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct DiskStats {
    /// Label shown on the page (e.g. "/").
    pub label: String,
    pub total: u64,
    pub used: u64,
    /// Available to unprivileged users (`df` "Avail"; excludes reserved blocks).
    pub avail: u64,
}
