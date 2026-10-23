use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use arcman::{AppState, Config};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{Request, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const PASSWORD: &str = "correct horse battery";

#[derive(Default)]
struct Upstream {
    statuses: HashMap<String, (u16, Value)>,
    videos: HashMap<String, (u16, Value)>,
    /// Web pages by name: status, content type and body.
    pages: HashMap<String, (u16, &'static str, String)>,
    missing_files: HashSet<String>,
}

type Fake = Arc<Mutex<Upstream>>;

async fn fake_status(State(fake): State<Fake>, Path(id): Path<String>) -> impl IntoResponse {
    let (code, status) = fake
        .lock()
        .unwrap()
        .statuses
        .get(&id)
        .cloned()
        .unwrap_or((404, Value::Null));
    let status_code = StatusCode::from_u16(code).unwrap();
    (status_code, Json(json!({ "code": code, "status": status })))
}

async fn fake_file(State(fake): State<Fake>, Path(name): Path<String>) -> impl IntoResponse {
    if fake.lock().unwrap().missing_files.contains(&name) {
        return (StatusCode::NOT_FOUND, Vec::new());
    }
    (StatusCode::OK, format!("FILE:{name}").into_bytes())
}

async fn fake_oembed(
    State(fake): State<Fake>,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    assert_eq!(q.get("format").map(String::as_str), Some("json"));
    let id = q["url"]
        .strip_prefix("https://www.youtube.com/watch?v=")
        .expect("oEmbed is asked for the watch URL");
    match fake.lock().unwrap().videos.get(id).cloned() {
        Some((200, v)) => (StatusCode::OK, Json(v)).into_response(),
        Some((code, _)) => StatusCode::from_u16(code).unwrap().into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn fake_thumbnail(
    State(fake): State<Fake>,
    Path((id, name)): Path<(String, String)>,
) -> impl IntoResponse {
    fake_file(State(fake), Path(format!("{id}/{name}"))).await
}

async fn fake_page(State(fake): State<Fake>, Path(name): Path<String>) -> impl IntoResponse {
    match fake.lock().unwrap().pages.get(&name).cloned() {
        Some((code, content_type, body)) => (
            StatusCode::from_u16(code).unwrap(),
            [(header::CONTENT_TYPE, content_type)],
            body,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn fake_image(State(fake): State<Fake>, Path(name): Path<String>) -> impl IntoResponse {
    if fake.lock().unwrap().missing_files.contains(&name) {
        return StatusCode::NOT_FOUND.into_response();
    }
    ([(header::CONTENT_TYPE, "image/png")], format!("IMG:{name}")).into_response()
}

async fn spawn_upstream() -> (String, Fake) {
    let fake = Fake::default();
    let router = Router::new()
        .route("/2/status/{id}", get(fake_status))
        .route("/files/{name}", get(fake_file))
        .route("/oembed", get(fake_oembed))
        .route("/vi/{id}/{name}", get(fake_thumbnail))
        .route("/pages/{name}", get(fake_page))
        .route("/images/{name}", get(fake_image))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base, fake)
}

async fn unreachable_base() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    format!("http://{}", listener.local_addr().unwrap())
}

struct TestApp {
    router: Router,
    state: AppState,
    base: String,
    fake: Fake,
    cookie: String,
    _home: TempDir,
}

impl TestApp {
    async fn start() -> Self {
        let (base, fake) = spawn_upstream().await;
        Self::start_with(base, fake, TempDir::new().unwrap(), None).await
    }

    async fn start_with(base: String, fake: Fake, home: TempDir, env_base: Option<String>) -> Self {
        let (state, queue) = AppState::open(Config {
            home: home.path().to_path_buf(),
            fxtwitter_base_url_env: Some(env_base.unwrap_or_else(|| base.clone())),
            youtube_oembed_url: Some(format!("{base}/oembed")),
        })
        .unwrap();
        arcman::auth::set_password(&state.db(), PASSWORD).unwrap();
        let router = arcman::app(state.clone(), queue);
        let mut app = Self {
            router,
            state,
            base,
            fake,
            cookie: String::new(),
            _home: home,
        };
        app.cookie = app.login(PASSWORD).await.1.expect("login succeeds");
        app
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        cookie: &str,
    ) -> (StatusCode, Value, Option<String>) {
        let mut req = Request::builder().method(method).uri(path);
        if !cookie.is_empty() {
            req = req.header(header::COOKIE, cookie);
        }
        let req = match body {
            Some(b) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let set_cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()));
        (status, value, set_cookie)
    }

    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (s, v, _) = self.send(method, path, body, &self.cookie).await;
        (s, v)
    }

    async fn login(&self, password: &str) -> (StatusCode, Option<String>) {
        let (s, _, c) = self
            .send(
                "POST",
                "/api/login",
                Some(json!({ "password": password })),
                "",
            )
            .await;
        (s, c)
    }

    fn upstream(&self, id: &str, status: Value) {
        self.fake
            .lock()
            .unwrap()
            .statuses
            .insert(id.to_string(), (200, status));
    }

    fn video(&self, id: &str, title: &str) {
        let video = json!({
            "title": title,
            "author_name": "Rick Astley",
            "author_url": "https://www.youtube.com/@RickAstleyYT",
            "type": "video",
            "thumbnail_url": format!("{}/vi/{id}/hqdefault.jpg", self.base),
            "thumbnail_width": 480,
            "thumbnail_height": 360
        });
        self.fake
            .lock()
            .unwrap()
            .videos
            .insert(id.to_string(), (200, video));
    }

    fn page(&self, name: &str, code: u16, html: &str) -> String {
        self.fake.lock().unwrap().pages.insert(
            name.to_string(),
            (code, "text/html; charset=utf-8", html.to_string()),
        );
        format!("{}/pages/{name}", self.base)
    }

    fn file(&self, name: &str) -> String {
        format!("{}/files/{name}", self.base)
    }

    fn status(&self, id: &str, text: &str) -> Value {
        json!({
            "type": "status",
            "id": id,
            "url": format!("https://x.com/alice/status/{id}"),
            "text": text,
            "lang": "ja",
            "created_timestamp": 1_700_000_000,
            "author": {"screen_name": "alice", "name": "Alice", "avatar_url": self.file(&format!("{id}-avatar.jpg"))},
            "likes": 3,
            "reposts": 1,
            "media": {}
        })
    }

    fn with_photos(&self, mut status: Value, n: usize) -> Value {
        let id = status["id"].as_str().unwrap().to_string();
        status["media"]["all"] = (0..n)
            .map(|i| json!({"type": "photo", "url": self.file(&format!("{id}-p{i}.jpg")), "width": 400, "height": 300}))
            .collect();
        status
    }

    fn with_video(&self, mut status: Value, kind: &str) -> Value {
        let id = status["id"].as_str().unwrap().to_string();
        status["media"]["all"] = json!([{
            "type": kind,
            "url": self.file(&format!("{id}-default.mp4")),
            "thumbnail_url": self.file(&format!("{id}-thumb.jpg")),
            "width": 1280, "height": 720,
            "formats": [
                {"url": self.file(&format!("{id}-low.mp4")), "container": "mp4", "bitrate": 256000},
                {"url": self.file(&format!("{id}-high.mp4")), "container": "mp4", "bitrate": 2176000}
            ]
        }]);
        status
    }

    /// Saves one URL and waits until its media archive finishes.
    async fn save(&self, url: &str, folder_id: Option<i64>) -> i64 {
        let (s, v) = self
            .call(
                "POST",
                "/api/posts",
                Some(json!({ "urls": [url], "folder_id": folder_id })),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v[0]["outcome"], "created", "{v}");
        let id = v[0]["post_id"].as_i64().unwrap();
        self.wait_until_settled(id).await;
        id
    }

    async fn wait_until_settled(&self, id: i64) -> Value {
        for _ in 0..200 {
            let (_, d) = self.call("GET", &format!("/api/posts/{id}"), None).await;
            if d["state"] != "pending" {
                return d;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("post {id} stayed pending");
    }

    async fn tiles(&self, query: &str) -> Value {
        let (s, v) = self.call("GET", &format!("/api/tiles?{query}"), None).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }

    async fn tile_post_ids(&self, query: &str) -> Vec<i64> {
        let mut ids: Vec<i64> = self.tiles(query).await["tiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["post_id"].as_i64().unwrap())
            .collect();
        ids.dedup();
        ids
    }

    async fn create(&self, path: &str, body: Value) -> i64 {
        let (s, v) = self.call("POST", path, Some(body)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v["id"].as_i64().unwrap()
    }
}

#[tokio::test]
async fn protected_endpoints_require_a_session() {
    let app = TestApp::start().await;
    for (method, path) in [
        ("GET", "/api/tiles"),
        ("POST", "/api/posts"),
        ("GET", "/api/sidebar"),
        ("GET", "/media/1/main-0.jpg"),
    ] {
        let (s, _, _) = app.send(method, path, None, "").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{method} {path}");
        let (s, _, _) = app.send(method, path, None, "arcman_session=forged").await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{method} {path}");
    }
}

#[tokio::test]
async fn logging_in_issues_an_http_only_session_cookie_and_logging_out_revokes_it() {
    let app = TestApp::start().await;
    let (s, _, _) = app.send("GET", "/api/session", None, &app.cookie).await;
    assert_eq!(s, StatusCode::NO_CONTENT);

    let req = Request::post("/api/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "password": PASSWORD }).to_string()))
        .unwrap();
    let res = app.router.clone().oneshot(req).await.unwrap();
    let set_cookie = res.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(
        set_cookie.contains("HttpOnly") && set_cookie.contains("SameSite=Strict"),
        "{set_cookie}"
    );
    assert!(!set_cookie.contains("Secure"));

    let (s, _, _) = app.send("POST", "/api/logout", None, &app.cookie).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = app.send("GET", "/api/session", None, &app.cookie).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_cookie_is_marked_secure_behind_an_https_proxy() {
    let app = TestApp::start().await;
    let req = Request::post("/api/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-proto", "https")
        .body(Body::from(json!({ "password": PASSWORD }).to_string()))
        .unwrap();
    let res = app.router.clone().oneshot(req).await.unwrap();
    assert!(
        res.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("; Secure")
    );
}

#[tokio::test]
async fn logging_out_everywhere_revokes_every_session() {
    let app = TestApp::start().await;
    let other = app.login(PASSWORD).await.1.unwrap();
    let (s, _, _) = app.send("POST", "/api/logout-all", None, &app.cookie).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = app.send("GET", "/api/session", None, &other).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn repeated_wrong_passwords_lock_out_even_the_right_one() {
    let app = TestApp::start().await;
    for _ in 0..5 {
        assert_eq!(app.login("wrong").await.0, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(app.login(PASSWORD).await.0, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn saving_a_post_archives_its_media_locally_and_shows_one_tile_per_image() {
    let app = TestApp::start().await;
    app.upstream("100", app.with_photos(app.status("100", "two photos"), 2));
    let id = app.save("https://x.com/alice/status/100?s=20", None).await;

    let page = app.tiles("").await;
    let tiles = page["tiles"].as_array().unwrap();
    assert_eq!(tiles.len(), 2);
    for (i, t) in tiles.iter().enumerate() {
        assert_eq!(t["post_id"], id);
        assert_eq!(t["kind"], "photo");
        assert_eq!(t["index"], i);
        assert_eq!(t["count"], 2);
        assert_eq!(t["width"], 400);
        assert_eq!(t["height"], 300);
    }
    let src = tiles[1]["src"].as_str().unwrap();
    let (s, body) = app.call("GET", src, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body, "FILE:100-p1.jpg");
}

#[tokio::test]
async fn videos_keep_only_the_highest_bitrate_mp4_with_a_thumbnail() {
    let app = TestApp::start().await;
    app.upstream("101", app.with_video(app.status("101", "video"), "video"));
    app.save("https://x.com/alice/status/101", None).await;

    let tile = &app.tiles("").await["tiles"][0];
    assert_eq!(tile["kind"], "video");
    assert_eq!(
        app.call("GET", tile["src"].as_str().unwrap(), None).await.1,
        "FILE:101-high.mp4"
    );
    assert_eq!(
        app.call("GET", tile["thumbnail"].as_str().unwrap(), None)
            .await
            .1,
        "FILE:101-thumb.jpg"
    );
}

#[tokio::test]
async fn a_post_without_media_becomes_a_text_tile() {
    let app = TestApp::start().await;
    let mut status = app.status("102", "just text");
    status["card"] = json!({"url": "https://n.example", "title": "News", "image": {"url": app.file("102-card.jpg")}});
    app.upstream("102", status);
    app.save("https://x.com/alice/status/102", None).await;

    let tiles = app.tiles("").await["tiles"].clone();
    assert_eq!(tiles.as_array().unwrap().len(), 1);
    assert_eq!(tiles[0]["kind"], "text");
    assert_eq!(tiles[0]["text"], "just text");
    assert_eq!(tiles[0]["author_name"], "Alice");
}

#[tokio::test]
async fn saving_the_same_post_from_another_url_form_points_to_the_existing_one() {
    let app = TestApp::start().await;
    app.upstream("103", app.status("103", "once"));
    let id = app.save("https://x.com/alice/status/103", None).await;
    let (_, v) = app
        .call("POST", "/api/posts", Some(json!({ "urls": ["https://fixupx.com/alice/status/103", "https://twitter.com/alice/status/103"] })))
        .await;
    assert_eq!(v[0]["outcome"], "duplicate");
    assert_eq!(v[0]["post_id"], id);
    assert_eq!(v[1]["outcome"], "duplicate");
    assert_eq!(app.tile_post_ids("").await, vec![id]);
}

#[tokio::test]
async fn several_urls_are_saved_in_one_request_with_a_result_per_line() {
    let app = TestApp::start().await;
    app.upstream("104", app.status("104", "a"));
    app.upstream("105", app.status("105", "b"));
    let (_, v) = app
        .call(
            "POST",
            "/api/posts",
            Some(json!({ "urls": ["https://x.com/a/status/104", "", "example.com/no-scheme", "https://x.com/a/status/105"] })),
        )
        .await;
    let outcomes: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["outcome"].as_str().unwrap())
        .collect();
    assert_eq!(outcomes, ["created", "invalid_url", "created"]);
}

#[tokio::test]
async fn deleted_or_private_posts_are_rejected_without_being_saved() {
    let app = TestApp::start().await;
    app.fake
        .lock()
        .unwrap()
        .statuses
        .insert("107".into(), (401, Value::Null));
    let (_, v) = app
        .call(
            "POST",
            "/api/posts",
            Some(json!({ "urls": ["https://x.com/a/status/106", "https://x.com/a/status/107"] })),
        )
        .await;
    assert_eq!(v[0]["outcome"], "unavailable");
    assert_eq!(v[0]["reason"], "not_found");
    assert_eq!(v[1]["reason"], "private");
    assert!(app.tiles("").await["tiles"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn an_unreachable_api_leaves_a_failed_post_that_can_be_retried() {
    let (base, fake) = spawn_upstream().await;
    let app = TestApp::start_with(
        base.clone(),
        fake,
        TempDir::new().unwrap(),
        Some(unreachable_base().await),
    )
    .await;
    app.upstream("108", app.with_photos(app.status("108", "later"), 1));

    let (_, v) = app
        .call(
            "POST",
            "/api/posts",
            Some(json!({ "urls": ["https://x.com/a/status/108"] })),
        )
        .await;
    assert_eq!(v[0]["outcome"], "created");
    let id = v[0]["post_id"].as_i64().unwrap();
    let tile = &app.tiles("").await["tiles"][0];
    assert_eq!(tile["kind"], "failed");
    assert_eq!(tile["failure"], "api_unavailable");

    let (s, _) = app
        .call("POST", &format!("/api/posts/{id}/retry"), None)
        .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    app.call(
        "PUT",
        "/api/settings",
        Some(json!({ "fxtwitter_base_url": base })),
    )
    .await;
    let (s, _) = app
        .call("POST", &format!("/api/posts/{id}/retry"), None)
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    assert_eq!(app.wait_until_settled(id).await["state"], "done");
    assert_eq!(app.tiles("").await["tiles"][0]["kind"], "photo");
}

#[tokio::test]
async fn progress_reports_the_state_of_each_known_post() {
    let app = TestApp::start().await;
    app.upstream("110", app.with_photos(app.status("110", "one"), 1));
    let id = app.save("https://x.com/a/status/110", None).await;

    let (s, v) = app
        .call("GET", &format!("/api/progress?ids={id},999"), None)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v,
        json!([{ "post_id": id, "state": "done", "progress": null }])
    );
}

#[tokio::test]
async fn a_failed_media_download_fails_the_post_until_retried() {
    let app = TestApp::start().await;
    app.upstream("109", app.with_photos(app.status("109", "flaky"), 1));
    app.fake
        .lock()
        .unwrap()
        .missing_files
        .insert("109-p0.jpg".into());
    let (_, v) = app
        .call(
            "POST",
            "/api/posts",
            Some(json!({ "urls": ["https://x.com/a/status/109"] })),
        )
        .await;
    let id = v[0]["post_id"].as_i64().unwrap();
    let d = app.wait_until_settled(id).await;
    assert_eq!(d["state"], "failed");
    assert_eq!(d["failure"], "media_download");

    app.fake.lock().unwrap().missing_files.clear();
    app.call("POST", &format!("/api/posts/{id}/retry"), None)
        .await;
    assert_eq!(app.wait_until_settled(id).await["state"], "done");
}

#[tokio::test]
async fn retrying_a_post_that_did_not_fail_is_rejected() {
    let app = TestApp::start().await;
    app.upstream("110", app.status("110", "fine"));
    let id = app.save("https://x.com/a/status/110", None).await;
    let (s, _) = app
        .call("POST", &format!("/api/posts/{id}/retry"), None)
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_detail_view_serves_snapshot_fields_with_local_media_urls() {
    let app = TestApp::start().await;
    let mut quoted = app.with_photos(app.status("111", "quoted text"), 1);
    quoted["author"]["screen_name"] = json!("bob");
    let mut status = app.with_photos(app.status("112", "hello"), 1);
    status["lang"] = json!("en");
    status["translation"] = json!({"text": "こんにちは", "source_lang": "en"});
    status["quote"] = quoted;
    status["card"] = json!({"url": "https://n.example", "title": "News", "domain": "n.example", "image": {"url": app.file("112-card.jpg")}});
    status["poll"] =
        json!({"choices": [{"label": "yes", "count": 2, "percentage": 100}], "total_votes": 2});
    status["community_note"] = json!({"text": "context"});
    app.upstream("112", status);
    let id = app.save("https://x.com/alice/status/112", None).await;

    let (_, d) = app.call("GET", &format!("/api/posts/{id}"), None).await;
    let s = &d["status"];
    assert_eq!(s["text"], "hello");
    assert_eq!(s["translation"]["text"], "こんにちは");
    assert_eq!(s["stats"]["likes"], 3);
    assert_eq!(s["posted_at"], 1_700_000_000);
    assert_eq!(s["poll"]["total_votes"], 2);
    assert_eq!(s["community_note"]["text"], "context");
    assert_eq!(s["card"]["title"], "News");
    assert_eq!(s["quote"]["available"], true);
    assert_eq!(s["quote"]["status"]["author"]["screen_name"], "bob");
    for (path, file) in [
        (&s["author"]["avatar"], "112-avatar.jpg"),
        (&s["media"][0]["src"], "112-p0.jpg"),
        (&s["card"]["image"], "112-card.jpg"),
        (&s["quote"]["status"]["media"][0]["src"], "111-p0.jpg"),
        (&s["quote"]["status"]["author"]["avatar"], "111-avatar.jpg"),
    ] {
        let url = path
            .as_str()
            .unwrap_or_else(|| panic!("missing local url for {file}"));
        assert!(url.starts_with(&format!("/media/{id}/")), "{url}");
        assert_eq!(app.call("GET", url, None).await.1, format!("FILE:{file}"));
    }
}

#[tokio::test]
async fn an_unavailable_quote_still_saves_the_post_and_explains_why() {
    let app = TestApp::start().await;
    let mut status = app.status("113", "quoting a deleted post");
    status["quote"] =
        json!({"type": "tombstone", "reason": "deleted", "message": "This post was deleted"});
    app.upstream("113", status);
    let id = app.save("https://x.com/alice/status/113", None).await;

    let (_, d) = app.call("GET", &format!("/api/posts/{id}"), None).await;
    assert_eq!(d["state"], "done");
    assert_eq!(d["status"]["quote"]["available"], false);
    assert_eq!(d["status"]["quote"]["message"], "This post was deleted");
}

#[tokio::test]
async fn article_content_is_returned_as_saved_without_downloading_outbound_links() {
    let app = TestApp::start().await;
    let mut status = app.status("114", "article");
    status["article"] = json!({"title": "Long read", "content": {"blocks": [{"text": "see https://example.com/a.jpg"}]}});
    app.upstream("114", status);
    let id = app.save("https://x.com/alice/status/114", None).await;
    let (_, d) = app.call("GET", &format!("/api/posts/{id}"), None).await;
    let article = &d["status"]["article"];
    assert_eq!(d["state"], "done");
    assert_eq!(article["content"]["title"], "Long read");
    assert_eq!(article["local_urls"], json!({}));
}

#[tokio::test]
async fn deleting_a_post_removes_its_media_files() {
    let app = TestApp::start().await;
    app.upstream("115", app.with_photos(app.status("115", "bye"), 1));
    let id = app.save("https://x.com/a/status/115", None).await;
    let dir = app.state.media_dir.join(id.to_string());
    assert!(dir.exists());
    let (s, _) = app.call("DELETE", &format!("/api/posts/{id}"), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(!dir.exists());
    assert_eq!(
        app.call("GET", &format!("/api/posts/{id}"), None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn posts_go_into_the_given_folder_and_can_be_moved_or_unsorted() {
    let app = TestApp::start().await;
    let folder = app.create("/api/folders", json!({ "name": "Cats" })).await;
    app.upstream("116", app.status("116", "in folder"));
    app.upstream("117", app.status("117", "unsorted"));
    let a = app.save("https://x.com/a/status/116", Some(folder)).await;
    let b = app.save("https://x.com/a/status/117", None).await;

    assert_eq!(
        app.tile_post_ids(&format!("folder={folder}")).await,
        vec![a]
    );
    assert_eq!(app.tile_post_ids("folder=unsorted").await, vec![b]);

    app.call(
        "PATCH",
        &format!("/api/posts/{b}"),
        Some(json!({ "folder_id": folder })),
    )
    .await;
    app.call(
        "PATCH",
        &format!("/api/posts/{a}"),
        Some(json!({ "folder_id": null })),
    )
    .await;
    assert_eq!(
        app.tile_post_ids(&format!("folder={folder}")).await,
        vec![b]
    );
    assert_eq!(app.tile_post_ids("folder=unsorted").await, vec![a]);

    let (s, v) = app
        .call(
            "PATCH",
            &format!("/api/posts/{a}"),
            Some(json!({ "folder_id": 999 })),
        )
        .await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("unknown_folder"))
    );
}

#[tokio::test]
async fn deleting_a_folder_deletes_its_posts_and_their_files() {
    let app = TestApp::start().await;
    let folder = app
        .create("/api/folders", json!({ "name": "Trash me" }))
        .await;
    app.upstream("118", app.with_photos(app.status("118", "x"), 1));
    app.upstream("119", app.status("119", "kept"));
    let doomed = app.save("https://x.com/a/status/118", Some(folder)).await;
    let kept = app.save("https://x.com/a/status/119", None).await;

    let (_, f) = app
        .call("GET", &format!("/api/folders/{folder}"), None)
        .await;
    assert_eq!(f["post_count"], 1);
    app.call("DELETE", &format!("/api/folders/{folder}"), None)
        .await;
    assert_eq!(app.tile_post_ids("").await, vec![kept]);
    assert!(!app.state.media_dir.join(doomed.to_string()).exists());
}

#[tokio::test]
async fn deleting_a_category_keeps_its_folders_uncategorized() {
    let app = TestApp::start().await;
    let cat = app
        .create("/api/categories", json!({ "name": "Hobby" }))
        .await;
    let folder = app
        .create(
            "/api/folders",
            json!({ "name": "Travel", "category_id": cat }),
        )
        .await;
    let (_, sb) = app.call("GET", "/api/sidebar", None).await;
    assert_eq!(sb["categories"][0]["folders"][0]["id"], folder);

    app.call("DELETE", &format!("/api/categories/{cat}"), None)
        .await;
    let (_, sb) = app.call("GET", "/api/sidebar", None).await;
    assert!(sb["categories"].as_array().unwrap().is_empty());
    assert_eq!(sb["uncategorized"][0]["id"], folder);
}

#[tokio::test]
async fn folders_can_share_names_and_be_reordered_into_another_category() {
    let app = TestApp::start().await;
    let hobby = app
        .create("/api/categories", json!({ "name": "Hobby" }))
        .await;
    let work = app
        .create("/api/categories", json!({ "name": "Work" }))
        .await;
    let a = app
        .create(
            "/api/folders",
            json!({ "name": "Same", "category_id": hobby }),
        )
        .await;
    let b = app
        .create(
            "/api/folders",
            json!({ "name": "Same", "category_id": hobby }),
        )
        .await;
    let c = app
        .create(
            "/api/folders",
            json!({ "name": "Docs", "category_id": work }),
        )
        .await;

    app.call(
        "PUT",
        "/api/folders/order",
        Some(json!({ "category_id": work, "ids": [b, c] })),
    )
    .await;
    app.call(
        "PUT",
        "/api/categories/order",
        Some(json!({ "ids": [work, hobby] })),
    )
    .await;
    let (_, sb) = app.call("GET", "/api/sidebar", None).await;
    let ids = |v: &Value| {
        v["folders"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_i64().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(sb["categories"][0]["id"], work);
    assert_eq!(ids(&sb["categories"][0]), vec![b, c]);
    assert_eq!(ids(&sb["categories"][1]), vec![a]);
}

#[tokio::test]
async fn blank_names_are_rejected() {
    let app = TestApp::start().await;
    let (s, _) = app
        .call("POST", "/api/folders", Some(json!({ "name": "  " })))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = app
        .call("POST", "/api/categories", Some(json!({ "name": "" })))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tags_are_created_on_the_fly_and_spelling_variants_resolve_to_one_tag() {
    let app = TestApp::start().await;
    app.upstream("120", app.status("120", "a"));
    app.upstream("121", app.status("121", "b"));
    let a = app.save("https://x.com/a/status/120", None).await;
    let b = app.save("https://x.com/a/status/121", None).await;

    let (_, tags) = app
        .call(
            "PUT",
            &format!("/api/posts/{a}/tags"),
            Some(json!({ "names": ["Rust", "#rust", "Cats"] })),
        )
        .await;
    let names: Vec<&str> = tags
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Cats", "Rust"]);
    app.call(
        "PUT",
        &format!("/api/posts/{b}/tags"),
        Some(json!({ "names": ["ＲＵＳＴ"] })),
    )
    .await;

    let (_, list) = app.call("GET", "/api/tags?sort=count", None).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[0]["name"], "Rust");
    assert_eq!(list[0]["count"], 2);
    assert_eq!(list[1]["count"], 1);
}

#[tokio::test]
async fn tags_can_be_renamed_recolored_and_deleted_without_touching_posts() {
    let app = TestApp::start().await;
    app.upstream("122", app.status("122", "a"));
    let post = app.save("https://x.com/a/status/122", None).await;
    let (_, tags) = app
        .call(
            "PUT",
            &format!("/api/posts/{post}/tags"),
            Some(json!({ "names": ["cat", "dog"] })),
        )
        .await;
    let (cat, dog) = (
        tags[0]["id"].as_i64().unwrap(),
        tags[1]["id"].as_i64().unwrap(),
    );

    let (s, v) = app
        .call(
            "PATCH",
            &format!("/api/tags/{cat}"),
            Some(json!({ "name": "#DOG" })),
        )
        .await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::CONFLICT, Some("tag_exists"))
    );
    let (s, _) = app
        .call(
            "PATCH",
            &format!("/api/tags/{cat}"),
            Some(json!({ "color": "red" })),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    app.call(
        "PATCH",
        &format!("/api/tags/{cat}"),
        Some(json!({ "name": "Kitten", "color": "#ff8800" })),
    )
    .await;
    app.call("DELETE", &format!("/api/tags/{dog}"), None).await;

    let (_, d) = app.call("GET", &format!("/api/posts/{post}"), None).await;
    assert_eq!(
        d["tags"],
        json!([{ "id": cat, "name": "Kitten", "color": "#ff8800" }])
    );
    app.call(
        "PATCH",
        &format!("/api/tags/{cat}"),
        Some(json!({ "color": null })),
    )
    .await;
    let (_, list) = app.call("GET", "/api/tags", None).await;
    assert_eq!(list[0]["color"], Value::Null);
}

#[tokio::test]
async fn keyword_search_matches_text_or_tag_names_within_the_open_folder() {
    let app = TestApp::start().await;
    let folder = app.create("/api/folders", json!({ "name": "F" })).await;
    for (id, text) in [
        ("130", "a sleepy cat"),
        ("131", "a dog"),
        ("132", "another cat"),
    ] {
        app.upstream(id, app.status(id, text));
    }
    let a = app.save("https://x.com/a/status/130", Some(folder)).await;
    let b = app.save("https://x.com/a/status/131", Some(folder)).await;
    let c = app.save("https://x.com/a/status/132", None).await;
    app.call(
        "PUT",
        &format!("/api/posts/{b}/tags"),
        Some(json!({ "names": ["Catlike"] })),
    )
    .await;

    let mut found = app.tile_post_ids("q=cat").await;
    found.sort();
    assert_eq!(found, vec![a, b, c]);
    let mut in_folder = app.tile_post_ids(&format!("q=CAT&folder={folder}")).await;
    in_folder.sort();
    assert_eq!(in_folder, vec![a, b]);
    assert!(app.tile_post_ids("q=100%25").await.is_empty());
}

#[tokio::test]
async fn selecting_several_tags_shows_only_posts_with_all_of_them() {
    let app = TestApp::start().await;
    app.upstream("133", app.status("133", "a"));
    app.upstream("134", app.status("134", "b"));
    let both = app.save("https://x.com/a/status/133", None).await;
    let one = app.save("https://x.com/a/status/134", None).await;
    let (_, t) = app
        .call(
            "PUT",
            &format!("/api/posts/{both}/tags"),
            Some(json!({ "names": ["x", "y"] })),
        )
        .await;
    app.call(
        "PUT",
        &format!("/api/posts/{one}/tags"),
        Some(json!({ "names": ["x"] })),
    )
    .await;
    let (x, y) = (t[0]["id"].as_i64().unwrap(), t[1]["id"].as_i64().unwrap());

    let mut only_x = app.tile_post_ids(&format!("tags={x}")).await;
    only_x.sort();
    assert_eq!(only_x, vec![both, one]);
    assert_eq!(
        app.tile_post_ids(&format!("tags={x},{y}")).await,
        vec![both]
    );
}

#[tokio::test]
async fn tiles_filter_by_media_type_and_author() {
    let app = TestApp::start().await;
    app.upstream("140", app.with_photos(app.status("140", "photo"), 1));
    app.upstream("141", app.with_video(app.status("141", "gif"), "gif"));
    let mut text = app.status("142", "text");
    text["author"]["screen_name"] = json!("Bob");
    app.upstream("142", text);
    let photo = app.save("https://x.com/a/status/140", None).await;
    let gif = app.save("https://x.com/a/status/141", None).await;
    let text = app.save("https://x.com/a/status/142", None).await;

    assert_eq!(app.tile_post_ids("media=photo").await, vec![photo]);
    assert_eq!(app.tile_post_ids("media=video").await, vec![gif]);
    assert_eq!(app.tile_post_ids("media=text").await, vec![text]);
    assert_eq!(app.tile_post_ids("author=bob").await, vec![text]);

    let (_, authors) = app.call("GET", "/api/authors", None).await;
    assert_eq!(
        authors[0],
        json!({ "screen_name": "alice", "name": "Alice", "count": 2 })
    );
}

#[tokio::test]
async fn tiles_filter_by_saved_date_range_and_sort_both_ways() {
    let app = TestApp::start().await;
    let mut ids = vec![];
    for (id, saved_at) in [
        ("150", "2026-09-01T00:00:00.000Z"),
        ("151", "2026-09-15T00:00:00.000Z"),
        ("152", "2026-10-01T00:00:00.000Z"),
    ] {
        app.upstream(id, app.status(id, id));
        let post = app
            .save(&format!("https://x.com/a/status/{id}"), None)
            .await;
        app.state
            .db()
            .execute(
                "UPDATE posts SET saved_at = ?1 WHERE id = ?2",
                rusqlite::params![saved_at, post],
            )
            .unwrap();
        ids.push(post);
    }
    assert_eq!(app.tile_post_ids("").await, vec![ids[2], ids[1], ids[0]]);
    assert_eq!(app.tile_post_ids("order=asc").await, ids);
    assert_eq!(
        app.tile_post_ids("from=2026-09-10T00:00:00%2B09:00&to=2026-10-01T00:00:00Z")
            .await,
        vec![ids[1]]
    );
}

#[tokio::test]
async fn pages_never_split_a_post_and_the_cursor_continues_where_the_last_page_ended() {
    let app = TestApp::start().await;
    let mut ids = vec![];
    for id in ["160", "161", "162"] {
        app.upstream(id, app.with_photos(app.status(id, id), 2));
        ids.push(
            app.save(&format!("https://x.com/a/status/{id}"), None)
                .await,
        );
    }
    let first = app.tiles("limit=2").await;
    assert_eq!(first["tiles"].as_array().unwrap().len(), 4);
    let cursor = first["next_cursor"]
        .as_str()
        .unwrap()
        .replace('|', "%7C")
        .replace('+', "%2B");
    let second = app.tiles(&format!("limit=2&cursor={cursor}")).await;
    assert_eq!(second["tiles"].as_array().unwrap().len(), 2);
    assert_eq!(second["tiles"][0]["post_id"], ids[0]);
    assert_eq!(second["next_cursor"], Value::Null);
}

#[tokio::test]
async fn settings_prefer_the_saved_value_then_the_environment() {
    let app = TestApp::start().await;
    let (_, v) = app.call("GET", "/api/settings", None).await;
    assert_eq!(
        v["fxtwitter_base_url"],
        json!({ "value": app.base, "source": "env" })
    );

    let (s, _) = app
        .call(
            "PUT",
            "/api/settings",
            Some(json!({ "fxtwitter_base_url": "not a url" })),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    app.call(
        "PUT",
        "/api/settings",
        Some(json!({ "fxtwitter_base_url": "https://fx.example/" })),
    )
    .await;
    let (_, v) = app.call("GET", "/api/settings", None).await;
    assert_eq!(
        v["fxtwitter_base_url"],
        json!({ "value": "https://fx.example", "source": "saved" })
    );
}

#[tokio::test]
async fn posts_left_pending_by_a_restart_are_archived_on_startup() {
    let (base, fake) = spawn_upstream().await;
    let home = TempDir::new().unwrap();
    let path = home.path().to_path_buf();
    {
        let conn = arcman::db::open(&path.join("arcman.db")).unwrap();
        let app_for_fixture =
            TestApp::start_with(base.clone(), fake.clone(), TempDir::new().unwrap(), None).await;
        let snapshot = app_for_fixture.with_photos(app_for_fixture.status("170", "resume me"), 1);
        conn.execute(
            "INSERT INTO posts (status_id, source_url, state, snapshot) VALUES ('170', 'https://x.com/a/status/170', 'pending', ?1)",
            [snapshot.to_string()],
        )
        .unwrap();
    }
    let app = TestApp::start_with(base, fake, home, None).await;
    let id: i64 = app
        .state
        .db()
        .query_row("SELECT id FROM posts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(app.wait_until_settled(id).await["state"], "done");
}

#[tokio::test]
#[ignore = "hits the real fxtwitter API and x.com media servers"]
async fn live_fxtwitter_photo_video_and_gif_posts_are_archived() {
    let (_, fake) = spawn_upstream().await;
    let base = arcman::settings::DEFAULT_FXTWITTER_BASE_URL.to_string();
    let app = TestApp::start_with(base.clone(), fake, TempDir::new().unwrap(), Some(base)).await;
    for (url, kind) in [
        (
            "https://x.com/TheEllenShow/status/440322224407314432",
            "photo",
        ),
        ("https://x.com/NASA/status/2112628310702272570", "video"),
        ("https://x.com/giphy/status/1957913446118285360", "gif"),
    ] {
        let id = app.save(url, None).await;
        let (_, d) = app.call("GET", &format!("/api/posts/{id}"), None).await;
        assert_eq!(d["state"], "done", "{url}: {d}");
        assert_eq!(d["status"]["media"][0]["kind"], kind, "{url}");
        let file = app.state.media_dir.join(id.to_string()).join(format!(
            "main-0.{}",
            if kind == "photo" { "jpg" } else { "mp4" }
        ));
        assert!(
            std::fs::metadata(&file).unwrap().len() > 10_000,
            "{}",
            file.display()
        );
    }
}

#[tokio::test]
async fn unknown_api_paths_answer_404_instead_of_the_app_page() {
    let app = TestApp::start().await;
    let (s, v) = app.call("GET", "/api/nope", None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(v["error"], "not_found");
}

#[tokio::test]
async fn video_tiles_and_details_carry_the_duration() {
    let app = TestApp::start().await;
    let mut status = app.with_video(app.status("180", "clip"), "video");
    status["media"]["all"][0]["duration"] = json!(24.666);
    app.upstream("180", status);
    let id = app.save("https://x.com/a/status/180", None).await;
    assert_eq!(app.tiles("").await["tiles"][0]["duration"], 24.666);
    let (_, d) = app.call("GET", &format!("/api/posts/{id}"), None).await;
    assert_eq!(d["status"]["media"][0]["duration"], 24.666);
}

#[tokio::test]
async fn a_youtube_video_is_saved_with_its_largest_thumbnail_as_one_tile() {
    let app = TestApp::start().await;
    app.video("dQw4w9WgXcQ", "Never Gonna Give You Up");
    let id = app
        .save("https://youtu.be/dQw4w9WgXcQ?si=share", None)
        .await;

    let tiles = app.tiles("").await["tiles"].clone();
    assert_eq!(tiles.as_array().unwrap().len(), 1);
    let tile = &tiles[0];
    assert_eq!(tile["kind"], "youtube");
    assert_eq!(tile["text"], "Never Gonna Give You Up");
    assert_eq!(tile["author_name"], "Rick Astley");
    assert_eq!(
        (tile["width"].clone(), tile["height"].clone()),
        (json!(16), json!(9))
    );
    assert_eq!(
        app.call("GET", tile["src"].as_str().unwrap(), None).await.1,
        "FILE:dQw4w9WgXcQ/maxresdefault.jpg"
    );

    let (_, d) = app.call("GET", &format!("/api/posts/{id}"), None).await;
    assert_eq!(d["source"], "youtube");
    assert!(d["status"].is_null());
    assert_eq!(
        d["video"],
        json!({
            "id": "dQw4w9WgXcQ",
            "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "shorts": false,
            "title": "Never Gonna Give You Up",
            "channel": {"name": "Rick Astley", "url": "https://www.youtube.com/@RickAstleyYT"},
            "thumbnail": tile["src"]
        })
    );

    let (_, authors) = app.call("GET", "/api/authors", None).await;
    assert_eq!(authors[0]["screen_name"], "RickAstleyYT");
    assert_eq!(app.tile_post_ids("q=gonna").await, vec![id]);
}

#[tokio::test]
async fn a_video_without_a_large_thumbnail_falls_back_to_the_oembed_one() {
    let app = TestApp::start().await;
    app.video("W8VZmVf4d4M", "a short");
    app.fake
        .lock()
        .unwrap()
        .missing_files
        .insert("W8VZmVf4d4M/maxresdefault.jpg".into());
    let id = app
        .save("https://www.youtube.com/shorts/W8VZmVf4d4M", None)
        .await;

    let tile = app.tiles("").await["tiles"][0].clone();
    assert_eq!(
        (tile["width"].clone(), tile["height"].clone()),
        (json!(9), json!(16))
    );
    assert_eq!(
        app.call("GET", tile["src"].as_str().unwrap(), None).await.1,
        "FILE:W8VZmVf4d4M/hqdefault.jpg"
    );
    let (_, d) = app.call("GET", &format!("/api/posts/{id}"), None).await;
    assert_eq!(d["video"]["shorts"], true);
    assert_eq!(
        d["video"]["url"],
        "https://www.youtube.com/shorts/W8VZmVf4d4M"
    );
}

#[tokio::test]
async fn the_same_video_from_another_url_form_points_to_the_existing_one() {
    let app = TestApp::start().await;
    app.video("dQw4w9WgXcQ", "once");
    let id = app
        .save("https://www.youtube.com/watch?v=dQw4w9WgXcQ", None)
        .await;
    let (_, v) = app
        .call(
            "POST",
            "/api/posts",
            Some(json!({ "urls": ["https://youtu.be/dQw4w9WgXcQ", "https://m.youtube.com/watch?v=dQw4w9WgXcQ&t=1"] })),
        )
        .await;
    assert_eq!(
        v[0],
        json!({"url": "https://youtu.be/dQw4w9WgXcQ", "outcome": "duplicate", "post_id": id})
    );
    assert_eq!(v[1]["outcome"], "duplicate");
}

#[tokio::test]
async fn missing_or_private_videos_are_rejected_without_being_saved() {
    let app = TestApp::start().await;
    app.fake
        .lock()
        .unwrap()
        .videos
        .insert("privatevid1".into(), (401, Value::Null));
    let (_, v) = app
        .call(
            "POST",
            "/api/posts",
            Some(
                json!({ "urls": ["https://youtu.be/missingvid1", "https://youtu.be/privatevid1"] }),
            ),
        )
        .await;
    assert_eq!(v[0]["reason"], "not_found");
    assert_eq!(v[1]["reason"], "private");
    assert!(app.tiles("").await["tiles"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn an_unreachable_oembed_leaves_a_failed_video_that_can_be_retried() {
    let app = TestApp::start().await;
    app.fake
        .lock()
        .unwrap()
        .videos
        .insert("dQw4w9WgXcQ".into(), (503, Value::Null));
    let (_, v) = app
        .call(
            "POST",
            "/api/posts",
            Some(json!({ "urls": ["https://youtu.be/dQw4w9WgXcQ"] })),
        )
        .await;
    let id = v[0]["post_id"].as_i64().unwrap();
    assert_eq!(app.tiles("").await["tiles"][0]["kind"], "failed");

    app.video("dQw4w9WgXcQ", "back");
    let (s, _) = app
        .call("POST", &format!("/api/posts/{id}/retry"), None)
        .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    assert_eq!(app.wait_until_settled(id).await["video"]["title"], "back");
}

#[tokio::test]
async fn youtube_videos_have_their_own_media_type() {
    let app = TestApp::start().await;
    app.upstream("150", app.with_photos(app.status("150", "photo"), 1));
    app.upstream("151", app.status("151", "text"));
    app.video("dQw4w9WgXcQ", "video");
    let photo = app.save("https://x.com/a/status/150", None).await;
    let text = app.save("https://x.com/a/status/151", None).await;
    let video = app.save("https://youtu.be/dQw4w9WgXcQ", None).await;

    assert_eq!(app.tile_post_ids("media=youtube").await, vec![video]);
    assert_eq!(app.tile_post_ids("media=photo").await, vec![photo]);
    assert_eq!(app.tile_post_ids("media=video").await, Vec::<i64>::new());
    assert_eq!(app.tile_post_ids("media=text").await, vec![text]);
}

#[tokio::test]
#[ignore = "hits the real YouTube oEmbed endpoint and thumbnail servers"]
async fn live_youtube_video_is_saved_with_its_thumbnail() {
    let (_, fake) = spawn_upstream().await;
    // `{base}/oembed` is YouTube's real endpoint.
    let base = "https://www.youtube.com".to_string();
    let app = TestApp::start_with(base, fake, TempDir::new().unwrap(), None).await;
    let id = app
        .save("https://www.youtube.com/watch?v=dQw4w9WgXcQ", None)
        .await;
    let (_, d) = app.call("GET", &format!("/api/posts/{id}"), None).await;
    assert_eq!(d["state"], "done", "{d}");
    assert_eq!(d["video"]["channel"]["name"], "Rick Astley");
    let file = app.state.media_dir.join(id.to_string()).join("main-0.jpg");
    assert!(std::fs::metadata(&file).unwrap().len() > 10_000);
}

#[tokio::test]
async fn a_web_page_is_bookmarked_with_its_share_image_and_icon() {
    let app = TestApp::start().await;
    let url = app.page(
        "harbor",
        200,
        r#"<html lang="ja"><head><title>fallback</title>
          <meta property="og:title" content="港町の朝">
          <meta property="og:description" content="朝の港を歩いた記録">
          <meta property="og:site_name" content="Harbor Notes">
          <meta property="og:image" content="/images/cover.png">
          <link rel="icon" href="/images/icon.png"></head></html>"#,
    );
    let id = app.save(&format!("{url}?utm_source=feed#top"), None).await;

    let tile = app.tiles("").await["tiles"][0].clone();
    assert_eq!(tile["kind"], "web");
    assert_eq!(tile["text"], "港町の朝\n朝の港を歩いた記録");
    assert_eq!(tile["author_name"], "Harbor Notes");
    assert_eq!(
        (tile["width"].clone(), tile["height"].clone()),
        (json!(1200), json!(630))
    );
    assert_eq!(
        app.call("GET", tile["src"].as_str().unwrap(), None).await.1,
        "IMG:cover.png"
    );

    let (_, d) = app.call("GET", &format!("/api/posts/{id}"), None).await;
    assert_eq!(d["source"], "web");
    assert_eq!(d["status_id"], url);
    let page = &d["page"];
    assert_eq!(page["url"], url);
    assert_eq!(page["title"], "港町の朝");
    assert_eq!(page["description"], "朝の港を歩いた記録");
    assert_eq!(page["host"], "127.0.0.1");
    assert_eq!(page["image"]["src"], tile["src"]);
    assert_eq!(
        app.call("GET", page["icon"].as_str().unwrap(), None)
            .await
            .1,
        "IMG:icon.png"
    );
    assert_eq!(app.tile_post_ids("q=歩いた").await, vec![id]);

    let (_, v) = app
        .call(
            "POST",
            "/api/posts",
            Some(json!({ "urls": [format!("{url}?fbclid=x")] })),
        )
        .await;
    assert_eq!(v[0]["outcome"], "duplicate");
    assert_eq!(v[0]["post_id"], id);
}

#[tokio::test]
async fn a_missing_or_non_image_share_image_still_saves_the_bookmark() {
    let app = TestApp::start().await;
    app.fake
        .lock()
        .unwrap()
        .missing_files
        .insert("gone.png".into());
    let url = app.page(
        "noimage",
        200,
        &format!(
            r#"<meta property="og:image" content="/images/gone.png"><link rel="icon" href="{}">"#,
            app.page("not-an-icon", 200, "<p>html</p>")
        ),
    );
    let id = app.save(&url, None).await;

    let (_, d) = app.call("GET", &format!("/api/posts/{id}"), None).await;
    assert_eq!(d["state"], "done");
    assert!(d["page"]["image"].is_null());
    assert!(d["page"]["icon"].is_null());
    assert_eq!(d["page"]["title"], "noimage");
    let tile = app.tiles("").await["tiles"][0].clone();
    assert_eq!(tile["kind"], "web");
    assert!(tile.get("src").is_none());
}

#[tokio::test]
async fn missing_pages_are_rejected_and_blocked_ones_fail_until_retried() {
    let app = TestApp::start().await;
    let gone = app.page("gone", 410, "");
    let blocked = app.page("blocked", 403, "");
    let (_, v) = app
        .call(
            "POST",
            "/api/posts",
            Some(json!({ "urls": [gone, blocked] })),
        )
        .await;
    assert_eq!(v[0]["outcome"], "unavailable");
    assert_eq!(v[0]["reason"], "not_found");
    assert_eq!(v[1]["outcome"], "created");
    let id = v[1]["post_id"].as_i64().unwrap();
    let tile = app.tiles("").await["tiles"][0].clone();
    assert_eq!(tile["kind"], "failed");
    assert_eq!(tile["failure"], "page_unavailable");

    let (s, v) = app
        .call("POST", &format!("/api/posts/{id}/retry"), None)
        .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert_eq!(v["error"], "page_unavailable");
    app.page("blocked", 200, "<title>open now</title>");
    app.call("POST", &format!("/api/posts/{id}/retry"), None)
        .await;
    assert_eq!(
        app.wait_until_settled(id).await["page"]["title"],
        "open now"
    );
}

#[tokio::test]
async fn web_pages_have_their_own_media_type() {
    let app = TestApp::start().await;
    app.upstream("160", app.status("160", "text"));
    let text = app.save("https://x.com/a/status/160", None).await;
    let page = app
        .save(&app.page("p", 200, "<title>p</title>"), None)
        .await;

    assert_eq!(app.tile_post_ids("media=web").await, vec![page]);
    assert_eq!(app.tile_post_ids("media=text").await, vec![text]);
}

#[tokio::test]
#[ignore = "hits real web pages and their images"]
async fn live_web_pages_are_bookmarked_with_their_share_images() {
    let app = TestApp::start().await;
    for url in [
        "https://github.com/rust-lang/rust",
        "https://www.rust-lang.org/",
    ] {
        let id = app.save(url, None).await;
        let (_, d) = app.call("GET", &format!("/api/posts/{id}"), None).await;
        assert_eq!(d["state"], "done", "{url}: {d}");
        assert!(
            !d["page"]["title"].as_str().unwrap().is_empty(),
            "{url}: {d}"
        );
        assert!(d["page"]["image"]["src"].is_string(), "{url}: {d}");
    }
}
