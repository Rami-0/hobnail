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
        for hop in value.as_bytes().rsplit(|&byte| byte == b',') {
            let Ok(ip) = std::str::from_utf8(hop)
                .unwrap_or_default()
                .trim_matches([' ', '\t'])
                .parse::<IpAddr>()
            else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderValue;

    #[test]
    fn forwarded_for_validates_only_hops_reached_from_the_trusted_side() {
        let state = crate::testutil::state_with(vec![], "");
        let mut req = Request::new(());
        req.extensions_mut()
            .insert(Peer("127.0.0.1:1234".parse().unwrap()));
        let proxy = "127.0.0.1".parse::<IpAddr>().unwrap();
        let client = "198.51.100.2".parse::<IpAddr>().unwrap();
        let mut env = state.env.clone();
        env.trusted_proxies = vec![proxy, "192.0.2.1".parse().unwrap()];

        for chain in [
            &[&b"\xff, 198.51.100.2, 192.0.2.1"[..]][..],
            &[&b"\xff"[..], b"198.51.100.2", b"::ffff:192.0.2.1"][..],
        ] {
            req.headers_mut().clear();
            for value in chain {
                req.headers_mut()
                    .append("x-forwarded-for", HeaderValue::from_bytes(value).unwrap());
            }
            assert_eq!(client_ip(&req, &env), client);
        }

        req.headers_mut().append(
            "x-forwarded-for",
            HeaderValue::from_bytes(b"\xff, 192.0.2.1").unwrap(),
        );
        assert_eq!(client_ip(&req, &env), proxy);
    }
}
