use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::timeout,
};

use super::*;

fn policy() -> HttpPolicy {
    HttpPolicy {
        allow_hosts: vec!["127.0.0.1".into()],
    }
}

fn credentials(origin: &str) -> Credentials {
    [
        ("user", "some-user"),
        ("password", "p+ ss&/"),
        ("token", "opaque-token"),
    ]
    .into_iter()
    .map(|(field, token)| {
        (
            field.into(),
            BoundCredential {
                origin: origin.into(),
                token: token.into(),
            },
        )
    })
    .collect()
}

pub(in crate::providers) async fn receive(listener: &TcpListener) -> (TcpStream, String) {
    let (mut socket, _) = timeout(Duration::from_secs(6), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut raw = Vec::new();
    while !raw.ends_with(b"\r\n\r\n") {
        raw.push(socket.read_u8().await.unwrap());
    }
    let head = String::from_utf8(raw).unwrap();
    let length: usize = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .map_or(0, |length| length.parse().unwrap());
    let mut body = vec![0; length];
    socket.read_exact(&mut body).await.unwrap();
    (socket, head + std::str::from_utf8(&body).unwrap())
}

pub(in crate::providers) async fn respond(
    socket: &mut TcpStream,
    status: u16,
    headers: &str,
    body: &str,
) {
    socket
        .write_all(
            format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\n{headers}\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
}

#[test]
fn rejects_authority_and_prefix_escape_variants() {
    let base = "https://fixture.invalid/reverse/proxy/";
    assert_eq!(join(base, "").unwrap(), base);
    for path in ["api/v1", "/api/v1", "/admin/api.php"] {
        assert_eq!(
            join(base, path).unwrap(),
            format!("{base}{}", path.trim_start_matches('/'))
        );
    }
    for path in [
        "https://other.invalid",
        "//other.invalid",
        "///other.invalid",
        "other.invalid:443",
        "https:%2f%2fother.invalid",
        "/%2f%2Fother.invalid",
        "/%252f%252fother.invalid",
        "/@other.invalid",
        "/%40other.invalid",
        "/\\other.invalid",
        "/%5Cother.invalid",
        "\\other.invalid",
        "/%255cother.invalid",
        "/foo//bar",
        "/a/../b",
        "/../b",
        "/./b",
        "/%2e%2E/b",
        "/%252e%252e/b",
        "/foo?bar=1",
        "/foo#bar",
        "/\t/x",
    ] {
        assert!(
            matches!(join(base, path), Err(error) if matches!(error.class, Class::Blocked)),
            "{path}"
        );
    }
}

#[tokio::test]
async fn shared_fetch_redacts_all_emitted_auth_representations() {
    use crate::{
        providers::{self, Job, Widget},
        widgets::StructuredBody,
    };
    use serde_json::json;

    fn echo<'a>(conn: &'a Conn<'_>, _: &'a Values) -> Job<'a, StructuredBody> {
        Box::pin(async {
            let session = conn.session("fake-sid+/")?;
            let response = conn
                .request(Step {
                    auth: &[
                        Auth::Bearer(Input::Credential("token")),
                        Auth::Basic(Input::Credential("user"), Input::Credential("password")),
                        Auth::Header("x-api-key", Input::Credential("header")),
                        Auth::Header("cookie", Input::Session(&session)),
                    ],
                    query: &[("auth", Input::Credential("query"))],
                    ..Step::get("")
                })
                .await?;
            serde_json::from_slice(&response.body).map_err(|_| Class::Protocol.into())
        })
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mut credentials = credentials(&origin);
    credentials.get_mut("token").unwrap().token = r#"token~*+"with\quotes"#.into();
    for (field, value) in [("header", "header-secret"), ("query", "query +/secret")] {
        credentials.insert(
            field.into(),
            BoundCredential {
                origin: origin.clone(),
                token: value.into(),
            },
        );
    }
    let encoded = base64(r#"token~*+"with\quotes"#);
    let echoes = [
        r#"token~*+"with\quotes"#,
        r#"Bearer token~*+"with\quotes"#,
        encoded.as_str(),
        r#"token~*+\"with\\quotes"#,
        "token~%2A%2B%22with%5Cquotes",
        "token%7e*%2B%22with%5cquotes",
        r#"token\u007e\u002a\u002b\u0022with\u005cquotes"#,
        "token&#126;&#x2a;+&quot;with&#92;quotes",
        "c29tZS11c2VyOnArIHNzJi8",
        "Basic c29tZS11c2VyOnArIHNzJi8=",
        "c29tZS11c2VyOnArIHNzJi8=",
        "some-user:p+ ss&/",
        "some-user",
        "p+ ss&/",
        "header-secret",
        "query%20%2B%2Fsecret",
        "query+%2b%2fsecret",
        "fake-sid+/",
        "fake-sid%2B%2F",
        "untouched",
    ];
    let count = echoes.len();
    let items: Vec<_> = echoes
        .iter()
        .map(|value| {
            json!({
                "title": value, "secondary": value, "link": format!("https://example.test/{value}")
            })
        })
        .collect();
    let fake = tokio::spawn(async move {
        let (mut socket, _) = receive(&listener).await;
        respond(
            &mut socket,
            200,
            "",
            &json!({"kind":"item_list","items":items}).to_string(),
        )
        .await;
    });
    let connections = Connections::default();
    let target = Target::new(format!("{origin}/"), credentials, false);
    let policy = policy();
    let permit = std::sync::Arc::new(Semaphore::new(1))
        .try_acquire_owned()
        .unwrap();
    let conn = Conn::new(&target, &policy, &connections, permit).unwrap();
    let widget = Widget {
        id: "echo",
        name: "Echo",
        description: "",
        recommended: false,
        fields: &[],
        fetch: echo,
        validate: |_| Ok(()),
    };
    let StructuredBody::ItemList { items } = providers::fetch(&conn, &widget, &Values::new())
        .await
        .unwrap()
    else {
        panic!("unexpected output shape")
    };
    fake.await.unwrap();
    assert_eq!(items.len(), count);
    for (index, item) in items.iter().enumerate() {
        let expected = if index == count - 1 {
            "untouched"
        } else {
            "<redacted>"
        };
        assert!(
            item.title == expected && item.secondary.as_deref() == Some(expected),
            "redaction at item {index}"
        );
        assert_eq!(item.link.is_some(), index == count - 1);
    }
}

#[tokio::test]
async fn steps_encode_credentials_and_only_return_requested_headers() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", upstream.local_addr().unwrap());
    let foreign = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other_origin = format!("http://{}", foreign.local_addr().unwrap());
    let fake = tokio::spawn(async move {
        let (mut socket, wire) = receive(&upstream).await;
        assert!(wire.starts_with("POST /prefix/login HTTP/1.1"));
        assert!(wire.contains("authorization: Basic c29tZS11c2VyOnArIHNzJi8=\r\n"));
        assert!(wire.ends_with("username=some-user&password=p%2B%20ss%26%2F"));
        respond(
            &mut socket,
            200,
            "Set-Cookie: sid=fixture\r\nSet-Cookie: second=fixture\r\nX-Hidden: private\r\n",
            "",
        )
        .await;
        let (mut socket, wire) = receive(&upstream).await;
        assert!(wire.starts_with("PUT /prefix/stats?auth=opaque-token&name%26=value%3D HTTP/1.1"));
        assert!(wire.contains("x-api-key: opaque-token\r\n"));
        assert!(wire.contains("authorization: PVEAPIToken=test@realm!id=opaque-token\r\n"));
        assert!(wire.contains("cookie: sid=fixture\r\n"));
        respond(
            &mut socket,
            302,
            &format!("Location: {other_origin}/receive\r\n"),
            "",
        )
        .await;
    });
    let connections = Connections::default();
    let target = Target::new(format!("{origin}/prefix"), credentials(&origin), false);
    let policy = policy();
    let permit = std::sync::Arc::new(Semaphore::new(1))
        .try_acquire_owned()
        .unwrap();
    let conn = Conn::new(&target, &policy, &connections, permit).unwrap();
    conn.run(async {
        let reply = conn
            .request(Step {
                verb: Verb::Post,
                auth: &[Auth::Basic(
                    Input::Credential("user"),
                    Input::Credential("password"),
                )],
                body: Some(Body::Form(&[
                    ("username", Input::Credential("user")),
                    ("password", Input::Credential("password")),
                ])),
                response_headers: &["set-cookie"],
                ..Step::get("/login")
            })
            .await?;
        assert_eq!(reply.headers.len(), 2);
        assert!(!reply.headers.contains_key("x-hidden"));
        let session = conn.session_header(&reply, "set-cookie")?;
        let response = conn
            .request(Step {
                verb: Verb::Put,
                auth: &[
                    Auth::Header("x-api-key", Input::Credential("token")),
                    Auth::Authorization("PVEAPIToken=test@realm!id=", Input::Credential("token")),
                    Auth::Header("cookie", Input::Session(&session)),
                ],
                query: &[
                    ("auth", Input::Credential("token")),
                    ("name&", Input::Text("value=")),
                ],
                ..Step::get("/stats")
            })
            .await?;
        assert_eq!(response.status, 302);
        assert!(response.headers.is_empty());
        let other = Target::new(
            format!("http://{}", foreign.local_addr().unwrap()),
            credentials(&origin),
            false,
        );
        let other = Conn::new(
            &other,
            &policy,
            &connections,
            std::sync::Arc::new(Semaphore::new(1))
                .try_acquire_owned()
                .unwrap(),
        )?;
        for auth in [
            Auth::Header("x-api-key", Input::Credential("token")),
            Auth::Header("cookie", Input::Session(&session)),
        ] {
            let denied = other
                .request(Step {
                    auth: &[auth],
                    ..Step::get("/wrong")
                })
                .await;
            assert!(matches!(denied, Err(error) if matches!(error.class, Class::Blocked)));
        }
        Ok(())
    })
    .await
    .unwrap();
    fake.await.unwrap();
    assert!(timeout(Duration::from_millis(50), foreign.accept())
        .await
        .is_err());
}

#[tokio::test]
async fn a_multistep_operation_has_one_deadline_and_keeps_one_permit() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = Target::new(
        format!("http://{}", upstream.local_addr().unwrap()),
        Credentials::new(),
        false,
    );
    let semaphore = std::sync::Arc::new(Semaphore::new(1));
    let permit = semaphore.clone().try_acquire_owned().unwrap();
    let connections = Connections::default();
    let policy = policy();
    let conn = Conn::new(&target, &policy, &connections, permit).unwrap();
    let fake = tokio::spawn(async move {
        let (mut socket, _) = receive(&upstream).await;
        tokio::time::sleep(Duration::from_millis(2600)).await;
        respond(&mut socket, 200, "", "").await;
        let (mut socket, _) = receive(&upstream).await;
        assert_eq!(
            timeout(Duration::from_secs(4), socket.read(&mut [0]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    let start = tokio::time::Instant::now();
    let result = conn
        .run(async {
            conn.request(Step::get("/first")).await?;
            assert!(semaphore.clone().try_acquire_owned().is_err());
            conn.request(Step::get("/second")).await
        })
        .await;
    assert!(matches!(result, Err(error) if matches!(error.class, Class::Timeout)));
    assert!(start.elapsed() < Duration::from_secs(6));
    fake.await.unwrap();
    drop(conn);
    assert_eq!(semaphore.available_permits(), 1);
}

#[tokio::test]
async fn session_cache_is_bounded_revision_bound_and_can_retry_a_409_with_its_header() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", upstream.local_addr().unwrap());
    let connections = Connections::default();
    let policy = policy();
    let semaphore = std::sync::Arc::new(Semaphore::new(2));
    let make = |id: &str| {
        let mut target = Target::new(origin.clone(), Credentials::new(), false);
        connections.bind(&mut target, Some(id), true);
        target
    };
    let first = make("initial");
    let conn = Conn::new(
        &first,
        &policy,
        &connections,
        semaphore.clone().try_acquire_owned().unwrap(),
    )
    .unwrap();
    let fake = tokio::spawn(async move {
        let (mut socket, wire) = receive(&upstream).await;
        assert!(wire.contains("x-session: old\r\n"));
        respond(&mut socket, 409, "X-Session: new\r\n", "").await;
        let (mut socket, wire) = receive(&upstream).await;
        assert!(wire.contains("x-session: new\r\n"));
        respond(&mut socket, 200, "", "").await;
    });
    conn.run(conn.retry_session(
        || async { conn.session("old").map(Some) },
        |session| {
            let conn = &conn;
            async move {
                let response = conn
                    .request(Step {
                        auth: &[Auth::Header(
                            "x-session",
                            Input::Session(session.as_ref().unwrap()),
                        )],
                        response_headers: &["x-session"],
                        ..Step::get("/rpc")
                    })
                    .await?;
                if response.status == 409 {
                    Err(Fault::session_conflict(
                        conn.session_header(&response, "x-session")?,
                    ))
                } else {
                    success(response)
                }
            }
        },
    ))
    .await
    .unwrap();
    fake.await.unwrap();
    assert_eq!(
        conn.get_or_create_session(async { panic!("cached session") })
            .await
            .unwrap(),
        Some(conn.session("new").unwrap())
    );
    assert!(conn.session(&"s".repeat(MAX_SESSION + 1)).is_err());
    for i in 0..MAX_SESSIONS {
        let target = make(&format!("test-{i}"));
        let other = Conn::new(
            &target,
            &policy,
            &connections,
            semaphore.clone().try_acquire_owned().unwrap(),
        )
        .unwrap();
        other
            .get_or_create_session(async { other.session(&"s".repeat(MAX_SESSION)).map(Some) })
            .await
            .unwrap();
    }
    {
        let cache = connections.sessions.lock().unwrap();
        assert_eq!(cache.len(), MAX_SESSIONS);
        assert_eq!(
            cache
                .iter()
                .map(|(_, session)| session.value.len())
                .sum::<usize>(),
            MAX_SESSIONS * MAX_SESSION
        );
        assert!(!cache.iter().any(|(key, _)| key.integration == "initial"));
    }
    connections.invalidate(Some("test-0"));
    assert!(!connections
        .sessions
        .lock()
        .unwrap()
        .iter()
        .any(|(key, _)| key.integration == "test-0"));
    connections.invalidate(Some("initial"));
    assert!(first.revision.check().is_err());
    let next = make("initial");
    assert!(next.session_key != first.session_key);
    assert_eq!(next.session_key.unwrap().origin, origin);
    connections.invalidate(None);
    assert!(connections.sessions.lock().unwrap().is_empty());
    assert!(next.revision.check().is_err());
}

#[tokio::test]
async fn debug_omits_request_response_credential_and_session_material() {
    let material = "private-fixture-value";
    let target = Target::new("http://127.0.0.1".into(), Credentials::new(), false);
    let connections = Connections::default();
    let policy = policy();
    let conn = Conn::new(
        &target,
        &policy,
        &connections,
        std::sync::Arc::new(Semaphore::new(1))
            .try_acquire_owned()
            .unwrap(),
    )
    .unwrap();
    let session = conn.session(material).unwrap();
    let response = Response {
        status: 200,
        body: material.into(),
        headers: [("set-cookie".parse().unwrap(), material.parse().unwrap())]
            .into_iter()
            .collect(),
    };
    let request = client::Request::post_json(
        format!("http://127.0.0.1/?key={material}"),
        material.as_bytes().into(),
    )
    .header("x-api-key", material);
    let credential = BoundCredential {
        origin: "http://127.0.0.1".into(),
        token: material.into(),
    };
    let debug = format!(
        "{request:?} {response:?} {credential:?} {session:?} {:?}",
        Step::get(material)
    );
    assert!(!debug.contains(material));
}

#[test]
fn rejects_ambiguous_paths_in_saved_targets() {
    for path in [
        "/prefix/./leaf",
        "/prefix/../leaf",
        "/prefix/%2e/leaf",
        "/prefix/.%2E/leaf",
        "/prefix/%2Fleaf",
        "/prefix/%5cleaf",
        "/prefix/%252e%252e/leaf",
        "/prefix/%252fleaf",
        "/prefix/%",
        "/prefix/%00",
    ] {
        assert!(
            client::origin(&format!("https://fixture.test{path}")).is_none(),
            "{path}"
        );
    }
    for path in [
        "/prefix/multi.part",
        "/prefix/encoded%20space",
        "/prefix/a%2Eb",
    ] {
        assert!(
            client::origin(&format!("https://fixture.test{path}")).is_some(),
            "{path}"
        );
    }
}

#[tokio::test]
async fn a_cold_transmission_session_starts_with_the_real_rpc() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let connections = Connections::default();
    let policy = policy();
    let mut target = Target::new(
        format!("http://{}", upstream.local_addr().unwrap()),
        Credentials::new(),
        false,
    );
    connections.bind(&mut target, Some("transmission"), true);
    let permit = std::sync::Arc::new(Semaphore::new(1))
        .try_acquire_owned()
        .unwrap();
    let conn = Conn::new(&target, &policy, &connections, permit).unwrap();
    const HEADER: &str = "x-transmission-session-id";
    let fake = tokio::spawn(async move {
        for (session, status, headers) in [
            (false, 200, ""),
            (false, 409, "X-Transmission-Session-Id: challenge\r\n"),
            (true, 200, ""),
            (true, 200, ""),
            (true, 409, "X-Transmission-Session-Id: replacement\r\n"),
            (true, 409, "X-Transmission-Session-Id: third\r\n"),
        ] {
            let (mut socket, request) = receive(&upstream).await;
            assert!(request.starts_with("POST /transmission/rpc HTTP/1.1"));
            assert_eq!(request.contains(HEADER), session);
            assert!(request.ends_with(r#"{"method":"session-get"}"#));
            respond(&mut socket, status, headers, "{}").await;
        }
    });
    let request = |session: Option<Session>| {
        let conn = &conn;
        async move {
            let headers = session
                .as_ref()
                .map(|session| [Auth::Header(HEADER, Input::Session(session))]);
            let response = conn
                .request(Step {
                    verb: Verb::Post,
                    auth: headers.as_ref().map_or(&[], |headers| headers.as_slice()),
                    body: Some(Body::Json(
                        serde_json::from_str(r#"{"method":"session-get"}"#).unwrap(),
                        &[],
                    )),
                    response_headers: &[HEADER],
                    ..Step::get("/transmission/rpc")
                })
                .await?;
            if response.status == 409 {
                Err(Fault::session_conflict(
                    conn.session_header(&response, HEADER)?,
                ))
            } else {
                success(response)
            }
        }
    };
    for _ in 0..3 {
        assert!(conn
            .run(conn.retry_session(|| async { Ok(None) }, request))
            .await
            .is_ok());
    }
    let error = conn
        .run(conn.retry_session(|| async { Ok(None) }, request))
        .await
        .unwrap_err();
    assert!(matches!(error.class, Class::Conflict));
    assert!(connections.sessions.lock().unwrap().is_empty());
    fake.await.unwrap();
}
