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
    assert_eq!(
        board["document"],
        json!({"version":0,"name":"Empty","appearance":{"background":"none","density":"comfortable","surface":"solid"},"sections":[]})
    );
    assert_eq!(board["resolved"], json!([]));
    let suggestions = owner
        .json("GET", &format!("{path}/suggestions"), Value::Null, 200)
        .await;
    assert!(suggestions.as_array().unwrap().iter().any(
        |s| s["payload"]["target"] == json!({"by":"compose","project":"shop","service":"web"})
    ));

    let doc = json!({"version":0,"sections":[{"title":"Services","position":0,"items":[{
        "x":0,"y":0,"w":6,"h":2,
        "payload":{"kind":"container","target":{"by":"compose","project":"gone","service":"web"}}
    },{
        "x":6,"y":0,"w":6,"h":2,
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
    invalid["sections"][0]["items"][0]["x"] = json!(3);
    let r = owner.call("PUT", &path, invalid).await;
    assert_api_error(&r, 400, "bad_request", "overlapping items");
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

    // One Save carries name, icon, appearance, collapse and layout: one revision.
    let mut styled = saved["document"].clone();
    styled["name"] = json!("Renamed");
    styled["icon"] = json!("home");
    styled["appearance"] =
        json!({"background":"aurora","density":"compact","surface":"glass","accent":"teal"});
    styled["sections"][0]["collapsed"] = json!(true);
    styled["sections"][0]["items"][1]["y"] = json!(7);
    styled["sections"][0]["items"][1]["look"] =
        json!({"accent":"rose","hide_status":true,"hide_metrics":false});
    let restyled = owner.json("PUT", &path, styled, 200).await;
    assert_eq!(restyled["board"]["version"], 2);
    assert_eq!(restyled["board"]["name"], "Renamed");
    assert_eq!(restyled["board"]["icon"], "home");
    assert_eq!(restyled["document"]["appearance"]["surface"], "glass");
    assert_eq!(restyled["document"]["sections"][0]["collapsed"], true);
    // Compacted back to the top row, look kept.
    let moved = &restyled["document"]["sections"][0]["items"][1];
    assert_eq!(
        (moved["x"].clone(), moved["y"].clone()),
        (json!(6), json!(0))
    );
    assert_eq!(moved["look"]["accent"], "rose");
    let mut unknown = restyled["document"].clone();
    unknown["appearance"]["background"] = json!("url(https://evil.test/x.png)");
    assert_eq!(owner.call("PUT", &path, unknown).await.status, 400);

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
        let (status, code) = if method == "GET" {
            (404, "not_found")
        } else {
            (403, "forbidden")
        };
        let path = format!("/api/v1/boards/{hidden}");
        assert_api_error(
            &admin.call(method, &path, json!({})).await,
            status,
            code,
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
            403,
            "forbidden",
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
        let (status, code) = if method == "GET" {
            (404, "not_found")
        } else {
            (403, "forbidden")
        };
        assert_api_error(
            &admin.call(method, &path, json!({})).await,
            status,
            code,
            &path,
        );
    }

    let integration_path = format!("/api/v1/integrations/{visible_i}");
    assert_api_error(
        &admin.call("DELETE", &integration_path, Value::Null).await,
        403,
        "forbidden",
        &integration_path,
    );
    assert_eq!(
        admin.json("GET", &integration_path, Value::Null, 200).await["admin"]["base_url"],
        "https://example.test/path"
    );
    for suffix in ["suggestions", "draft"] {
        let path = format!("/api/v1/boards/{visible}/{suffix}");
        assert_api_error(
            &viewer.call("GET", &path, Value::Null).await,
            403,
            "forbidden",
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
            "id":visible_i,"name":"Service","kind":"http","origin":"https://example.test","has_secret":true,
            "enabled":true,"check":null
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
    {
        let id = "nonexistent";
        let r = owner
            .call("PATCH", settings, json!({"default_board":id}))
            .await;
        assert_api_error(&r, 400, "bad_request", "invalid default");
        assert_eq!(
            serde_json::from_str::<Value>(&r.body).unwrap()["error"]["field"],
            "default_board"
        );
    }
    assert_api_error(
        &admin
            .call("PATCH", settings, json!({"default_board":visible}))
            .await,
        403,
        "forbidden",
        settings,
    );
    owner
        .json("PATCH", settings, json!({"default_board":hidden}), 200)
        .await;
    assert_eq!(
        viewer.json("GET", settings, Value::Null, 200).await,
        json!({"default_board":null,"default_theme":null,"provider_internet":false})
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
            .call("GET", "/api/v1/icons/homepage", Value::Null)
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
async fn scoped_board_edits_cannot_introduce_targets_or_reuse_foreign_ids() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let board = owner.board("Granted").await;
    let foreign = owner.board("Foreign").await;
    let integration = owner.json("POST", "/api/v1/integrations", json!({
        "name":"Private connection","kind":"http","enabled":true,"base_url":"https://example.test/private"
    }), 201).await;
    let item = |payload| {
        json!({
            "x":0, "y":0, "w":6, "h":2, "payload":payload
        })
    };
    let integration = item(json!({"kind":"integration","integration_id":integration["id"]}));
    let mut on_board = item(
        json!({"kind":"container","target":{"by":"compose","project":"shop","service":"web"}}),
    );
    on_board["x"] = json!(6);
    let foreign_container =
        item(json!({"kind":"container","target":{"by":"compose","project":"shop","service":"db"}}));
    let doc = |items: Vec<Value>| json!({"version":0,"sections":[{"title":"Services","position":0,"items":items}]});
    let path = format!("/api/v1/boards/{board}");
    let draft = format!("{path}/draft");
    let saved = owner
        .json("PUT", &path, doc(vec![integration.clone(), on_board]), 200)
        .await;
    let foreign = owner
        .json(
            "PUT",
            &format!("/api/v1/boards/{foreign}"),
            doc(vec![foreign_container.clone()]),
            200,
        )
        .await;
    let admin = owner
        .guest("scoped-board-editor", "admin", json!({"boards":[board]}))
        .await;

    let visible = admin.json("GET", &path, Value::Null, 200).await;
    assert_eq!(visible["resolved"][0]["missing"], true);
    assert_eq!(visible["resolved"][0]["integration"], Value::Null);
    assert_eq!(visible["resolved"][1]["container"]["name"], "web");
    let unchanged = saved["document"].clone();
    let cross_board_section = &foreign["document"]["sections"][0];
    let mut bad_section = unchanged.clone();
    bad_section["sections"][0]["id"] = cross_board_section["id"].clone();
    let mut bad_item = unchanged.clone();
    bad_item["sections"][0]["items"][0]["id"] = cross_board_section["items"][0]["id"].clone();
    for route in [&draft, &path] {
        for payload in [&integration, &foreign_container] {
            let mut adding = unchanged.clone();
            adding["sections"][0]["items"]
                .as_array_mut()
                .unwrap()
                .push(payload.clone());
            assert_api_error(
                &admin.call("PUT", route, adding).await,
                400,
                "bad_request",
                route,
            );
        }
        for invalid in [&bad_section, &bad_item] {
            assert_api_error(
                &admin.call("PUT", route, invalid.clone()).await,
                400,
                "bad_request",
                route,
            );
        }
        admin.json("PUT", route, unchanged.clone(), 200).await;
    }
}

#[tokio::test]
async fn scoped_widget_subscriptions_only_count_when_a_widget_is_visible() {
    let st = state();
    std::fs::write(st.config.path(), "[[widgets]]\nid = \"board-only\"\ntype = \"custom-api\"\nurl = \"http://127.0.0.1:1/fixture\"\n").unwrap();
    st.config.reload();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st.clone()).await).await;
    let board = owner.board("Widgets").await;
    let viewer = owner
        .guest("widget-viewer", "viewer", json!({"boards": [board]}))
        .await;
    let (mut stream, _) = Stream::open(
        owner.addr,
        "/api/v1/stream?topics=widget,boards",
        &viewer.cookie,
    )
    .await;
    assert_eq!(stream.next().await.0, "hello");
    st.changed("reload");
    assert_eq!(stream.next().await.0, "config");
    assert_eq!(st.widget_viewers(), 0);
    let path = format!("/api/v1/boards/{board}");
    owner
        .json(
            "PUT",
            &path,
            json!({"version": 0, "sections": [{
                "title": "Widgets", "position": 0, "items": [{
                    "x": 0, "y": 0, "w": 6, "h": 2,
                    "payload": {"kind": "widget", "widget_id": "board-only"}
                }]
            }]}),
            200,
        )
        .await;
    assert_eq!(stream.next().await.0, "boards");
    assert_eq!(st.widget_viewers(), 1);
    st.changed("w-board-only");
    let (topic, item) = stream.next().await;
    assert_eq!(topic, "widget");
    assert_eq!(item["id"], "board-only");
    owner
        .json("PUT", &path, json!({"version": 1, "sections": []}), 200)
        .await;
    assert_eq!(stream.next().await.0, "boards");
    assert_eq!(st.widget_viewers(), 0);
}

#[tokio::test]
async fn scoped_viewers_only_read_containers_on_their_boards() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st.clone()).await).await;
    let board = owner.board("Service").await;
    let path = format!("/api/v1/boards/{board}");
    let mut document = json!({"version":0,"sections":[{
        "title":"Services","position":0,"items":[{
            "x":0,"y":0,"w":6,"h":2,
            "payload":{"kind":"container","target":{"by":"compose","project":"shop","service":"web"}}
        }]
    }]});
    owner.json("PUT", &path, document.clone(), 200).await;
    let scopes = json!({"boards":[board],"integrations":[]});
    let viewer = owner.guest("scoped-reader", "viewer", scopes.clone()).await;
    let admin = owner.guest("scoped-editor", "admin", scopes).await;

    let list = viewer
        .json("GET", "/api/v1/containers", Value::Null, 200)
        .await;
    assert_eq!(list["groups"][0]["containers"][0]["name"], "web");
    assert_eq!(list["groups"].as_array().unwrap().len(), 1);
    assert_eq!(list["groups"][0]["containers"].as_array().unwrap().len(), 1);
    assert_eq!(list["groups"][0]["stopped"], 0);
    // Both containers need attention, but this reader is granted only web.
    let mut snapshot = (*st.snapshot()).clone();
    for c in &mut snapshot.containers {
        c.state = "restarting".into();
    }
    st.containers.send_replace(std::sync::Arc::new(snapshot));
    assert_eq!(
        owner.json("GET", "/api/v1/alarm", Value::Null, 200).await["problems"],
        3
    );
    assert_eq!(
        viewer.json("GET", "/api/v1/alarm", Value::Null, 200).await["problems"],
        1
    );
    let strips = viewer.json("GET", "/api/v1/strips", Value::Null, 200).await;
    assert_eq!(strips["containers"].as_object().unwrap().len(), 1);
    assert_eq!(strips["stacks"]["shop"], strips["containers"]["web"]);
    for (route, status) in [
        ("/api/v1/containers/web", 200),
        ("/api/v1/containers/db", 404),
        ("/api/history/container/web?metric=cpu", 200),
        ("/api/history/container/db?metric=cpu", 404),
        ("/api/logs/levels?c=web", 200),
        ("/api/logs/levels?c=db", 404),
        ("/api/logs/stream?c=db", 400),
        ("/api/logs/download?c=db", 400),
        ("/api/logs/status", 403),
        ("/api/state", 404),
        ("/api/events", 403),
        ("/api/history/system?metric=cpu", 403),
        ("/api/palette", 404),
        ("/api/v1/stream?topics=system", 403),
        ("/api/stream", 404),
        ("/", 404),
        ("/containers/web", 404),
    ] {
        assert_eq!(
            viewer.call("GET", route, Value::Null).await.status,
            status,
            "{route}"
        );
    }
    use crate::api::access::{Access, ROUTES};
    for (method, route) in ROUTES
        .iter()
        .filter(|r| r.access == Access::Admin)
        .map(|r| {
            (
                r.method,
                r.pattern
                    .replace("<id>", "outside")
                    .replace("<name>", "web"),
            )
        })
        .chain(
            WRITES
                .iter()
                .filter(|(_, path)| path.starts_with("/api/settings/"))
                .map(|(method, path)| (*method, path.to_string())),
        )
        .chain(
            SETTINGS_READS
                .iter()
                .map(|suffix| ("GET", format!("/api/settings/{suffix}"))),
        )
        .chain([("DELETE", path.clone())])
    {
        assert_eq!(
            admin.call(method, &route, json!({})).await.status,
            403,
            "{method} {route}"
        );
    }

    let suggestions_board = owner.board("Allow suggestions").await;
    let users = owner.json("GET", "/api/v1/users", Value::Null, 200).await;
    let user_id = |name: &str| {
        users
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["name"] == name)
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    };
    owner
        .json(
            "PUT",
            &format!("/api/v1/users/{}/scopes", user_id("scoped-editor")),
            json!({"scopes":{"boards":[board,suggestions_board]}}),
            200,
        )
        .await;
    let suggestions = admin
        .json(
            "GET",
            &format!("/api/v1/boards/{suggestions_board}/suggestions"),
            Value::Null,
            200,
        )
        .await;
    assert_eq!(suggestions.as_array().unwrap().len(), 1);
    assert_eq!(suggestions[0]["payload"]["target"]["service"], "web");

    let levels = viewer
        .call("GET", "/api/logs/levels?c=web", Value::Null)
        .await;
    assert!(levels.body.contains("points"));
    let download = viewer
        .call("GET", "/api/logs/download?stack=shop", Value::Null)
        .await;
    assert_eq!(download.status, 200);
    assert!(download.body.contains("web"));
    assert!(!download.body.contains("# db:"));
    let (mut logs, _) = Stream::open(owner.addr, "/api/logs/stream?c=web,db", &viewer.cookie).await;
    assert!(logs.until(b"\n\n").await.starts_with("event: hello\n"));
    assert_eq!(logs.next().await.1["c"], 0);
    assert!(logs.until(b"\n\n").await.starts_with("event: done\n"));

    let (mut stream, _) = Stream::open(
        owner.addr,
        "/api/v1/stream?topics=containers,stats",
        &viewer.cookie,
    )
    .await;
    assert_eq!(stream.next().await.0, "hello");
    st.changed("groups");
    let (topic, stats) = stream.next().await;
    assert_eq!(topic, "stats");
    assert_eq!(
        stats["containers"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        vec!["web"]
    );
    owner
        .json("PUT", &path, json!({"version":1,"sections":[]}), 200)
        .await;
    let (topic, list) = stream.next().await;
    assert_eq!(topic, "containers");
    assert_eq!(list["groups"], json!([]));
    document["version"] = json!(2);
    owner.json("PUT", &path, document, 200).await;
    assert_eq!(
        stream.next().await.1["groups"][0]["containers"][0]["name"],
        "web"
    );

    owner
        .json(
            "PUT",
            &format!("/api/v1/users/{}/scopes", user_id("scoped-reader")),
            json!({"scopes":{"boards":[]}}),
            200,
        )
        .await;
    assert_eq!(stream.next().await.0, "config");
    st.changed("groups");
    assert_eq!(stream.next().await.1["containers"], json!({}));
}

#[tokio::test]
async fn stolen_admin_session_cannot_read_or_retarget_a_secret() {
    let st = crate::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['127.0.0.1']\n");
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
async fn a_new_tls_pin_requires_saved_credentials_to_be_entered_again() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let base = "https://service.test/";
    let (one, two) = ("a".repeat(64), "b".repeat(64));
    let body = |pin: Option<&str>, secret: Option<&str>| {
        let mut input = json!({"kind":"http","name":"Pinned","enabled":true,"base_url":base});
        if let Some(pin) = pin {
            input["fingerprint"] = json!(pin);
        }
        if let Some(secret) = secret {
            input["secret"] = json!(secret);
        }
        input
    };
    let created = owner
        .json(
            "POST",
            "/api/v1/integrations",
            body(None, Some("token")),
            201,
        )
        .await;
    let path = format!("/api/v1/integrations/{}", created["id"].as_str().unwrap());
    let refused = |r: &Resp, what: &str| {
        assert_api_error(r, 400, "bad_request", what);
        let error = serde_json::from_str::<Value>(&r.body).unwrap();
        assert_eq!(error["error"]["field"], "secret", "{what}");
    };
    // Adding a pin, then changing it, keeps nothing unconfirmed.
    refused(
        &owner.call("PUT", &path, body(Some(&one), None)).await,
        "added pin",
    );
    owner
        .json("PUT", &path, body(Some(&one), Some("again")), 200)
        .await;
    owner.json("PUT", &path, body(Some(&one), None), 200).await;
    owner
        .json("PUT", &path, body(Some(&one.to_uppercase()), None), 200)
        .await;
    refused(
        &owner.call("PUT", &path, body(Some(&two), None)).await,
        "changed pin",
    );
    // Removing the pin falls back to public roots and keeps the credential.
    let unpinned = owner.json("PUT", &path, body(None, None), 200).await;
    assert_eq!(unpinned["has_secret"], true);
    // Without a saved credential a pin change needs nothing.
    let bare = owner
        .json(
            "POST",
            "/api/v1/integrations",
            json!({"kind":"http","name":"Bare","enabled":true,"base_url":base}),
            201,
        )
        .await;
    let bare = format!("/api/v1/integrations/{}", bare["id"].as_str().unwrap());
    owner
        .json(
            "PUT",
            &bare,
            json!({"kind":"http","name":"Bare","enabled":true,"base_url":base,"fingerprint":two}),
            200,
        )
        .await;
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
    let st = crate::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['127.0.0.1']\n");
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
    assert_eq!(r["fingerprint"].as_str().unwrap().len(), 64);
    assert!(r["message"]
        .as_str()
        .unwrap()
        .contains("public certificate authorities only"));
    tls.await.unwrap();
}

#[tokio::test]
async fn dashboard_icons_use_the_authenticated_existing_cache() {
    let st = crate::testutil::state_with(Vec::new(), "[icons]\nremote=true\n");
    crate::auth::set_password_for_tests(&st.db, PW);
    let dir = st.env.data_dir.join("icons/di");
    std::fs::create_dir_all(&dir).unwrap();
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><title>cached icon</title></svg>"#;
    std::fs::write(dir.join("homepage.svg"), svg).unwrap();
    let owner = Client::owner(start(st).await).await;
    let bootstrap = owner
        .json("GET", "/api/v1/bootstrap", Value::Null, 200)
        .await;
    assert_eq!(bootstrap["remote_icons"], true);
    let r = owner
        .call("GET", "/api/v1/icons/homepage", Value::Null)
        .await;
    assert_eq!(r.status, 200);
    assert_eq!(r.body, svg);
    assert_eq!(
        header(&r.head, "content-type").as_deref(),
        Some("image/svg+xml")
    );
    assert_eq!(
        header(&r.head, "cache-control").as_deref(),
        Some("private, max-age=86400")
    );
    assert!(crate::http::assets::CSP.contains("img-src 'self' data:"));
}

#[tokio::test]
async fn board_and_integration_creates_enforce_capacity() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    st.db.dashboard(|c| -> rusqlite::Result<()> {
        for _ in 0..63 {
            c.execute("INSERT INTO boards (name,created,updated) VALUES ('Board',0,0)", [])?;
        }
        for _ in 0..127 {
            c.execute("INSERT INTO integrations (name,kind,origin,config) VALUES ('Service','http','https://example.test','{}')", [])?;
        }
        Ok(())
    }).unwrap();
    let owner = Client::owner(start(st).await).await;
    for (path, body, limit) in [
        ("/api/v1/boards", json!({"name":"Board"}), 64),
        (
            "/api/v1/integrations",
            json!({"name":"Service","kind":"http","base_url":"https://example.test","enabled":true}),
            128,
        ),
    ] {
        owner.json("POST", path, body.clone(), 201).await;
        let response = owner.call("POST", path, body).await;
        assert_api_error(&response, 400, "bad_request", path);
        assert!(response.body.contains(&format!("at most {limit}")));
    }
}

#[tokio::test]
async fn deleting_targets_revokes_pending_invites_and_refreshes_boards() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let board = owner.board("Service").await;
    let integration = owner
        .json(
            "POST",
            "/api/v1/integrations",
            json!({
                "name":"Service","kind":"http","enabled":true,"base_url":"https://example.test"
            }),
            201,
        )
        .await;
    let id = integration["id"].as_str().unwrap();
    let path = format!("/api/v1/boards/{board}");
    owner
        .json(
            "PUT",
            &path,
            json!({"version":0,"sections":[{
                "title":"Services","position":0,"items":[{
                    "x":0,"y":0,"w":6,"h":2,
                    "payload":{"kind":"integration","integration_id":id}
                }]
            }]}),
            200,
        )
        .await;
    let invites = "/api/v1/invites";
    owner
        .json(
            "POST",
            invites,
            json!({"role":"viewer","scopes":{"boards":[board]}}),
            201,
        )
        .await;
    owner
        .json(
            "POST",
            invites,
            json!({"role":"viewer","scopes":{"integrations":[id]}}),
            201,
        )
        .await;
    owner
        .json("POST", invites, json!({"role":"viewer","scopes":{}}), 201)
        .await;
    let (mut stream, _) =
        Stream::open(owner.addr, "/api/v1/stream?topics=boards", &owner.cookie).await;
    assert_eq!(stream.next().await.0, "hello");
    assert_eq!(
        owner
            .call("DELETE", &format!("/api/v1/integrations/{id}"), Value::Null)
            .await
            .status,
        204
    );
    assert_eq!(stream.next().await.0, "config");
    assert_eq!(
        owner.json("GET", &path, Value::Null, 200).await["resolved"][0]["missing"],
        true
    );
    assert_eq!(owner.call("DELETE", &path, Value::Null).await.status, 204);
    let invites = owner.json("GET", invites, Value::Null, 200).await;
    for invite in invites.as_array().unwrap() {
        let scopes = &invite["scopes"];
        let affected = scopes["boards"] == json!([board]) || scopes["integrations"] == json!([id]);
        assert_eq!(invite["revoked_at"].is_number(), affected, "{invite}");
    }
}

#[tokio::test]
async fn disabled_integration_cannot_send_its_credential() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let created = owner
        .json(
            "POST",
            "/api/v1/integrations",
            json!({
                "name":"Disabled","kind":"http","enabled":false,
                "base_url":"https://example.test","secret":"saved-token"
            }),
            201,
        )
        .await;
    assert_eq!(created["has_secret"], true);
    let path = format!(
        "/api/v1/integrations/{}/test",
        created["id"].as_str().unwrap()
    );
    let response = owner.call("POST", &path, Value::Null).await;
    assert_api_error(&response, 400, "bad_request", &path);
    assert!(response.body.contains("Enable this integration"));
}

