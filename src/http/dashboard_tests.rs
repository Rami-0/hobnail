use super::*;
use serde_json::{json, Value};

struct Client {
    addr: std::net::SocketAddr,
    cookie: String,
    csrf: String,
}

impl Client {
    async fn owner(addr: std::net::SocketAddr) -> Self {
        let (cookie, csrf) = login(addr).await;
        Self { addr, cookie, csrf }
    }

    async fn call(&self, method: &str, path: &str, body: Value) -> Resp {
        req(
            self.addr,
            method,
            path,
            &[
                ("Origin", &format!("http://{}", self.addr)),
                ("Content-Type", "application/json"),
                ("Cookie", &self.cookie),
                ("X-RH-CSRF", &self.csrf),
            ],
            &if body.is_null() {
                String::new()
            } else {
                body.to_string()
            },
        )
        .await
    }

    async fn json(&self, method: &str, path: &str, body: Value, status: u16) -> Value {
        let response = self.call(method, path, body).await;
        assert_eq!(
            response.status, status,
            "{method} {path}: {}",
            response.body
        );
        assert_json_no_store(&response, path);
        serde_json::from_str(&response.body).expect("JSON response")
    }

    async fn guest(&self, name: &str, role: &str, scopes: Value) -> Self {
        let invite = self
            .json(
                "POST",
                "/api/v1/invites",
                json!({"role":role,"scopes":scopes}),
                201,
            )
            .await;
        let accepted = self
            .call(
                "POST",
                "/api/v1/invites/accept",
                json!({
                    "username":name, "password":PW,"token":invite["token"]
                }),
            )
            .await;
        assert_eq!(accepted.status, 201, "{}", accepted.body);
        let (cookie, csrf) = credentials_of(&accepted);
        Self {
            addr: self.addr,
            cookie,
            csrf,
        }
    }

