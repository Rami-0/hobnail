use super::*;
use crate::{
    config::HttpPolicy,
    http::client::BoundCredential,
    providers::connection::tests::{receive, respond},
};
use std::sync::Arc;
use tokio::{net::TcpListener, sync::Semaphore};

pub(super) struct Exchange {
    pub request: &'static str,
    pub status: u16,
    pub body: &'static str,
}

pub(super) async fn exercise(
    provider: &Provider,
    secrets: &[(&str, &str)],
    options: Values,
    exchanges: &[Exchange],
    inspect: impl Fn(usize, &str) -> &'static str,
    verify: impl AsyncFnOnce(&Conn<'_>),
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let credentials = secrets
        .iter()
        .map(|(name, token)| {
            (
                (*name).into(),
                BoundCredential {
                    origin: origin.clone(),
                    token: (*token).into(),
                },
            )
        })
        .collect();
    let mut target = Target::new(format!("{origin}/prefix"), credentials, false);
    target.options = options;
    let connections = Connections::default();
    connections.bind(&mut target, Some(provider.id), true);
    let policy = HttpPolicy {
        allow_hosts: vec!["127.0.0.1".into()],
    };
    let permit = Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap();
    let conn = Conn::new(&target, &policy, &connections, permit).unwrap();
    let server = async {
        for (index, exchange) in exchanges.iter().enumerate() {
            let (mut socket, wire) = receive(&listener).await;
            let request_line = wire.lines().next().unwrap();
            if let Some(prefix) = exchange.request.strip_suffix('*') {
                assert!(request_line.starts_with(prefix), "wrong request path");
            } else {
                assert!(request_line == exchange.request, "wrong request line");
            }
            let headers = inspect(index, &wire);
            respond(&mut socket, exchange.status, headers, exchange.body).await;
        }
    };
    tokio::join!(server, verify(&conn));
}

pub(super) fn assert_header(wire: &str, header: &str, credential: &str) {
    let expected = format!("\r\n{header}\r\n");
    assert!(wire.contains(&expected), "missing authentication header");
    assert!(!wire.replacen(&expected, "", 1).contains(credential));
}

pub(super) async fn assert_test_fault(conn: &Conn<'_>, provider: &Provider, class: Class) {
    let fault = conn.run((provider.test)(conn)).await.unwrap_err();
    assert_eq!(fault.class.message(), class.message());
}