#[tokio::test]
async fn link_tests_honour_the_internet_switch_and_count_redirects_and_challenges_as_reachable() {
    // Internet access off (the default): a public address is refused before
    // any connection. 203.0.113.0/24 is documentation space.
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let link = |url: &str| json!({ "url": url });
    let r = owner
        .json(
            "POST",
            "/api/v1/boards/test-link",
            link("http://203.0.113.1:9/"),
            200,
        )
        .await;
    assert_eq!(
        (&r["ok"], &r["class"]),
        (&json!(false), &json!("internet_off"))
    );

    let st = crate::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['127.0.0.1']\n");
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", upstream.local_addr().unwrap());
    for (status, ok, says) in [
        ("302 Found\r\nLocation: /login", true, "302"),
        ("401 Unauthorized\r\nWWW-Authenticate: Basic", true, "401"),
        ("500 Internal Server Error", false, "non-success"),
    ] {
        let answer = async {
            let (mut socket, _) = upstream.accept().await.unwrap();
            upstream_request(&mut socket).await;
            socket
                .write_all(
                    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
        };
        let (r, ()) = tokio::join!(
            owner.json("POST", "/api/v1/boards/test-link", link(&url), 200),
            answer
        );
        assert_eq!(r["ok"], ok, "{status}: {r}");
        assert!(
            r["message"].as_str().unwrap().contains(says),
            "{status}: {r}"
        );
    }
}

#[tokio::test]
async fn stored_test_verdicts_are_dropped_when_the_internet_switch_changes() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let created = owner
        .json(
            "POST",
            "/api/v1/integrations",
            json!({"name":"Blocked","kind":"http","enabled":true,"base_url":"http://localhost"}),
            201,
        )
        .await;
    let path = format!("/api/v1/integrations/{}", created["id"].as_str().unwrap());
    owner
        .json("POST", &format!("{path}/test"), Value::Null, 200)
        .await;
    let tested = owner.json("GET", &path, Value::Null, 200).await;
    assert_eq!(tested["check"]["verdict"]["class"], "blocked");
    owner
        .json(
            "PATCH",
            "/api/v1/settings",
            json!({"provider_internet":true}),
            200,
        )
        .await;
    assert_eq!(
        owner.json("GET", &path, Value::Null, 200).await["check"],
        Value::Null
    );
}