    async fn board(&self, name: &str) -> String {
        self.json("POST", "/api/v1/boards", json!({"name":name}), 201)
            .await["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

#[tokio::test]
async fn board_save_is_atomic_and_drafts_are_private_and_pruned() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st.clone()).await;
    let owner = Client::owner(addr).await;
    let admin = owner.guest("editor", "admin", Value::Null).await;
    let id = owner.board("Empty").await;
    let path = format!("/api/v1/boards/{id}");
    let draft = format!("{path}/draft");
    let board = owner.json("GET", &path, Value::Null, 200).await;
    assert_eq!(board["document"], json!({"version":0,"sections":[]}));
    assert_eq!(board["resolved"], json!([]));
    let suggestions = owner
        .json("GET", &format!("{path}/suggestions"), Value::Null, 200)
        .await;
    assert!(suggestions.as_array().unwrap().iter().any(
        |s| s["payload"]["target"] == json!({"by":"compose","project":"shop","service":"web"})
    ));

    let doc = json!({"version":0,"sections":[{"title":"Services","position":0,"items":[{
        "position":0,"spans":{"mobile":4,"tablet":4,"desktop":6,"rows":1},
        "payload":{"kind":"container","target":{"by":"compose","project":"gone","service":"web"}}
    },{
        "position":1,"spans":{"mobile":4,"tablet":4,"desktop":6,"rows":1},
        "payload":{"kind":"container","target":{"by":"compose","project":"shop","service":"web"},"title":"Web service"}
    }]}]});
    let stored = owner.json("PUT", &draft, doc.clone(), 200).await;
    assert_eq!(stored, owner.json("GET", &draft, Value::Null, 200).await);
    assert_eq!(
        admin.json("GET", &draft, Value::Null, 200).await,
        Value::Null
    );
    admin.json("PUT", &draft, doc.clone(), 200).await;
    assert_eq!(admin.call("DELETE", &draft, Value::Null).await.status, 204);
    assert_eq!(
        admin.json("GET", &draft, Value::Null, 200).await,
        Value::Null
    );
    admin.json("PUT", &draft, doc.clone(), 200).await;

    let mut invalid = doc.clone();
    invalid["sections"][0]["items"][0]["spans"]["mobile"] = json!(5);
    let r = owner.call("PUT", &path, invalid).await;
    assert_api_error(&r, 400, "bad_request", "invalid spans");
    assert_eq!(owner.json("GET", &path, Value::Null, 200).await, board);

    let mut secret_in_draft = doc.clone();
    secret_in_draft["sections"][0]["items"][0]["payload"]["secret"] = json!("private");
    assert_api_error(
        &owner.call("PUT", &draft, secret_in_draft).await,
        400,
        "bad_request",
        "unknown draft field",
    );

    let saved = owner.json("PUT", &path, doc.clone(), 200).await;
    assert_eq!(saved["document"]["version"], 1);
    assert_eq!(saved["resolved"][0]["missing"], true);
    assert_eq!(saved["resolved"][1]["missing"], false);
    assert_eq!(saved["resolved"][1]["container"]["name"], "web");
    assert_eq!(saved["resolved"][1]["container"]["title"], "Web service");
    assert_eq!(saved["resolved"][1]["container"]["state"], "running");
    assert!(saved["document"]["sections"][0]["id"].as_str().is_some());
    assert!(saved["document"]["sections"][0]["items"][0]["id"]
        .as_str()
        .is_some());
    let r = admin.call("PUT", &path, doc).await;
    assert_eq!(r.status, 409);
    assert_json_no_store(&r, "other editing session");
    let stale: Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(stale["error"]["code"], "conflict");
    assert_eq!(stale["current_version"], 1);
    assert_eq!(owner.json("GET", &path, Value::Null, 200).await, saved);
    assert_eq!(
        owner.json("GET", &draft, Value::Null, 200).await,
        Value::Null
    );
    assert_eq!(
        admin.json("GET", &draft, Value::Null, 200).await["document"]["version"],
        0
    );

    let now = crate::auth::now_s();
    st.db
        .dashboard::<_, rusqlite::Error>(|c| {
            c.execute("UPDATE drafts SET updated = ?1", [now - 7 * 86400 - 1])?;
            Ok(())
        })
        .unwrap();
    st.db.maintain(now);
    let count = st
        .db
        .dashboard::<_, rusqlite::Error>(|c| {
            c.query_row("SELECT count(*) FROM drafts", [], |r| r.get::<_, i64>(0))
        })
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn dashboard_route_and_stream_scopes_and_settings() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let visible = owner.board("Visible").await;
    let hidden = owner.board("Hidden").await;
    let integration = json!({"name":"Service","kind":"http","base_url":"https://example.test/path","enabled":true,"secret":"hidden-token"});
    let connected = owner
        .json("POST", "/api/v1/integrations", integration.clone(), 201)
        .await;
    let visible_i = connected["id"].as_str().unwrap();
    let hidden_row = owner
        .json("POST", "/api/v1/integrations", integration, 201)
        .await;
    let hidden_i = hidden_row["id"].as_str().unwrap();
    let scopes = json!({"boards":[visible],"integrations":[visible_i]});
    let viewer = owner
        .guest("limited-viewer", "viewer", scopes.clone())
        .await;
    let admin = owner.guest("limited-admin", "admin", scopes).await;

    for (method, path) in WRITES.iter().copied().filter(|(_, path)| {
        path.starts_with("/api/v1/boards")
            || path.starts_with("/api/v1/integrations")
            || *path == "/api/v1/settings"
    }) {
        assert_api_error(
            &viewer.call(method, path, json!({})).await,
            403,
            "forbidden",
            path,
        );
    }
    for (method, path) in WRITES
        .iter()
        .copied()
        .filter(|(_, path)| path.starts_with("/api/v1/users/"))
    {
        assert_api_error(
            &admin.call(method, path, json!({})).await,
            403,
            "forbidden",
            path,
        );
    }
    for method in ["GET", "PUT", "PATCH", "DELETE"] {
        let path = format!("/api/v1/boards/{hidden}");
        assert_api_error(
            &admin.call(method, &path, json!({})).await,
            404,
            "not_found",
            &path,
        );
    }
    for (method, suffix) in [
        ("GET", "suggestions"),
        ("GET", "draft"),
        ("PUT", "draft"),
        ("DELETE", "draft"),
    ] {
        let path = format!("/api/v1/boards/{hidden}/{suffix}");
        assert_api_error(
            &admin.call(method, &path, json!({})).await,
            404,
            "not_found",
            &path,
        );
    }
    for (method, suffix) in [
        ("GET", ""),
        ("PUT", ""),
        ("DELETE", ""),
        ("PUT", "/secret"),
        ("DELETE", "/secret"),
        ("POST", "/test"),
    ] {
        let path = format!("/api/v1/integrations/{hidden_i}{suffix}");
        assert_api_error(
            &admin.call(method, &path, json!({})).await,
            404,
            "not_found",
            &path,
        );
    }

    let boards = viewer.json("GET", "/api/v1/boards", Value::Null, 200).await;
    assert_eq!(boards.as_array().unwrap().len(), 1);
    assert_eq!(boards[0]["id"], visible);
    let integrations = viewer
        .json("GET", "/api/v1/integrations", Value::Null, 200)
        .await;
    assert_eq!(
        integrations,
        json!([{
            "id":visible_i,"name":"Service","kind":"http","origin":"https://example.test","has_secret":true
        }])
    );

    let (mut stream, _) =
        Stream::open(owner.addr, "/api/v1/stream?topics=boards", &viewer.cookie).await;
    assert_eq!(stream.next().await.0, "hello");
    owner
        .json(
            "PATCH",
            &format!("/api/v1/boards/{hidden}"),
            json!({"version":0,"name":"Hidden again","position":1}),
            200,
        )
        .await;
    owner
        .json(
            "PATCH",
            &format!("/api/v1/boards/{visible}"),
            json!({"version":0,"name":"Visible again","position":0}),
            200,
        )
        .await;
    assert_eq!(
        stream.next().await,
        ("boards".into(), json!({"id":visible,"version":1}))
    );

    let settings = "/api/v1/settings";
    for id in ["nonexistent", hidden.as_str()] {
        let r = admin
            .call("PATCH", settings, json!({"default_board":id}))
            .await;
        assert_api_error(&r, 400, "bad_request", "invalid default");
        assert_eq!(
            serde_json::from_str::<Value>(&r.body).unwrap()["error"]["field"],
            "default_board"
        );
    }
    owner
        .json("PATCH", settings, json!({"default_board":hidden}), 200)
        .await;
    assert_eq!(
        viewer.json("GET", settings, Value::Null, 200).await,
        json!({"default_board":null})
    );
    let bootstrap = viewer
        .json("GET", "/api/v1/bootstrap", Value::Null, 200)
        .await;
    assert_eq!(bootstrap["default_board"], Value::Null);
    assert_eq!(
        bootstrap["boards"],
        json!([{"id":visible,"name":"Visible again","icon":null}])
    );
    assert_eq!(bootstrap["remote_icons"], false);
    assert_api_error(
        &viewer
            .call("GET", "/api/v1/icons/postgres", Value::Null)
            .await,
        404,
        "not_found",
        "icons disabled",
    );

    let users = owner.json("GET", "/api/v1/users", Value::Null, 200).await;
    let scoped = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["name"] == "limited-admin")
        .unwrap();
    assert_eq!(
        scoped["scope_targets"]["boards"],
        json!([{"id":visible,"name":"Visible again"}])
    );
    assert_eq!(
        scoped["scope_targets"]["integrations"],
        json!([{"id":visible_i,"name":"Service"}])
    );
    let r = owner
        .call(
            "PUT",
            &format!("/api/v1/users/{}/scopes", scoped["id"]),
            json!({"scopes":{"boards":["f".repeat(32)],"integrations":[]}}),
        )
        .await;
    assert_api_error(&r, 400, "bad_request", "unknown scope");
    assert_eq!(
        owner
            .call(
                "DELETE",
                &format!("/api/v1/integrations/{visible_i}"),
                Value::Null
            )
            .await
            .status,
        204
    );
    assert_eq!(
        owner
            .call("DELETE", &format!("/api/v1/boards/{visible}"), Value::Null)
            .await
            .status,
        204
    );
    let users = owner.json("GET", "/api/v1/users", Value::Null, 200).await;
    let scoped = users
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["name"] == "limited-admin")
        .unwrap();
    assert_eq!(scoped["scopes"], json!({"boards":[],"integrations":[]}));
}

