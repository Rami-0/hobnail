//! Published ports: which address a viewer should use, and which ports are web UIs.

use std::net::IpAddr;

use crate::model::PortMapping;

/// Container ports that are never a web UI (databases, mail, queues, DNS).
const NON_HTTP_PORTS: &[u16] = &[
    5432, 3306, 6379, 27017, 1025, 25, 587, 465, 110, 143, 993, 995, 1110, 11211, 5672, 9092, 2181,
    53,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortView {
    /// Host port shown on the badge.
    pub label: String,
    /// Link, for HTTP-ish ports reachable from the viewer.
    pub url: Option<String>,
    /// Published on loopback only: not reachable from another machine.
    pub local: bool,
    /// Tooltip: "8125 → 8025/tcp on 100.64.0.10, 127.0.0.1".
    pub detail: String,
    /// `tcp` or `udp`: a chip that is not a link says it (OV-25).
    pub proto: String,
}

fn is_unspecified(ip: &str) -> bool {
    ip.is_empty() || ip.parse::<IpAddr>().is_ok_and(|a| a.is_unspecified())
}

fn is_loopback(ip: &str) -> bool {
    ip == "localhost" || ip.parse::<IpAddr>().is_ok_and(|a| a.is_loopback())
}

/// Bracket IPv6 literals for use in a URL.
pub(super) fn url_host_part(h: &str) -> String {
    if h.contains(':') && !h.starts_with('[') {
        format!("[{h}]")
    } else {
        h.to_string()
    }
}

/// Which address a viewer on `host` should use for a port published on
/// `bindings` (ip, host_port). None: published on loopback only.
fn pick_binding(bindings: &[(&str, u16)], host: &str) -> Option<(String, u16)> {
    if let Some(&(_, p)) = bindings.iter().find(|(ip, _)| is_unspecified(ip)) {
        return Some((host.to_string(), p));
    }
    if let Some(&(ip, p)) = bindings.iter().find(|(ip, _)| *ip == host) {
        return Some((ip.to_string(), p));
    }
    if is_loopback(host) {
        if let Some(&(_, p)) = bindings.iter().find(|(ip, _)| is_loopback(ip)) {
            return Some((host.to_string(), p));
        }
    }
    bindings
        .iter()
        .find(|(ip, _)| !is_loopback(ip))
        .map(|&(ip, p)| (ip.to_string(), p))
}

/// What one host binding means for a viewer on `host`: `Ok(url)`, or why
/// there is no link (`Err("loopback only" | "not a web port")`).
pub fn binding_link(p: &PortMapping, host: &str) -> Result<String, &'static str> {
    if p.proto != "tcp" || NON_HTTP_PORTS.contains(&p.container_port) {
        return Err("not a web port");
    }
    let h = if is_unspecified(&p.host_ip) {
        host.to_string()
    } else if is_loopback(&p.host_ip) {
        if !is_loopback(host) {
            return Err("loopback only");
        }
        host.to_string()
    } else {
        p.host_ip.clone()
    };
    let scheme = if matches!(p.container_port, 443 | 8443) {
        "https"
    } else {
        "http"
    };
    Ok(format!("{scheme}://{}:{}", url_host_part(&h), p.host_port))
}

/// One badge per published container port.
pub fn port_views(ports: &[PortMapping], host: &str) -> Vec<PortView> {
    let mut keys: Vec<(u16, &str)> = Vec::new();
    for p in ports {
        let k = (p.container_port, p.proto.as_str());
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys.into_iter()
        .map(|(cport, proto)| {
            let bindings: Vec<(&str, u16)> = ports
                .iter()
                .filter(|p| p.container_port == cport && p.proto == proto)
                .map(|p| (p.host_ip.as_str(), p.host_port))
                .collect();
            let chosen = pick_binding(&bindings, host);
            let non_http = proto != "tcp" || NON_HTTP_PORTS.contains(&cport);
            let scheme = if matches!(cport, 443 | 8443) {
                "https"
            } else {
                "http"
            };
            let label = chosen
                .as_ref()
                .map(|c| c.1)
                .unwrap_or(bindings[0].1)
                .to_string();
            let mut ips: Vec<&str> = bindings
                .iter()
                .map(|(ip, _)| if ip.is_empty() { "*" } else { *ip })
                .collect();
            ips.dedup();
            let mut host_ports: Vec<String> = bindings.iter().map(|b| b.1.to_string()).collect();
            host_ports.dedup();
            PortView {
                url: chosen
                    .as_ref()
                    .filter(|_| !non_http)
                    .map(|(h, p)| format!("{scheme}://{}:{p}", url_host_part(h))),
                local: chosen.is_none(),
                detail: format!(
                    "{} → {cport}/{proto} on {}",
                    host_ports.join(", "),
                    ips.join(", ")
                ),
                label,
                proto: proto.to_string(),
            }
        })
        .collect()
}