#[tokio::test]
async fn connection_verdict_enforces_outbound_policy() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let created = owner
        .json(
            "POST",
            "/api/v1/integrations",
            json!({
                "name":"Blocked","kind":"http","enabled":true,"base_url":"http://localhost"
            }),
            201,
        )
        .await;
    let path = format!(
        "/api/v1/integrations/{}/test",
        created["id"].as_str().unwrap()
    );
    for _ in 0..3 {
        let r = owner.json("POST", &path, Value::Null, 200).await;
        assert_eq!(r["class"], "blocked");
        assert_eq!(r["ok"], false);
    }
    assert_api_error(
        &owner.call("POST", &path, Value::Null).await,
        429,
        "rate_limited",
        &path,
    );
    let other = owner
        .guest("independent-budget", "admin", Value::Null)
        .await;
    assert_eq!(
        other.json("POST", &path, Value::Null, 200).await["class"],
        "blocked"
    );
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
        Default::default(),
    )
    .await;
    assert_eq!(
        result.unwrap_err().message,
        "blocked credential destination"
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), destination.accept())
            .await
            .is_err()
    );
}

async fn browser_owner(addr: std::net::SocketAddr) -> Client {
    let response = req(
        addr,
        "POST",
        "/api/v1/onboarding/owner",
        &[
            ("Origin", &format!("http://{addr}")),
            ("Content-Type", "application/json"),
        ],
        &json!({"username":"admin","password":PW}).to_string(),
    )
    .await;
    assert_eq!(response.status, 201);
    let (cookie, csrf) = credentials_of(&response);
    Client { addr, cookie, csrf }
}

const SETUP_DRAFT: &str = "/api/v1/onboarding/draft";
const SETUP_DISCOVERY: &str = "/api/v1/onboarding/discovery";
const SETUP_TEST: &str = "/api/v1/onboarding/test";
const SETUP_COMMIT: &str = "/api/v1/onboarding/commit";
/// The documented setup draft PUT cap (src/api/setup.rs DRAFT_BYTES).
const DRAFT_BYTES: usize = 512 * 1024;

#[tokio::test]
async fn setup_draft_cap_fits_a_maximal_suggestion_draft_and_refuses_more() {
    let st = state();
    let addr = start(st.clone()).await;
    let owner = browser_owner(addr).await;
    owner.json("GET", SETUP_DRAFT, Value::Null, 200).await;
    // 512 items (the item limit), each as long as a Docker-derived suggestion
    // realistically gets: 64-byte names, a 256-byte link and a 96-byte icon.
    let long = |c: char, n: usize| c.to_string().repeat(n);
    let items: Vec<Value> = (0..512)
        .map(|n| {
            json!({
                "id":format!("{n:032x}"),"included":true,"name":long('n', 64),"group":long('g', 64),
                "target":{"kind":"board","payload":{
                    "kind":"container","target":{"by":"compose","project":long('p', 64),"service":long('s', 64)},
                    "title":long('t', 64),"icon":long('i', 96),
                    "link":format!("https://{}.test/", long('l', 240))
                }}
            })
        })
        .collect();
    let mut update =
        json!({"revision":0,"document":{"step":"select","items":items},"credentials":[]});
    let size = update.to_string().len();
    assert!(size < DRAFT_BYTES, "{size}");
    owner.json("PUT", SETUP_DRAFT, update.clone(), 200).await;
    update["revision"] = json!(1);
    update["document"]["items"][0]["source"] = json!(long('x', DRAFT_BYTES));
    owner.json("PUT", SETUP_DRAFT, update, 413).await;
}

#[tokio::test]
async fn setup_resumes_and_commits_once_with_a_failing_connection() {
    let st = state();
    let addr = start(st.clone()).await;
    let owner = browser_owner(addr).await;
    assert_eq!(
        owner.json("GET", "/api/v1/session", Value::Null, 200).await["setup_pending"],
        true
    );
    let initial = owner.json("GET", SETUP_DRAFT, Value::Null, 200).await;
    assert_eq!(initial["document"]["step"], "appearance");
    assert_eq!(initial["theme_default"], "auto");
    let discovery = owner.json("GET", SETUP_DISCOVERY, Value::Null, 200).await;
    assert_eq!(discovery["docker"], "reachable");
    let item = |name: &str, id: &str| {
        json!({
            "id":id,"included":name != "db","name":if name == "web" { "Shop web" } else { name },
            "group":"Shop","target":{"kind":"board","payload":{
                "kind":"container","target":{"by":"compose","project":"shop","service":name}
            }}
        })
    };
    let http_id = "00000000000000000000000000000003";
    let http_item = json!({"id":http_id,"included":true,"name":"Private service","group":"Tools",
        "target":{"kind":"http","base_url":"http://127.0.0.1:1/","enabled":true}});
    let earlier_integration = json!({
        "id":"00000000000000000000000000000004","included":true,"name":"Earlier integration","group":"Tools",
        "target":{"kind":"http","base_url":"https://example.test/","enabled":true}
    });
    let mut document = json!({
        "theme":"ember","step":"connections","items":[
            item("web","00000000000000000000000000000001"),
            item("db","00000000000000000000000000000002"),earlier_integration,http_item
        ]
    });
    let update = json!({"revision":0,"document":document,"credentials":[{
        "item_id":http_id,"base_url":"http://127.0.0.1:1/","secret":"setup-test-only"
    }]});
    let saved = owner.json("PUT", SETUP_DRAFT, update, 200).await;
    assert_eq!(saved["revision"], 1);
    assert_eq!(saved["credential_origins"][http_id], "http://127.0.0.1:1");
    assert!(!saved.to_string().contains("setup-test-only"));

    // Reopen the file and sign in with a new session. No browser storage is
    // needed for the step, editing choices or the staged credential.
    let reopened = AppState::new(st.env.clone());
    let new_addr = start(reopened.clone()).await;
    let resumed = Client::owner(new_addr).await;
    assert_eq!(
        resumed.json("GET", SETUP_DRAFT, Value::Null, 200).await,
        saved
    );
    let result = resumed
        .json(
            "POST",
            SETUP_TEST,
            json!({"revision":1,"item_id":http_id}),
            200,
        )
        .await;
    assert_eq!(result["ok"], false);
    assert_eq!(result["class"], "blocked");
    assert!(!result.to_string().contains("setup-test-only"));

    document["step"] = json!("review");
    document["items"][3]["target"]["base_url"] = json!("http://127.0.0.1:2/");
    resumed
        .json(
            "PUT",
            SETUP_DRAFT,
            json!({"revision":1,"document":document,"credentials":[]}),
            200,
        )
        .await;
    assert_eq!(
        resumed
            .call("POST", SETUP_COMMIT, json!({"revision":2}))
            .await
            .status,
        400
    );
    assert_eq!(
        resumed
            .json("GET", "/api/v1/boards", Value::Null, 200)
            .await,
        json!([])
    );
    assert_eq!(
        resumed
            .json("GET", "/api/v1/integrations", Value::Null, 200)
            .await,
        json!([])
    );
    assert_eq!(
        resumed
            .call(
                "PUT",
                SETUP_DRAFT,
                json!({"revision":1,"document":document,"credentials":[]})
            )
            .await
            .status,
        409
    );
    document["items"][3]["target"]["base_url"] = json!("http://127.0.0.1:1/");
    resumed
        .json(
            "PUT",
            SETUP_DRAFT,
            json!({"revision":2,"document":document,"credentials":[]}),
            200,
        )
        .await;

    let (first, second) = tokio::join!(
        resumed.json("POST", SETUP_COMMIT, json!({"revision":3}), 200),
        resumed.json("POST", SETUP_COMMIT, json!({"revision":3}), 200),
    );
    assert_eq!(first, second);
    assert_eq!(
        first,
        resumed
            .json("POST", SETUP_COMMIT, json!({"revision":3}), 200)
            .await
    );
    let id = first["board_id"].as_str().unwrap();
    let boards = resumed
        .json("GET", "/api/v1/boards", Value::Null, 200)
        .await;
    assert_eq!(boards.as_array().unwrap().len(), 1);
    let view = resumed
        .json("GET", &format!("/api/v1/boards/{id}"), Value::Null, 200)
        .await;
    let sections = view["document"]["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0]["title"], "Shop");
    assert_eq!(sections[0]["items"][0]["payload"]["title"], "Shop web");
    assert_eq!(sections[0]["items"].as_array().unwrap().len(), 1);
    let connections = resumed
        .json("GET", "/api/v1/integrations", Value::Null, 200)
        .await;
    assert_eq!(connections.as_array().unwrap().len(), 2);
    assert_eq!(connections[1]["has_secret"], true);
    assert!(!connections.to_string().contains("setup-test-only"));
    let done = resumed.json("GET", SETUP_DRAFT, Value::Null, 200).await;
    assert_eq!(done["board_id"], id);
    assert_eq!(done["credential_origins"], json!({}));
    assert_eq!(
        resumed
            .call(
                "PUT",
                SETUP_DRAFT,
                json!({"revision":done["revision"],"document":{},"credentials":[]})
            )
            .await
            .status,
        409
    );
    assert_eq!(
        resumed.json("GET", SETUP_DRAFT, Value::Null, 200).await,
        done
    );
    let boot = resumed
        .json("GET", "/api/v1/bootstrap", Value::Null, 200)
        .await;
    assert_eq!(boot["default_board"], id);
    assert_eq!(boot["theme_default"], "ember");
    assert_eq!(
        resumed
            .json("GET", "/api/v1/session", Value::Null, 200)
            .await["setup_pending"],
        false
    );
    resumed
        .json(
            "PATCH",
            "/api/v1/settings",
            json!({"default_theme":null}),
            200,
        )
        .await;
    let boot = resumed
        .json("GET", "/api/v1/bootstrap", Value::Null, 200)
        .await;
    assert_eq!(boot["default_board"], id);
    assert_eq!(boot["theme_default"], "auto");
}

#[tokio::test]
async fn setup_allows_empty_board_when_docker_is_unavailable_and_rejects_non_owners() {
    let st = state();
    st.containers.send_replace(Arc::new(crate::model::Snapshot {
        error: Some("test socket unavailable".into()),
        ..Default::default()
    }));
    let owner = browser_owner(start(st).await).await;
    let discovery = owner.json("GET", SETUP_DISCOVERY, Value::Null, 200).await;
    assert_eq!(discovery["docker"], "unavailable");
    assert_eq!(discovery["suggestions"], json!([]));
    for role in ["admin", "viewer"] {
        let guest = owner
            .guest(&format!("guest-{role}"), role, Value::Null)
            .await;
        for (method, path) in [
            ("GET", SETUP_DRAFT),
            ("GET", SETUP_DISCOVERY),
            ("PUT", SETUP_DRAFT),
            ("POST", SETUP_TEST),
            ("POST", SETUP_COMMIT),
        ] {
            assert_eq!(guest.call(method, path, Value::Null).await.status, 403);
        }
    }
    let draft = owner
        .json(
            "PUT",
            SETUP_DRAFT,
            json!({
                "revision":0,"document":{"step":"review","theme":null,"items":[]},"credentials":[]
            }),
            200,
        )
        .await;
    let result = owner
        .json(
            "POST",
            SETUP_COMMIT,
            json!({"revision":draft["revision"]}),
            200,
        )
        .await;
    let id = result["board_id"].as_str().unwrap();
    let board = owner
        .json("GET", &format!("/api/v1/boards/{id}"), Value::Null, 200)
        .await;
    assert_eq!(board["document"]["sections"], json!([]));
}

#[tokio::test]
async fn board_tour_dismissal_is_idempotent_and_private_to_the_account() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let addr = start(st).await;
    let owner = Client::owner(addr).await;
    let viewer = owner.guest("tour-viewer", "viewer", Value::Null).await;
    let session = "/api/v1/session";
    let dismiss = "/api/v1/account/board-tour";
    assert_eq!(
        viewer.json("GET", session, Value::Null, 200).await["board_tour_pending"],
        true
    );
    for _ in 0..2 {
        assert_eq!(viewer.call("POST", dismiss, Value::Null).await.status, 204);
    }
    assert_eq!(
        viewer.json("GET", session, Value::Null, 200).await["board_tour_pending"],
        false
    );
    let new_login = Client::owner(addr).await;
    assert_eq!(
        new_login.json("GET", session, Value::Null, 200).await["board_tour_pending"],
        true
    );
    assert_eq!(owner.call("POST", dismiss, Value::Null).await.status, 204);
    let new_login = Client::owner(addr).await;
    assert_eq!(
        new_login.json("GET", session, Value::Null, 200).await["board_tour_pending"],
        false
    );
}