async fn upstream_request(socket: &mut TcpStream) -> String {
    let mut incoming = Vec::new();
    while !incoming.ends_with(b"\r\n\r\n") {
        incoming.push(socket.read_u8().await.unwrap());
    }
    String::from_utf8(incoming).unwrap()
}

#[tokio::test]
async fn stolen_admin_session_cannot_read_or_retarget_a_secret() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let admin = owner.guest("stolen-admin", "admin", Value::Null).await;
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", upstream.local_addr().unwrap());
    let other = format!("http://{}", redirect.local_addr().unwrap());
    let secret = "sentinel-private-bearer";
    let input = json!({"kind":"http","name":"External","enabled":true,"base_url":format!("{origin}/base"),"secret":secret});
    let created = owner.json("POST", "/api/v1/integrations", input, 201).await;
    let id = created["id"].as_str().unwrap();
    let path = format!("/api/v1/integrations/{id}");
    for value in [
        created,
        admin.json("GET", &path, Value::Null, 200).await,
        admin
            .json("GET", "/api/v1/integrations", Value::Null, 200)
            .await,
        admin
            .json("GET", "/api/v1/bootstrap", Value::Null, 200)
            .await,
    ] {
        let serialized = value.to_string();
        assert!(!serialized.contains(secret));
        assert!(!serialized.contains("sentinel"));
    }
    let export = admin
        .call("GET", "/api/settings/history/export", Value::Null)
        .await;
    assert_eq!(export.status, 200);
    assert!(!export.body.contains(secret));
    let updated = admin.json("PUT",&path,json!({
        "kind":"http","name":"External","enabled":true,"base_url":format!("{origin}/changed")
    }),200).await;
    assert_eq!(updated["admin"]["secret_origin"], origin);
    assert_eq!(updated["has_secret"], true);

    let attempted = admin
        .call(
            "PUT",
            &path,
            json!({
                "kind":"http","name":"External","enabled":true,"base_url":other
            }),
        )
        .await;
    assert_api_error(&attempted, 400, "bad_request", "retarget");
    assert_eq!(
        serde_json::from_str::<Value>(&attempted.body).unwrap()["error"]["field"],
        "secret"
    );
    assert!(!attempted.body.contains(secret));

    let target = other.clone();
    let task = tokio::spawn(async move {
        let (mut socket, _) = upstream.accept().await.unwrap();
        let request = upstream_request(&mut socket).await;
        assert!(request.starts_with("GET /changed HTTP/1.1"));
        assert!(request.contains(&format!("authorization: Bearer {secret}\r\n")));
        let body = format!("upstream-private-body {secret}");
        socket.write_all(format!(
            "HTTP/1.1 302 Found\r\nLocation: {target}/capture\r\nX-Upstream-Secret: {secret}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ).as_bytes()).await.unwrap();
    });
    let verdict = admin
        .json("POST", &format!("{path}/test"), Value::Null, 200)
        .await;
    task.await.unwrap();
    assert_eq!(verdict["ok"], false);
    assert_eq!(verdict["class"], "http_status");
    assert_eq!(verdict.as_object().unwrap().len(), 4);
    assert!(!verdict.to_string().contains("upstream-private"));
    assert!(!verdict.to_string().contains(secret));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), redirect.accept())
            .await
            .is_err()
    );

    let updated = admin.json("PUT",&path,json!({
        "kind":"http","name":"External","enabled":true,"base_url":other,"secret":"replacement-token"
    }),200).await;
    assert_eq!(updated["admin"]["secret_origin"], other);
    assert!(!updated.to_string().contains("replacement-token"));
    let stale_secret = admin
        .call(
            "PUT",
            &format!("{path}/secret"),
            json!({
                "base_url":origin,"secret":"stale-token"
            }),
        )
        .await;
    assert_api_error(&stale_secret, 400, "bad_request", "stale credential form");
    assert!(!stale_secret.body.contains("stale-token"));
    let cleared = admin
        .json("DELETE", &format!("{path}/secret"), Value::Null, 200)
        .await;
    assert_eq!(cleared["has_secret"], false);
    assert_eq!(cleared["admin"]["secret_origin"], Value::Null);
    let set = admin
        .json(
            "PUT",
            &format!("{path}/secret"),
            json!({"secret":"write-only","base_url":other}),
            200,
        )
        .await;
    assert_eq!(set["has_secret"], true);
    assert_eq!(set["admin"]["secret_origin"], other);
    assert!(!set.to_string().contains("write-only"));
}

