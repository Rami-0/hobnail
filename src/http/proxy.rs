//! Forwarding headers are meaningful only on a connection from a trusted peer.

use std::net::{IpAddr, Ipv4Addr};

use hyper::Request;

use super::server::Peer;
use crate::state::Env;

fn peer_ip<B>(req: &Request<B>) -> Option<IpAddr> {
    req.extensions()
        .get::<Peer>()
        .map(|peer| peer.0.ip().to_canonical())
}

/// The listener is plain TCP. Only the configured TLS terminator can attest to
/// HTTPS. The request-target scheme and Origin header come from the client.
pub(crate) fn is_https<B>(req: &Request<B>, env: &Env) -> bool {
    if !peer_ip(req).is_some_and(|ip| env.trusted_proxies.contains(&ip)) {
        return false;
    }
    let mut proto = req.headers().get_all("x-forwarded-proto").iter();
    let https = proto.next().is_some_and(|value| {
        value
            .to_str()
            .is_ok_and(|s| s.trim().eq_ignore_ascii_case("https"))
    });
    https && proto.next().is_none()
}

/// Walk the proxy chain from the connection back toward the sender. Once
/// an untrusted hop is found, anything farther left is its own assertion.
pub(crate) fn client_ip<B>(req: &Request<B>, env: &Env) -> IpAddr {
    let peer = peer_ip(req).unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    if !env.trusted_proxies.contains(&peer) {
        return peer;
    }
    for value in req.headers().get_all("x-forwarded-for").iter().rev() {
        let Ok(value) = value.to_str() else {
            return peer;
        };
        for hop in value.rsplit(',') {
            let Ok(ip) = hop.trim().parse::<IpAddr>() else {
                return peer;
            };
            let ip = ip.to_canonical();
            if !env.trusted_proxies.contains(&ip) {
                return ip;
            }
        }
    }
    peer
}