#[tokio::test]
async fn setup_keeps_incomplete_entries_and_attributes_invalid_urls() {
    let owner = browser_owner(start(state()).await).await;
    let incomplete_link = "00000000000000000000000000000001";
    let incomplete_http = "00000000000000000000000000000002";
    let ready = "00000000000000000000000000000003";
    let mut document = json!({"step":"review","theme":null,"items":[
        {"id":incomplete_link,"included":true,"name":"Unfinished link","group":"",
           "target":{"kind":"board","payload":{"kind":"link","title":"","url":""}}},
        {"id":incomplete_http,"included":true,"name":"Unfinished integration","group":"",
           "target":{"kind":"http","base_url":"   ","enabled":true}},
        {"id":ready,"included":true,"name":"Ready link","group":"",
           "target":{"kind":"board","payload":{"kind":"link","title":"","url":"invalid"}}}
    ]});
    owner
        .json(
            "PUT",
            SETUP_DRAFT,
            json!({"revision":0,"document":document,"credentials":[]}),
            200,
        )
        .await;
    let error = owner
        .json("POST", SETUP_COMMIT, json!({"revision":1}), 400)
        .await;
    assert_eq!(error["error"]["field"], format!("items.{ready}.url"));

    document["items"][2]["target"]["payload"]["url"] = json!("https://example.test/");
    owner
        .json(
            "PUT",
            SETUP_DRAFT,
            json!({"revision":1,"document":document,"credentials":[]}),
            200,
        )
        .await;
    let result = owner
        .json("POST", SETUP_COMMIT, json!({"revision":2}), 200)
        .await;
    let path = format!("/api/v1/boards/{}", result["board_id"].as_str().unwrap());
    let board = owner.json("GET", &path, Value::Null, 200).await;
    let items = board["document"]["sections"][0]["items"]
        .as_array()
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["payload"]["title"], "Ready link");
    let done = owner.json("GET", SETUP_DRAFT, Value::Null, 200).await;
    assert_eq!(done["document"]["items"].as_array().unwrap().len(), 3);
    assert_eq!(done["document"]["items"][0]["target"]["payload"]["url"], "");
    assert_eq!(done["document"]["items"][1]["target"]["base_url"], "   ");
}

#[tokio::test]
async fn setup_deduplicates_links_by_canonical_identity() {
    let owner = browser_owner(start(state()).await).await;
    let links = [
        "HTTPS://Example.TEST:443",
        "https://example.test/",
        "https://example.test/Path",
        "https://example.test/path",
        "https://example.test/?q=1",
        "https://example.test/?q=2",
        "https://example.test/#first",
        "https://example.test/#second",
    ];
    let mut items: Vec<_> = links.iter().enumerate().map(|(index, url)| json!({
        "id":format!("{index:032x}"),"included":true,"name":format!("Link {index}"),"group":"",
        "target":{"kind":"board","payload":{"kind":"link","title":"","url":url}}
    })).collect();
    owner
        .json(
            "PUT",
            SETUP_DRAFT,
            json!({"revision":0,"document":{"step":"review","items":items},"credentials":[]}),
            200,
        )
        .await;
    let error = owner
        .json("POST", SETUP_COMMIT, json!({"revision":1}), 400)
        .await;
    assert_eq!(
        error["error"]["field"],
        format!("items.{:032x}.included", 1)
    );
    items[1]["included"] = json!(false);
    owner
        .json(
            "PUT",
            SETUP_DRAFT,
            json!({"revision":1,"document":{"step":"review","items":items},"credentials":[]}),
            200,
        )
        .await;
    let result = owner
        .json("POST", SETUP_COMMIT, json!({"revision":2}), 200)
        .await;
    let path = format!("/api/v1/boards/{}", result["board_id"].as_str().unwrap());
    let mut document = owner.json("GET", &path, Value::Null, 200).await["document"].clone();
    let items = document["sections"][0]["items"].as_array_mut().unwrap();
    assert_eq!(items.len(), links.len() - 1);
    items.push(json!({
        "x":0,"y":200,"w":12,"h":1,
        "payload":{"kind":"link","title":"Duplicate","url":links[1]}
    }));
    let error = owner.json("PUT", &path, document, 400).await;
    assert_eq!(error["error"]["field"], "url");
}