#[tokio::test]
async fn integration_test_reports_public_root_failure() {
    use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
    // Self-signed loopback test fixture. The private key is disposable.
    let server = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![CertificateDer::from(
            include_bytes!("testdata/integration-self-signed.der").to_vec(),
        )],
        PrivatePkcs8KeyDer::from(include_bytes!("testdata/integration-test-key.der").to_vec())
            .into(),
    )
    .unwrap();
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = upstream.local_addr().unwrap();
    let tls = tokio::spawn(async move {
        let (socket, _) = upstream.accept().await.unwrap();
        assert!(tokio_rustls::TlsAcceptor::from(Arc::new(server))
            .accept(socket)
            .await
            .is_err());
    });
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let created = owner.json("POST","/api/v1/integrations",json!({
        "name":"Private CA","kind":"http","enabled":true,"base_url":format!("https://{upstream_addr}")
    }),201).await;
    let r = owner
        .json(
            "POST",
            &format!(
                "/api/v1/integrations/{}/test",
                created["id"].as_str().unwrap()
            ),
            Value::Null,
            200,
        )
        .await;
    assert_eq!(r["ok"], false);
    assert_eq!(r["class"], "tls");
    assert!(r["message"]
        .as_str()
        .unwrap()
        .contains("public certificate authorities only"));
    tls.await.unwrap();
}

