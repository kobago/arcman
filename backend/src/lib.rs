pub mod archive;
pub mod auth;
pub mod db;
pub mod error;
pub mod fxtwitter;
pub mod organize;
pub mod posts;
pub mod settings;
pub mod tags;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    Router, middleware,
    routing::{get, post, put},
};
use rusqlite::Connection;
use tokio::sync::mpsc;
use tower_http::services::ServeDir;

pub struct Config {
    pub home: PathBuf,
    pub fxtwitter_base_url_env: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub media_dir: PathBuf,
    pub fxtwitter_base_url_env: Option<Arc<str>>,
    pub http: reqwest::Client,
    pub archive_queue: mpsc::UnboundedSender<i64>,
    pub login_guard: Arc<Mutex<auth::LoginGuard>>,
}

impl AppState {
    pub fn open(config: Config) -> BoxResult<(Self, mpsc::UnboundedReceiver<i64>)> {
        let media_dir = config.home.join("media");
        std::fs::create_dir_all(&media_dir)?;
        let conn = db::open(&config.home.join("arcman.db"))?;
        let (tx, rx) = mpsc::unbounded_channel();
        let state = Self {
            db: Arc::new(Mutex::new(conn)),
            media_dir,
            fxtwitter_base_url_env: config.fxtwitter_base_url_env.map(Into::into),
            http: reqwest::Client::builder()
                .user_agent(concat!("arcman/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(600))
                .build()?,
            archive_queue: tx,
            login_guard: Default::default(),
        };
        Ok((state, rx))
    }

    pub fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn app(state: AppState, queue: mpsc::UnboundedReceiver<i64>) -> Router {
    archive::spawn_worker(state.clone(), queue);
    let protected = Router::new()
        .route("/api/logout", post(auth::logout))
        .route("/api/logout-all", post(auth::logout_all))
        .route("/api/session", get(auth::session))
        .route("/api/posts", post(posts::create))
        .route(
            "/api/posts/{id}",
            get(posts::detail)
                .patch(posts::update)
                .delete(posts::delete),
        )
        .route("/api/posts/{id}/tags", put(posts::set_tags))
        .route("/api/posts/{id}/retry", post(posts::retry))
        .route("/api/tiles", get(posts::tiles))
        .route("/api/authors", get(posts::authors))
        .route("/api/sidebar", get(organize::sidebar))
        .route("/api/categories", post(organize::create_category))
        .route("/api/categories/order", put(organize::order_categories))
        .route(
            "/api/categories/{id}",
            axum::routing::patch(organize::rename_category).delete(organize::delete_category),
        )
        .route("/api/folders", post(organize::create_folder))
        .route("/api/folders/order", put(organize::order_folders))
        .route(
            "/api/folders/{id}",
            get(organize::folder)
                .patch(organize::rename_folder)
                .delete(organize::delete_folder),
        )
        .route("/api/tags", get(tags::list))
        .route(
            "/api/tags/{id}",
            axum::routing::patch(tags::update).delete(tags::delete),
        )
        .route("/api/settings", get(settings::get).put(settings::put))
        .nest_service("/media", ServeDir::new(&state.media_dir))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ));
    Router::new()
        .route("/api/login", post(auth::login))
        .merge(protected)
        .with_state(state)
}

pub type BoxResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