#[tokio::test]
async fn setup_reviews_matches_to_saved_integrations_before_reuse_or_creation() {
    let owner = browser_owner(start(state()).await).await;
    let existing = owner.json("POST", "/api/v1/integrations", json!({
        "kind":"http","name":"Saved service","base_url":"https://Example.test:443/path","enabled":true
    }), 201).await;
    let id = existing["id"].as_str().unwrap();
    let reuse = "00000000000000000000000000000001";
    let add = "00000000000000000000000000000002";
    let different = "00000000000000000000000000000003";
    let mut items: Vec<_> = [
        (reuse, "Reused", "/path"),
        (add, "New", "/path"),
        (different, "Different", "/Path"),
    ]
    .into_iter()
    .map(|(id, name, path)| {
        json!({
            "id":id,"included":true,"name":name,"group":"",
            "target":{"kind":"http","base_url":format!("https://example.test{path}"),"enabled":true}
        })
    })
    .collect();
    let saved = owner
        .json(
            "PUT",
            SETUP_DRAFT,
            json!({"revision":0,"document":{"step":"review","items":items},"credentials":[]}),
            200,
        )
        .await;
    assert_eq!(saved["integration_matches"], json!({(reuse):id,(add):id}));
    assert_eq!(
        saved["matching_integrations"],
        json!({(id):[{"id":id,"name":"Saved service"}]})
    );
    let error = owner
        .json("POST", SETUP_COMMIT, json!({"revision":1}), 400)
        .await;
    assert_eq!(
        error["error"]["field"],
        format!("items.{reuse}.integration_choice")
    );
    items[0]["integration_choice"] = json!(id);
    items[1]["integration_choice"] = json!("new");
    owner
        .json(
            "PUT",
            SETUP_DRAFT,
            json!({"revision":1,"document":{"step":"review","items":items},"credentials":[]}),
            200,
        )
        .await;
    let result = owner
        .json("POST", SETUP_COMMIT, json!({"revision":2}), 200)
        .await;
    let path = format!("/api/v1/boards/{}", result["board_id"].as_str().unwrap());
    let board = owner.json("GET", &path, Value::Null, 200).await;
    let payloads = board["document"]["sections"][0]["items"]
        .as_array()
        .unwrap();
    assert_eq!(payloads[0]["payload"]["integration_id"], id);
    assert_eq!(payloads[0]["payload"]["title"], "Reused");
    assert_ne!(payloads[1]["payload"]["integration_id"], id);
    let connections = owner
        .json("GET", "/api/v1/integrations", Value::Null, 200)
        .await;
    assert_eq!(connections.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn discovery_caps_suggestions_before_serializing_container_identity_rows() {
    let st = state();
    let mut snapshot = st.snapshot().as_ref().clone();
    let sample = snapshot.containers[0].clone();
    snapshot.containers = (0..300)
        .map(|i| {
            let mut container = sample.clone();
            container.name = format!("discovery-{i}");
            container.project = None;
            container.service = None;
            // Without a published web port, so no detected app links.
            container.ports.clear();
            container
        })
        .collect();
    st.containers.send_replace(Arc::new(snapshot));
    let owner = browser_owner(start(st).await).await;
    let discovery = owner.json("GET", SETUP_DISCOVERY, Value::Null, 200).await;
    let board_id = owner.board("Discovery").await;
    let board = owner
        .json(
            "GET",
            &format!("/api/v1/boards/{board_id}/suggestions"),
            Value::Null,
            200,
        )
        .await;
    assert_eq!(discovery["suggestions"], board);
    let suggestions = board.as_array().unwrap();
    assert_eq!(suggestions.len(), 256);
    assert_eq!(
        suggestions[255]["payload"]["target"]["name"],
        "discovery-255"
    );
    assert!(suggestions
        .iter()
        .all(|suggestion| suggestion["payload"]["kind"] == "container"
            && suggestion.get("stats").is_none()));
}

#[tokio::test]
async fn setup_draft_limits_fields_without_requiring_complete_values() {
    let owner = browser_owner(start(state()).await).await;
    let id = "00000000000000000000000000000001";
    let link = json!({"id":id,"included":true,"name":"","group":"",
        "target":{"kind":"board","payload":{"kind":"link","title":"","url":""}}});
    let update = |item| json!({"revision":1,"document":{"step":"connections","items":[item]},"credentials":[]});
    let saved = owner
        .json(
            "PUT",
            SETUP_DRAFT,
            json!({"revision":0,"document":{"step":"connections","items":[link]},"credentials":[]}),
            200,
        )
        .await;
    for (pointer, field, limit) in [
        ("/name", "name", 256),
        ("/group", "group", 256),
        ("/target/payload/url", "url", 2048),
    ] {
        let mut item = link.clone();
        *item.pointer_mut(pointer).unwrap() = json!("é".repeat(limit / 2 + 1));
        let error = owner.json("PUT", SETUP_DRAFT, update(item), 400).await;
        assert_eq!(error["error"]["field"], format!("items.{id}.{field}"));
    }
    let mut http = link.clone();
    http["target"] = json!({"kind":"http","base_url":"a".repeat(2049),"enabled":true});
    let error = owner.json("PUT", SETUP_DRAFT, update(http), 400).await;
    assert_eq!(error["error"]["field"], format!("items.{id}.base_url"));
    let mut nested = link.clone();
    nested["target"]["payload"]["icon"] = json!("x".repeat(97));
    let error = owner.json("PUT", SETUP_DRAFT, update(nested), 400).await;
    assert_eq!(error["error"]["field"], format!("items.{id}.icon"));
    let mut sourced = link.clone();
    sourced["source"] = json!("x".repeat(4097));
    let error = owner.json("PUT", SETUP_DRAFT, update(sourced), 400).await;
    assert_eq!(error["error"]["field"], format!("items.{id}.source"));
    assert_eq!(
        owner.json("GET", SETUP_DRAFT, Value::Null, 200).await,
        saved
    );

    let mut at_limit = link;
    at_limit["name"] = json!("é".repeat(128));
    at_limit["group"] = json!("b".repeat(256));
    at_limit["target"]["payload"]["url"] = json!("h".repeat(2048));
    owner.json("PUT", SETUP_DRAFT, update(at_limit), 200).await;
}

#[tokio::test]
async fn provider_widgets_poll_only_for_open_boards_and_share_scrubbed_data() {
    use std::time::Duration;

    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/fixture", upstream.local_addr().unwrap());
    let state = crate::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['127.0.0.1']\n");
    crate::auth::set_password_for_tests(&state.db, PW);
    let owner = Client::owner(start(state.clone()).await).await;
    let run = tokio::spawn(crate::live::run(state.clone()));
    let board = owner.board("Provider test").await;
    let elsewhere = owner.board("Other board").await;
    let token = r#"M3-only-fake"provider\credential"#;
    let install = |name: &str, board: &str| {
        json!({
            "integration": {"name": name, "kind":"http", "enabled":true, "base_url":url, "secret":token},
            "board_id": board, "widgets":[{"id":"json","options":{"paths":["reading", "echo", "nested"], "refresh":15}}]
        })
    };
    crate::log::capture_test_logs();
    let first = owner
        .json(
            "POST",
            "/api/v1/providers/install",
            install("Fixture", &board),
            201,
        )
        .await;
    let second = owner
        .json(
            "POST",
            "/api/v1/providers/install",
            install("Second", &elsewhere),
            201,
        )
        .await;
    assert_ne!(first["id"], second["id"]);
    assert!(!first.to_string().contains(token));
    assert!(!second.to_string().contains(token));
    let path = format!("/api/v1/boards/{board}");
    // The widget's declared default size (4 × 2), not the kind default (4 × 3).
    let placed = owner.json("GET", &path, Value::Null, 200).await;
    let tile = &placed["document"]["sections"][0]["items"][0];
    assert_eq!((tile["w"].clone(), tile["h"].clone()), (json!(4), json!(2)));
    let view = owner.json("GET", &path, Value::Null, 200).await;
    assert_eq!(view["resolved"][0]["widget"]["status"], "waiting");
    assert!(
        tokio::time::timeout(Duration::from_millis(100), upstream.accept())
            .await
            .is_err()
    );

    let stream_path = format!("/api/v1/stream?topics=widget,boards&board={board}");
    let (mut a, _) = Stream::open(owner.addr, &stream_path, &owner.cookie).await;
    let (mut b, _) = Stream::open(owner.addr, &stream_path, &owner.cookie).await;
    for stream in [&mut a, &mut b] {
        assert_eq!(stream.next().await.0, "hello");
        assert_eq!(stream.next().await.1["status"], "waiting");
    }
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), upstream.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(upstream_request(&mut socket)
        .await
        .contains(&format!("authorization: Bearer {token}\r\n")));
    let encoded = json!(token).to_string();
    let body = json!({
        "reading":"42 units", "echo":token, "nested":{"echo": &encoded[1..encoded.len() - 1]}
    })
    .to_string();
    socket
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    for stream in [&mut a, &mut b] {
        let (topic, widget) = stream.next().await;
        assert_eq!(topic, "widget");
        assert_eq!(widget["status"], "ready");
        assert_eq!(
            widget["structured"]["rows"],
            json!([
                ["reading", "42 units"],
                ["echo", "<redacted>"],
                ["nested", r#"{"echo":"<redacted>"}"#]
            ])
        );
        assert!(!widget.to_string().contains(token));
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(100), upstream.accept())
            .await
            .is_err()
    );
    drop(a);
    drop(b);
    for _ in 0..40 {
        if state.viewers() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(state.viewers(), 0);
    // A test uses only the updated origin and clears the old token atomically.
    let path = format!("/api/v1/integrations/{}", first["id"].as_str().unwrap());
    let unavailable = "http://127.0.0.1:1";
    let updated = owner.json("PUT", &path, json!({
        "kind":"http","name":"Fixture","base_url":unavailable,"enabled":true,"clear_secret":true
    }), 200).await;
    assert_eq!(updated["has_secret"], false);
    assert!(crate::log::take_test_logs()
        .iter()
        .all(|line| !line.contains(token)));
    run.abort();
}

#[tokio::test]
async fn scoped_admins_can_test_and_configure_granted_provider_integrations() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let st = crate::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['127.0.0.1']\n");
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let board = owner.board("Granted board").await;
    let other = owner.board("Private board").await;
    let input = json!({"name":"Existing","kind":"http","enabled":true,"base_url":url});
    let granted = owner
        .json("POST", "/api/v1/integrations", input.clone(), 201)
        .await;
    let denied = owner
        .json("POST", "/api/v1/integrations", input.clone(), 201)
        .await;
    let admin = owner
        .guest(
            "scoped-provider-admin",
            "admin",
            json!({
                "boards":[board],"integrations":[granted["id"]]
            }),
        )
        .await;
    let test = "/api/v1/providers/test";
    let install = "/api/v1/providers/install";
    for route in [test, install] {
        admin
            .json("POST", route, json!({"integration":input}), 403)
            .await;
        admin
            .json(
                "POST",
                route,
                json!({"integration":input,"integration_id":denied["id"]}),
                403,
            )
            .await;
    }
    let fake = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        upstream_request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 204 No Content\r\n\r\n")
            .await
            .unwrap();
    });
    let draft = json!({"name":"Edited","kind":"http","enabled":true,"base_url":url});
    let result = admin
        .json(
            "POST",
            test,
            json!({"integration":draft,"integration_id":granted["id"]}),
            200,
        )
        .await;
    assert_eq!(result["ok"], true);
    fake.await.unwrap();

    let placement = |board: &str, widget_options: Value| {
        json!({
            "integration":draft,"integration_id":granted["id"],"board_id":board,
            "widgets":[{"id":"json","options":widget_options}]
        })
    };
    admin
        .json("POST", install, placement(&other, json!({})), 404)
        .await;
    let id = granted["id"].as_str().unwrap();
    let options =
        |id: &str, board: &str| format!("/api/v1/providers/widgets?integration={id}&board={board}");
    admin
        .json("GET", &options(id, &other), Value::Null, 404)
        .await;
    admin
        .json(
            "GET",
            &options(denied["id"].as_str().unwrap(), &board),
            Value::Null,
            403,
        )
        .await;
    let saved = admin
        .json(
            "POST",
            install,
            placement(&board, json!({"paths":["reading"]})),
            201,
        )
        .await;
    assert_eq!(saved["name"], "Edited");
    let saved_options = admin
        .json("GET", &options(id, &board), Value::Null, 200)
        .await;
    let mut change = placement(&board, json!({"paths":["new_reading"]}));
    change["widgets"][0]["placement"] = saved_options["json"][0]["id"].clone();
    admin.json("POST", install, change, 201).await;
    let saved_options = admin
        .json("GET", &options(id, &board), Value::Null, 200)
        .await;
    assert_eq!(
        saved_options["json"][0]["options"]["paths"],
        json!(["new_reading"])
    );

    // Switching a saved placement off removes it in one board revision.
    let placed = saved_options["json"][0]["id"].clone();
    let board_path = format!("/api/v1/boards/{board}");
    let before = admin.json("GET", &board_path, Value::Null, 200).await;
    let mut foreign = placement(&board, json!({}));
    foreign["widgets"] = json!([]);
    foreign["remove"] = json!(["provider:not-this-integration"]);
    admin.json("POST", install, foreign, 400).await;
    let mut both = placement(&board, json!({"paths":["value"]}));
    both["widgets"][0]["placement"] = placed.clone();
    both["remove"] = json!([placed]);
    admin.json("POST", install, both, 400).await;
    let mut off = placement(&board, json!({}));
    off["widgets"] = json!([]);
    off["remove"] = json!([placed]);
    admin.json("POST", install, off, 201).await;
    let saved_options = admin
        .json("GET", &options(id, &board), Value::Null, 200)
        .await;
    assert_eq!(saved_options, json!({}));
    let after = admin.json("GET", &board_path, Value::Null, 200).await;
    assert_eq!(
        after["document"]["version"].as_u64().unwrap(),
        before["document"]["version"].as_u64().unwrap() + 1
    );
    assert!(!after["document"]
        .to_string()
        .contains(placed.as_str().unwrap()));
}