#[tokio::test]
async fn dashboard_icons_use_the_authenticated_existing_cache() {
    let st = crate::ui::testutil::state_with(Vec::new(), "[icons]\nremote=true\n");
    crate::auth::set_password_for_tests(&st.db, PW);
    let dir = st.env.data_dir.join("icons/di");
    std::fs::create_dir_all(&dir).unwrap();
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><title>cached icon</title></svg>"#;
    std::fs::write(dir.join("postgres.svg"), svg).unwrap();
    let owner = Client::owner(start(st).await).await;
    let bootstrap = owner
        .json("GET", "/api/v1/bootstrap", Value::Null, 200)
        .await;
    assert_eq!(bootstrap["remote_icons"], true);
    let r = owner
        .call("GET", "/api/v1/icons/postgres", Value::Null)
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.body, svg);
    assert_eq!(
        header(&r.head, "content-type").as_deref(),
        Some("image/svg+xml")
    );
    assert_eq!(
        header(&r.head, "cache-control").as_deref(),
        Some("no-store")
    );
    assert!(crate::http::assets::CSP.contains("img-src 'self' data:"));
}

#[tokio::test]
async fn connection_verdict_enforces_outbound_policy() {
    let st =
        crate::ui::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['only.example.test']\n");
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let created = owner
        .json(
            "POST",
            "/api/v1/integrations",
            json!({
                "name":"Blocked","kind":"http","enabled":true,"base_url":"http://127.0.0.1"
            }),
            201,
        )
        .await;
    let path = format!(
        "/api/v1/integrations/{}/test",
        created["id"].as_str().unwrap()
    );
    let r = owner.json("POST", &path, Value::Null, 200).await;
    assert_eq!(r["class"], "blocked");
    assert_eq!(r["ok"], false);
}

#[tokio::test]
async fn outbound_client_checks_origin_before_connecting() {
    use crate::http::client::{self, BoundCredential};

    assert_eq!(
        client::origin("https://Example.test:443/path").as_deref(),
        Some("https://example.test")
    );
    assert_eq!(
        client::origin("http://[::1]:8080/path").as_deref(),
        Some("http://[::1]:8080")
    );
    for url in [
        "https://user@example.test",
        "https://example.test?q=token",
        "http://example.test:0",
    ] {
        assert!(client::origin(url).is_none());
    }

    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", destination.local_addr().unwrap());
    let result = client::send_bound(
        url,
        Some(BoundCredential {
            origin: "http://127.0.0.1:1".into(),
            token: "private-token".into(),
        }),
        &Default::default(),
    )
    .await;
    assert_eq!(result.unwrap_err(), "blocked credential destination");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), destination.accept())
            .await
            .is_err()
    );
}
