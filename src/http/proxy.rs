//! Forwarding headers are meaningful only on a connection from a trusted peer.

use std::net::IpAddr;

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