#[tokio::test]
async fn credentialed_json_widgets_require_specific_paths() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let board = owner.board("Readings").await;
    for options in [
        json!({}),
        json!({"paths":["$"]}),
        json!({"paths":["value","$."]}),
    ] {
        let error = owner
            .json(
                "POST",
                "/api/v1/providers/install",
                json!({
                    "integration":{"name":"Fixture","kind":"http","base_url":"http://fixture.test",
                                   "enabled":true,"secret":"fixture-token"},
                    "board_id":board,"widgets":[{"id":"json","options":options}]
                }),
                400,
            )
            .await;
        assert_eq!(error["error"]["field"], "widgets.0.paths");
    }
    assert_eq!(
        owner
            .json("GET", "/api/v1/integrations", Value::Null, 200)
            .await,
        json!([])
    );
}

#[tokio::test]
async fn integration_tiles_are_masked_without_the_grant_and_round_trip() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let board = owner.board("Shared").await;
    let integration = owner
        .json(
            "POST",
            "/api/v1/integrations",
            json!({"kind":"http","name":"Hidden name","enabled":true,"base_url":"http://service.test/"}),
            201,
        )
        .await;
    let integration = integration["id"].as_str().unwrap().to_owned();
    let path = format!("/api/v1/boards/{board}");
    let tile = |title: &str| {
        json!({"x":0,"y":0,"w":6,"h":2,
            "payload":{"kind":"integration","integration_id":integration,"title":title}})
    };
    let doc = json!({"version":0,"sections":[{"title":"Tiles","position":0,"items":[tile("Custom private title")]}]});
    let saved = owner.json("PUT", &path, doc, 200).await;
    let scopes = json!({"boards":[board],"integrations":[]});
    let editor = owner.guest("tile-editor", "admin", scopes).await;
    let seen = editor.json("GET", &path, Value::Null, 200).await;
    let text = seen.to_string();
    assert!(!text.contains(&integration), "{text}");
    assert!(!text.contains("Custom private title"), "{text}");
    assert!(text.contains("Integration unavailable"), "{text}");
    let item = &seen["document"]["sections"][0]["items"][0];
    assert_eq!(item["payload"]["integration_id"], item["id"]);
    // Saving the masked board (draft and board) keeps the real tile.
    let mut doc = seen["document"].clone();
    doc["sections"][0]["title"] = json!("Renamed");
    editor
        .json("PUT", &format!("{path}/draft"), doc.clone(), 200)
        .await;
    editor.json("PUT", &path, doc, 200).await;
    let after = owner.json("GET", &path, Value::Null, 200).await;
    let payload = &after["document"]["sections"][0]["items"][0]["payload"];
    assert_eq!(payload["integration_id"], json!(integration));
    assert_eq!(payload["title"], "Custom private title");
    assert_eq!(after["document"]["sections"][0]["title"], "Renamed");
    assert_eq!(
        after["resolved"][0]["integration"]["id"],
        saved["resolved"][0]["integration"]["id"]
    );
}

#[tokio::test]
async fn provider_widgets_require_board_and_integration_grants() {
    use std::time::Duration;

    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let state = crate::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['127.0.0.1']\n");
    crate::auth::set_password_for_tests(&state.db, PW);
    let owner = Client::owner(start(state.clone()).await).await;
    let run = tokio::spawn(crate::live::run(state.clone()));
    let board = owner.board("Shared board").await;
    let integration = owner.json("POST", "/api/v1/providers/install", json!({
        "integration":{"name":"Private service name","kind":"http","enabled":true,
                       "base_url":format!("http://{}", upstream.local_addr().unwrap()),"secret":"fake-token"},
        "board_id":board,"widgets":[{"id":"json","options":{"paths":["value"]}}]
    }), 201).await;
    let id = &integration["id"];
    let board_only = json!({"boards":[board],"integrations":[]});
    let both = json!({"boards":[board],"integrations":[id]});
    let viewer = owner
        .guest("provider-viewer", "viewer", board_only.clone())
        .await;
    let admin = owner
        .guest("provider-editor", "admin", board_only.clone())
        .await;
    let viewer_id = state
        .db
        .accounts()
        .unwrap()
        .into_iter()
        .find(|account| account.name == "provider-viewer")
        .unwrap()
        .id;
    let scopes = format!("/api/v1/users/{viewer_id}/scopes");
    let path = format!("/api/v1/boards/{board}");
    let stream_path = format!("/api/v1/stream?topics=widget,boards&board={board}");
    let redacted = viewer.json("GET", &path, Value::Null, 200).await;
    assert_eq!(redacted["resolved"][0]["widget"], Value::Null);
    assert!(!redacted.to_string().contains("Private service name"));
    assert!(redacted.to_string().contains("Widget unavailable"));
    let mut doc = admin.json("GET", &path, Value::Null, 200).await["document"].clone();
    let mut copied = doc["sections"][0]["items"][0].clone();
    let widget = copied["payload"]["widget_id"].as_str().unwrap().to_owned();
    copied.as_object_mut().unwrap().remove("id");
    copied["y"] = json!(100);
    doc["sections"][0]["items"]
        .as_array_mut()
        .unwrap()
        .push(copied);
    let refused = admin.json("PUT", &path, doc, 400).await;
    assert_eq!(refused["error"]["field"], "widget_id");

    let (mut stream, _) = Stream::open(viewer.addr, &stream_path, &viewer.cookie).await;
    assert_eq!(stream.next().await.0, "hello");
    assert!(stream.quiet(100).await);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), upstream.accept())
            .await
            .is_err()
    );

    owner
        .json("PUT", &scopes, json!({"scopes":both}), 200)
        .await;
    assert_eq!(stream.next().await.0, "config");
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), upstream.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(upstream_request(&mut socket)
        .await
        .contains("authorization: Bearer fake-token"));
    let body = r#"{"value":"private reading"}"#;
    socket
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let (topic, body) = stream.next().await;
    assert_eq!(topic, "widget");
    assert_eq!(
        body["structured"]["rows"],
        json!([["value", "private reading"]])
    );

    owner
        .json("PUT", &scopes, json!({"scopes":board_only}), 200)
        .await;
    assert_eq!(stream.next().await.0, "config");
    let _ = state.events.send(format!("w-{widget}").into());
    assert!(stream.quiet(150).await);
    let hidden = viewer.json("GET", &path, Value::Null, 200).await;
    assert_eq!(hidden["resolved"][0]["widget"], Value::Null);
    assert!(!hidden.to_string().contains("private reading"));
    run.abort();
}

#[tokio::test]
async fn provider_requests_are_capped_in_size_and_time_and_block_public_addresses() {
    use std::time::Duration;
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", upstream.local_addr().unwrap());
    let state = crate::testutil::state_with(
        Vec::new(),
        "[http]\nallow_hosts=['127.0.0.1','203.0.113.1']\n",
    );
    crate::auth::set_password_for_tests(&state.db, PW);
    let owner = Client::owner(start(state.clone()).await).await;
    let body = |url: &str| json!({"integration":{"name":"Fixture","kind":"http","enabled":true,"base_url":url}});
    let endpoint = "/api/v1/providers/test";
    let blocked = owner
        .json("POST", endpoint, body("http://203.0.113.1"), 200)
        .await;
    assert_eq!(blocked["class"], "internet_off");
    let board = owner.board("Response size").await;
    let mut install = body(&url);
    install["board_id"] = json!(board);
    install["widgets"] = json!([{"id":"json"}]);
    owner
        .json("POST", "/api/v1/providers/install", install, 201)
        .await;
    let run = tokio::spawn(crate::live::run(state));
    let path = format!("/api/v1/stream?topics=widget&board={board}");
    let (mut stream, _) = Stream::open(owner.addr, &path, &owner.cookie).await;
    assert_eq!(stream.next().await.0, "hello");
    assert_eq!(stream.next().await.1["status"], "waiting");
    let fake = tokio::spawn(async move {
        let (mut socket, _) = upstream.accept().await.unwrap();
        upstream_request(&mut socket).await;
        let size = crate::providers::MAX_RESPONSE + 1;
        socket
            .write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let _ = socket.write_all(&vec![b'x'; size]).await;
        let (mut socket, _) = upstream.accept().await.unwrap();
        upstream_request(&mut socket).await;
        tokio::time::sleep(Duration::from_secs(6)).await;
    });
    let large = stream.next().await;
    assert_eq!(large.0, "widget");
    assert!(large.1["error"].as_str().unwrap().contains("size limit"));
    drop(stream);
    let elapsed = std::time::Instant::now();
    let timeout = owner.json("POST", endpoint, body(&url), 200).await;
    assert_eq!(timeout["class"], "timeout");
    assert!((Duration::from_secs(4)..Duration::from_secs(6)).contains(&elapsed.elapsed()));
    fake.abort();
    run.abort();
}

#[tokio::test]
async fn provider_refresh_concurrency_is_global_and_disconnect_releases_slots() {
    use std::time::Duration;
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", upstream.local_addr().unwrap());
    let state = crate::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['127.0.0.1']\n");
    crate::auth::set_password_for_tests(&state.db, PW);
    let owner = Client::owner(start(state.clone()).await).await;
    let run = tokio::spawn(crate::live::run(state.clone()));
    let board = owner.board("Many widgets").await;
    let other = owner.board("Two boards use the same limit").await;
    for i in 0..5 {
        owner.json("POST", "/api/v1/providers/install", json!({
            "integration":{"name":format!("Fixture {i}"),"kind":"http","base_url":url,"enabled":true},
            "board_id": if i < 3 { &board } else { &other },
            "widgets":[{"id":"json"}]
        }), 201).await;
    }

    let path = |id: &str| format!("/api/v1/stream?topics=widget&board={id}");
    let (a, _) = Stream::open(owner.addr, &path(&board), &owner.cookie).await;
    let (b, _) = Stream::open(owner.addr, &path(&other), &owner.cookie).await;
    let mut sockets = Vec::new();
    for _ in 0..4 {
        sockets.push(
            tokio::time::timeout(Duration::from_secs(3), upstream.accept())
                .await
                .unwrap()
                .unwrap()
                .0,
        );
    }
    assert_eq!(state.provider_requests.available_permits(), 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(200), upstream.accept())
            .await
            .is_err()
    );
    let blocked = owner
        .json(
            "POST",
            "/api/v1/providers/test",
            json!({
                "integration":{"name":"Would be sixth","kind":"http","base_url":url,"enabled":true}
            }),
            200,
        )
        .await;
    assert_eq!(blocked["class"], "busy");
    drop(a);
    drop(b);
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.viewers() != 0 || state.provider_requests.available_permits() != 4 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(200), upstream.accept())
            .await
            .is_err()
    );
    run.abort();
}

#[tokio::test]
async fn provider_connection_and_policy_edits_close_upstream_sockets() {
    use std::time::Duration;
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", upstream.local_addr().unwrap());
    let state = crate::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['127.0.0.1']\n");
    crate::auth::set_password_for_tests(&state.db, PW);
    let owner = Client::owner(start(state.clone()).await).await;
    owner
        .json(
            "PATCH",
            "/api/v1/settings",
            json!({"provider_internet":true}),
            200,
        )
        .await;
    let input = |url: &str| json!({"kind":"http","name":"Fixture","base_url":url,"enabled":true});
    let saved = owner
        .json("POST", "/api/v1/integrations", input(&url), 201)
        .await;
    let integration = format!("/api/v1/integrations/{}", saved["id"].as_str().unwrap());
    for change in ["internet", "connection", "allow_hosts"] {
        let path = format!("{integration}/test");
        let test = owner.json("POST", &path, Value::Null, 200);
        let edit = async {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), upstream.accept())
                .await
                .unwrap()
                .unwrap();
            let _ = upstream_request(&mut socket).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\n")
                .await
                .unwrap();
            match change {
                "internet" => {
                    owner
                        .json(
                            "PATCH",
                            "/api/v1/settings",
                            json!({"provider_internet":false}),
                            200,
                        )
                        .await;
                }
                "connection" => {
                    owner
                        .json("PUT", &integration, input(&format!("{url}/edited")), 200)
                        .await;
                }
                "allow_hosts" => {
                    std::fs::write(
                        state.config.path(),
                        "[http]\nallow_hosts=['other.invalid']\n",
                    )
                    .unwrap();
                    assert!(state.config.reload());
                }
                _ => unreachable!(),
            }
            // Closing with unread upstream bytes can send RST instead of FIN.
            let closed = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut [0]))
                .await
                .expect("upstream socket closed after revision change");
            match closed {
                Ok(0) => {}
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                other => panic!("upstream socket stayed open: {other:?}"),
            }
        };
        let (verdict, ()) = tokio::join!(test, edit);
        assert_eq!(verdict["class"], "changed");
    }
    assert_eq!(state.provider_requests.available_permits(), 4);
}

#[tokio::test]
async fn provider_placements_keep_options_and_share_only_identical_fetch_inputs() {
    use std::time::Duration;
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", upstream.local_addr().unwrap());
    let state = crate::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['127.0.0.1']\n");
    crate::auth::set_password_for_tests(&state.db, PW);
    let owner = Client::owner(start(state.clone()).await).await;
    let a = owner.board("A").await;
    let b = owner.board("B").await;
    let c = owner.board("C").await;
    let input = json!({"kind":"http","name":"Fixture","base_url":url,"enabled":true});
    let integration = owner
        .json("POST", "/api/v1/integrations", input.clone(), 201)
        .await;
    let install = |board: &str, path: &str, refresh| {
        json!({
            "integration":input, "integration_id":integration["id"], "board_id":board,
            "widgets":[{"id":"json","options":{"paths":[path],"refresh":refresh}}]
        })
    };
    for (board, path, refresh) in [(&a, "one", 15), (&b, "two", 30), (&c, "one", 60)] {
        owner
            .json(
                "POST",
                "/api/v1/providers/install",
                install(board, path, refresh),
                201,
            )
            .await;
        let opts = format!(
            "/api/v1/providers/widgets?integration={}&board={board}",
            integration["id"].as_str().unwrap()
        );
        let config = owner.json("GET", &opts, Value::Null, 200).await;
        assert_eq!(
            config["json"][0]["options"],
            json!({"paths":[path],"refresh":refresh})
        );
    }
    let stream = |id: &str| format!("/api/v1/stream?topics=widget&board={id}");
    let (mut a_stream, _) = Stream::open(owner.addr, &stream(&a), &owner.cookie).await;
    let (mut c_stream, _) = Stream::open(owner.addr, &stream(&c), &owner.cookie).await;
    let run = tokio::spawn(crate::live::run(state));
    for stream in [&mut a_stream, &mut c_stream] {
        assert_eq!(stream.next().await.0, "hello");
        assert_eq!(stream.next().await.1["status"], "waiting");
    }
    let reply = async {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), upstream.accept())
            .await
            .unwrap()
            .unwrap();
        upstream_request(&mut socket).await;
        let body = r#"{"one":"first","two":"second"}"#;
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    };
    reply.await;
    let first = a_stream.next().await.1;
    let shared = c_stream.next().await.1;
    assert_ne!(first["id"], shared["id"]);
    assert_eq!(first["structured"]["rows"], json!([["one", "first"]]));
    assert_eq!(first["structured"], shared["structured"]);
    assert!(
        tokio::time::timeout(Duration::from_millis(150), upstream.accept())
            .await
            .is_err()
    );
    let other = owner
        .json("GET", &format!("/api/v1/boards/{b}"), Value::Null, 200)
        .await;
    assert_eq!(other["resolved"][0]["widget"]["status"], "waiting");
    let mut edit = install(&b, "two", 45);
    edit["widgets"][0]["placement"] = first["id"].clone();
    assert_eq!(
        owner
            .call("POST", "/api/v1/providers/install", edit.clone())
            .await
            .status,
        400
    );
    edit["board_id"] = json!(a);
    owner
        .json("POST", "/api/v1/providers/install", edit, 201)
        .await;
    let config = owner
        .json(
            "GET",
            &format!(
                "/api/v1/providers/widgets?integration={}&board={b}",
                integration["id"].as_str().unwrap()
            ),
            Value::Null,
            200,
        )
        .await;
    assert_eq!(
        config["json"][0]["options"],
        json!({"paths":["two"],"refresh":30})
    );
    run.abort();
}

#[tokio::test]
async fn provider_credentials_keep_replace_and_clear_independently() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let url = "http://pihole.invalid";
    let input = |url: &str, credentials: Value| {
        json!({
            "kind":"pihole","name":"Pi-hole","base_url":url,"enabled":true,"credentials":credentials,
        })
    };
    let saved = owner
        .json(
            "POST",
            "/api/v1/integrations",
            input(
                url,
                json!({"password":"fixture password","api_token":"fixture-token"}),
            ),
            201,
        )
        .await;
    assert_eq!(
        saved["admin"]["credentials"],
        json!({"password":url,"api_token":url})
    );
    assert!(!saved.to_string().contains("fixture"));
    let path = format!("/api/v1/integrations/{}", saved["id"].as_str().unwrap());
    let saved = owner
        .json(
            "PUT",
            &path,
            input(
                "http://pihole.invalid/prefix",
                json!({"password":"fixture replacement"}),
            ),
            200,
        )
        .await;
    assert_eq!(
        saved["admin"]["credentials"],
        json!({"password":url,"api_token":url})
    );
    assert!(!saved.to_string().contains("fixture"));
    let other = "https://other.invalid";
    let denied = owner
        .call(
            "PUT",
            &path,
            input(other, json!({"password":"fixture replacement"})),
        )
        .await;
    assert_eq!(denied.status, 400);
    let saved = owner
        .json(
            "PUT",
            &path,
            input(
                other,
                json!({"password":"fixture replacement","api_token":null}),
            ),
            200,
        )
        .await;
    assert_eq!(saved["admin"]["credentials"], json!({"password":other}));
    let saved = owner
        .json("PUT", &path, input(other, json!({"password":null})), 200)
        .await;
    assert_eq!(saved["admin"]["credentials"], json!({}));
}

#[tokio::test]
async fn guided_setup_saves_the_registry_provider_and_all_staged_fields() {
    let owner = browser_owner(start(state()).await).await;
    let item = "00000000000000000000000000000001";
    let url = "http://pihole.invalid";
    let draft = owner.json("PUT", SETUP_DRAFT, json!({
        "revision":0,
        "document":{"step":"review","items":[{
            "id":item,"included":true,"name":"DNS","group":"Network",
            "target":{"kind":"provider","provider":"pihole","base_url":url,"enabled":true,"options":{"version":"v5"}}
        }]},
        "credentials":[
            {"item_id":item,"base_url":url,"field":"password","secret":"test-fixture-password"},
            {"item_id":item,"base_url":url,"field":"api_token","secret":"test-fixture-token"}
        ]
    }), 200).await;
    assert_eq!(
        draft["credential_bindings"][item],
        json!({"password":url,"api_token":url})
    );
    assert!(!draft.to_string().contains("test-fixture"));
    owner
        .json(
            "POST",
            SETUP_COMMIT,
            json!({"revision":draft["revision"]}),
            200,
        )
        .await;
    let integrations = owner
        .json("GET", "/api/v1/integrations", Value::Null, 200)
        .await;
    assert_eq!(integrations[0]["kind"], "pihole");
    assert_eq!(integrations[0]["admin"]["options"]["version"], "v5");
    assert_eq!(
        integrations[0]["admin"]["credentials"],
        json!({"password":url,"api_token":url})
    );
    assert!(!integrations.to_string().contains("test-fixture"));
}

#[tokio::test]
async fn swapping_provider_references_preserves_their_source_options() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let board = owner.board("Swap").await;
    let mut integrations = Vec::new();
    for name in ["first", "second"] {
        let integration = owner.json("POST", "/api/v1/providers/install", json!({
            "integration":{"name":name,"kind":"http","enabled":true,"base_url":"http://fixture.invalid"},
            "board_id":board, "widgets":[{"id":"json","options":{"paths":[name]}}]
        }), 201).await;
        integrations.push(integration["id"].as_str().unwrap().to_owned());
    }
    let path = format!("/api/v1/boards/{board}");
    let mut doc = owner.json("GET", &path, Value::Null, 200).await["document"].clone();
    let mut widgets = Vec::new();
    for section in doc["sections"].as_array().unwrap() {
        widgets.push(section["items"][0]["payload"]["widget_id"].clone());
    }
    doc["sections"][0]["items"][0]["payload"]["widget_id"] = widgets[1].clone();
    doc["sections"][1]["items"][0]["payload"]["widget_id"] = widgets[0].clone();
    owner.json("PUT", &path, doc, 200).await;
    for (index, name) in ["first", "second"].into_iter().enumerate() {
        let id = &integrations[index];
        let options = owner
            .json(
                "GET",
                &format!("/api/v1/providers/widgets?integration={id}&board={board}"),
                Value::Null,
                200,
            )
            .await;
        assert_eq!(options["json"][0]["id"], widgets[1 - index]);
        assert_eq!(options["json"][0]["options"]["paths"], json!([name]));
    }
}

#[tokio::test]
async fn changing_provider_options_cancels_an_unneeded_in_flight_fetch() {
    use std::time::Duration;

    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let st = crate::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['127.0.0.1']\n");
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st.clone()).await).await;
    let board = owner.board("Change options").await;
    let path = "/api/v1/providers/install";
    let mut install = json!({
        "integration":{"name":"Fixture","kind":"http","enabled":true,
                       "base_url":format!("http://{}", upstream.local_addr().unwrap())},
        "board_id":board, "widgets":[{"id":"json","options":{"paths":["first"]}}]
    });
    let connection = owner.json("POST", path, install.clone(), 201).await;
    let (mut stream, _) = Stream::open(
        owner.addr,
        &format!("/api/v1/stream?topics=widget&board={board}"),
        &owner.cookie,
    )
    .await;
    assert_eq!(stream.next().await.0, "hello");
    assert_eq!(stream.next().await.1["status"], "waiting");
    let run = tokio::spawn(crate::live::run(st));
    let (mut original, _) = tokio::time::timeout(Duration::from_secs(3), upstream.accept())
        .await
        .unwrap()
        .unwrap();
    upstream_request(&mut original).await;

    install["integration_id"] = connection["id"].clone();
    install["widgets"][0]["options"]["paths"] = json!(["second"]);
    owner.json("POST", path, install, 201).await;
    let closed = tokio::time::timeout(Duration::from_secs(2), original.read(&mut [0]))
        .await
        .expect("old request cancelled after options changed");
    match closed {
        Ok(0) => {}
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
        other => panic!("upstream socket stayed open: {other:?}"),
    }
    let (mut replacement, _) = tokio::time::timeout(Duration::from_secs(3), upstream.accept())
        .await
        .unwrap()
        .unwrap();
    upstream_request(&mut replacement).await;
    let body = r#"{"first":"old","second":"new"}"#;
    replacement
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    loop {
        let (topic, frame) = stream.next().await;
        if topic == "widget" {
            assert_eq!(frame["structured"]["rows"], json!([["second", "new"]]));
            break;
        }
    }
    run.abort();
}

#[tokio::test]
async fn editing_and_committing_setup_cancel_in_flight_tests() {
    use std::time::Duration;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let st = crate::testutil::state_with(Vec::new(), "[http]\nallow_hosts=['127.0.0.1']\n");
    let addr = start(st.clone()).await;
    let owner = browser_owner(addr).await;
    let item = "11111111111111111111111111111111";
    let document = json!({"step":"review", "items":[{
        "id":item, "name":"Fixture", "group":"", "included":true,
        "target":{"kind":"provider","provider":"http","base_url":url,"enabled":true}
    }]});
    owner.json("PUT", SETUP_DRAFT, json!({
        "revision":0, "document":document, "credentials":[{"item_id":item,"base_url":url,"secret":"fixture-token"}]
    }), 200).await;
    let other_tab = Client::owner(addr).await;
    for revision in [1, 2] {
        let test = owner.json(
            "POST",
            SETUP_TEST,
            json!({"revision":revision,"item_id":item}),
            200,
        );
        let change = async {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
                .await
                .unwrap()
                .unwrap();
            upstream_request(&mut socket).await;
            if revision == 1 {
                other_tab
                    .json(
                        "PUT",
                        SETUP_DRAFT,
                        json!({
                            "revision":revision, "document":document,
                            "credentials":[{"item_id":item,"base_url":url,"secret":null}]
                        }),
                        200,
                    )
                    .await;
            } else {
                other_tab
                    .json("POST", SETUP_COMMIT, json!({"revision":revision}), 200)
                    .await;
            }
            let closed = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut [0]))
                .await
                .expect("draft revision cancels the upstream request");
            match closed {
                Ok(0) => {}
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                other => panic!("upstream socket stayed open: {other:?}"),
            }
        };
        let (verdict, ()) = tokio::join!(test, change);
        assert_eq!(verdict["class"], "changed");
    }
    assert_eq!(st.provider_requests.available_permits(), 4);
}

#[tokio::test]
async fn renaming_and_deleting_an_integration_hide_legacy_widget_titles() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st.clone()).await).await;
    let board = owner.board("Shared").await;
    let mut input = json!({
        "name":"Former private name","kind":"http","enabled":true,"base_url":"http://fixture.invalid"
    });
    let saved = owner
        .json(
            "POST",
            "/api/v1/providers/install",
            json!({
                "integration":input, "board_id":board, "widgets":[{"id":"json"}]
            }),
            201,
        )
        .await;
    let board_path = format!("/api/v1/boards/{board}");
    let integration = saved["id"].as_str().unwrap();
    let integration_path = format!("/api/v1/integrations/{integration}");
    let installed = owner.json("GET", &board_path, Value::Null, 200).await;
    assert_eq!(installed["document"]["sections"][0]["title"], "Widgets");

    // Emulate the legacy title and marker brought forward by migration.
    st.db
        .dashboard(|conn| {
            conn.execute(
                "UPDATE boards SET sections=json_set(sections,
            '$[0].title','Former private name','$[0].legacy_provider_title',?2)
            WHERE id=?1",
                (board.as_str(), integration),
            )?;
            conn.execute(
                "UPDATE board_items SET config=json_set(config,'$.title','Private widget title')
            WHERE board_id=?1",
                [&board],
            )?;
            Ok::<_, rusqlite::Error>(())
        })
        .unwrap();
    let shared = owner
        .guest(
            "shared-viewer",
            "viewer",
            json!({"boards":[board], "integrations":[]}),
        )
        .await;
    input["name"] = json!("Current private name");
    owner.json("PUT", &integration_path, input, 200).await;
    let view = shared.json("GET", &board_path, Value::Null, 200).await;
    assert_eq!(view["document"]["sections"][0]["title"], "Widgets");
    assert_eq!(
        view["document"]["sections"][0]["items"][0]["payload"]["title"],
        "Widget unavailable"
    );
    assert!(
        !view.to_string().contains("private name")
            && !view.to_string().contains("Private widget title")
    );
    let mut moved = owner.json("GET", &board_path, Value::Null, 200).await["document"].clone();
    let items = moved["sections"][0]["items"].take();
    moved["sections"][0]["items"] = json!([]);
    moved["sections"].as_array_mut().unwrap().push(json!({
        "title":"More widgets", "position":1, "items":items
    }));
    owner.json("PUT", &board_path, moved, 200).await;
    let view = shared.json("GET", &board_path, Value::Null, 200).await;
    assert_eq!(view["document"]["sections"][0]["title"], "Widgets");
    assert_eq!(
        owner
            .call("DELETE", &integration_path, Value::Null)
            .await
            .status,
        204
    );
    for reader in [&shared, &owner] {
        let view = reader.json("GET", &board_path, Value::Null, 200).await;
        assert_eq!(view["document"]["sections"][0]["title"], "Widgets");
        assert_eq!(
            view["document"]["sections"][1]["items"][0]["payload"]["title"],
            "Widget unavailable"
        );
        assert!(
            !view.to_string().contains("private name")
                && !view.to_string().contains("Private widget title")
        );
    }
}

#[tokio::test]
async fn detected_services_from_the_fake_docker_snapshot() {
    use crate::model::{Container, NetworkAddr, PortMapping};
    let ctr = |name: &str, image: &str, port: Option<(&str, u16, u16)>| Container {
        id: format!("{name:0<64}"),
        name: name.into(),
        image: image.into(),
        state: "running".into(),
        ports: port
            .map(|(host_ip, host_port, container_port)| PortMapping {
                host_ip: host_ip.into(),
                host_port,
                container_port,
                proto: "tcp".into(),
            })
            .into_iter()
            .collect(),
        networks: vec![NetworkAddr {
            name: "media".into(),
            ip: "172.20.0.9".into(),
        }],
        ..Default::default()
    };
    let st = crate::testutil::state_with(
        vec![
            ctr(
                "sonarr",
                "lscr.io/linuxserver/sonarr:latest",
                Some(("192.168.1.20", 8989, 8989)),
            ),
            ctr(
                "pihole",
                "pihole/pihole:latest",
                Some(("192.168.1.20", 8053, 80)),
            ),
            ctr(
                "postgres",
                "postgres:16-alpine",
                Some(("192.168.1.20", 5432, 5432)),
            ),
        ],
        "",
    );
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let found = owner
        .json("GET", "/api/v1/providers/detected", Value::Null, 200)
        .await;
    assert_eq!(
        found,
        json!([
            {"provider":"pihole","container":"pihole","image":"pihole/pihole:latest","matched":"image","port":80,
             "candidates":[{"url":"http://192.168.1.20:8053","via":"published"}]},
            {"provider":"sonarr","container":"sonarr","image":"lscr.io/linuxserver/sonarr:latest","matched":"image","port":8989,
             "candidates":[{"url":"http://192.168.1.20:8989","via":"published"}]},
        ])
    );
    // Connecting Sonarr hides its suggestion; nothing was contacted to find it.
    owner
        .json(
            "POST",
            "/api/v1/integrations",
            json!({"name":"Sonarr","kind":"sonarr","base_url":"http://192.168.1.20:8989","enabled":true,"credentials":{"api_key":"k"}}),
            201,
        )
        .await;
    let found = owner
        .json("GET", "/api/v1/providers/detected", Value::Null, 200)
        .await;
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["provider"], "pihole");
    // The Add picker shows logos: the provider's for an integration, the
    // image's brand logo for a container.
    let board = owner.board("Picker").await;
    let suggestions = owner
        .json(
            "GET",
            &format!("/api/v1/boards/{board}/suggestions"),
            Value::Null,
            200,
        )
        .await;
    let icon = |title: &str| {
        suggestions
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["title"] == title)
            .map(|s| s["icon"].clone())
    };
    assert_eq!(icon("Sonarr"), Some(json!("sonarr")));
    assert_eq!(icon("pihole"), Some(json!("pi-hole")));
    assert_eq!(icon("postgres"), Some(json!("postgres")));
    // Web UIs are offered as branded links on their published address; the
    // database is not.
    let apps: Vec<_> = suggestions
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s.get("detected").is_some())
        .map(|s| (s["detected"].clone(), s["payload"].clone()))
        .collect();
    assert_eq!(
        apps,
        [
            (
                json!("pihole"),
                json!({"kind":"link","title":"Pi-hole","url":"http://192.168.1.20:8053","icon":"pi-hole"})
            ),
            (
                json!("sonarr"),
                json!({"kind":"link","title":"Sonarr","url":"http://192.168.1.20:8989","icon":"sonarr"})
            ),
        ]
    );
    // A scoped admin cannot connect new integrations, so sees no detection.
    let board = owner.board("Scoped").await;
    let admin = owner
        .guest(
            "scoped-admin",
            "admin",
            json!({"boards":[board],"integrations":[]}),
        )
        .await;
    assert_eq!(
        admin
            .call("GET", "/api/v1/providers/detected", Value::Null)
            .await
            .status,
        403
    );
}

#[tokio::test]
async fn hobnail_widgets_install_into_a_chosen_section() {
    let st = state();
    crate::auth::set_password_for_tests(&st.db, PW);
    let owner = Client::owner(start(st).await).await;
    let board = owner.board("Homelab").await;
    let path = format!("/api/v1/boards/{board}");
    let view = owner.json("GET", &path, Value::Null, 200).await;
    let mut doc = view["document"].clone();
    doc["sections"] = json!([
        {"title":"Overview","position":0,"collapsed":false,"items":[]},
        {"title":"Apps","position":1,"collapsed":false,"items":[]}
    ]);
    owner.json("PUT", &path, doc, 200).await;
    let hobnail = json!({"name":"Hobnail","kind":"hobnail","base_url":"","enabled":true,"options":{},"credentials":{}});
    let integration = owner
        .json(
            "POST",
            "/api/v1/providers/install",
            json!({"integration":hobnail,"board_id":board,"section":0,
                   "widgets":[{"id":"host","options":{}},{"id":"history","options":{"metric":"mem","range":"24h"}}]}),
            201,
        )
        .await;
    let doc = owner.json("GET", &path, Value::Null, 200).await["document"].clone();
    let sections = doc["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 2, "no extra Widgets section");
    assert_eq!(sections[0]["title"], "Overview");
    let items = sections[0]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!((&items[0]["w"], &items[0]["h"]), (&json!(4), &json!(4)));
    // The built-in integration is reused, and is not a status-tile suggestion.
    owner
        .json(
            "POST",
            "/api/v1/providers/install",
            json!({"integration":hobnail,"integration_id":integration["id"],"board_id":board,"section":1,
                   "widgets":[{"id":"stacks","options":{}}]}),
            201,
        )
        .await;
    let doc = owner.json("GET", &path, Value::Null, 200).await["document"].clone();
    assert_eq!(doc["sections"][1]["items"].as_array().unwrap().len(), 1);
    let suggestions = owner
        .json("GET", &format!("{path}/suggestions"), Value::Null, 200)
        .await;
    assert!(!suggestions
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["payload"]["kind"] == "integration"));
}
