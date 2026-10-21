use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AppState,
    error::{ApiError, ApiResult},
    fxtwitter::{self, Card, FetchError, Note, Status, Translation},
    settings, tags,
};

pub fn remove_media_dir(state: &AppState, post_id: i64) {
    let dir = state.media_dir.join(post_id.to_string());
    if let Err(e) = std::fs::remove_dir_all(&dir)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!("remove {}: {e}", dir.display());
    }
}

fn media_url(post_id: i64, file: &str) -> String {
    format!("/media/{post_id}/{file}")
}

fn ensure_folder(conn: &Connection, id: Option<i64>) -> ApiResult<()> {
    if let Some(id) = id {
        conn.query_row("SELECT 1 FROM folders WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .ok_or(ApiError::BadRequest("unknown_folder"))?;
    }
    Ok(())
}

fn store_snapshot(
    conn: &Connection,
    post_id: i64,
    raw: &Value,
    status: &Status,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE posts SET author_screen_name = ?1, author_name = ?2, text = ?3, lang = ?4,
             posted_at = strftime('%Y-%m-%dT%H:%M:%SZ', ?5, 'unixepoch'),
             snapshot = ?6, state = 'pending', failure = NULL
         WHERE id = ?7",
        params![
            status.author.screen_name,
            status.author.name,
            status.text,
            status.lang,
            status.created_timestamp,
            raw.to_string(),
            post_id
        ],
    )?;
    Ok(())
}

#[derive(Deserialize)]
pub struct NewPosts {
    urls: Vec<String>,
    folder_id: Option<i64>,
}

#[derive(Serialize)]
pub struct SaveResult {
    url: String,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

pub async fn create(
    State(state): State<AppState>,
    Json(input): Json<NewPosts>,
) -> ApiResult<Json<Vec<SaveResult>>> {
    ensure_folder(&state.db(), input.folder_id)?;
    let base = settings::fxtwitter_base_url(&state)?;
    let mut results = vec![];
    for url in input
        .urls
        .iter()
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
    {
        let result = |outcome, post_id, reason| SaveResult {
            url: url.to_string(),
            outcome,
            post_id,
            reason,
        };
        let Some(status_id) = fxtwitter::parse_status_id(url) else {
            results.push(result("invalid_url", None, None));
            continue;
        };
        let existing: Option<i64> = state
            .db()
            .query_row(
                "SELECT id FROM posts WHERE status_id = ?1",
                [&status_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            results.push(result("duplicate", Some(id), None));
            continue;
        }
        let fetched = fxtwitter::fetch(&state.http, &base, &status_id).await;
        if let Err(FetchError::Unavailable(reason)) = fetched {
            results.push(result("unavailable", None, Some(reason)));
            continue;
        }
        let post_id = {
            let db = state.db();
            let inserted = db.execute(
                "INSERT INTO posts (status_id, source_url, state, failure, folder_id)
                 VALUES (?1, ?2, 'failed', 'api_unavailable', ?3) ON CONFLICT (status_id) DO NOTHING",
                params![status_id, url, input.folder_id],
            )?;
            let id: i64 = db.query_row(
                "SELECT id FROM posts WHERE status_id = ?1",
                [&status_id],
                |r| r.get(0),
            )?;
            if inserted == 0 {
                results.push(result("duplicate", Some(id), None));
                continue;
            }
            match &fetched {
                Ok((raw, status)) => store_snapshot(&db, id, raw, status)?,
                Err(e) => eprintln!("fetch {status_id}: {e:?}"),
            }
            id
        };
        if fetched.is_ok() {
            let _ = state.archive_queue.send(post_id);
        }
        results.push(result("created", Some(post_id), None));
    }
    Ok(Json(results))
}

pub async fn retry(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let (status_id, post_state, has_snapshot): (String, String, bool) = state.db().query_row(
        "SELECT status_id, state, snapshot IS NOT NULL FROM posts WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if post_state != "failed" {
        return Err(ApiError::Conflict("not_failed"));
    }
    if has_snapshot {
        state.db().execute(
            "UPDATE posts SET state = 'pending', failure = NULL WHERE id = ?1",
            [id],
        )?;
    } else {
        let base = settings::fxtwitter_base_url(&state)?;
        match fxtwitter::fetch(&state.http, &base, &status_id).await {
            Ok((raw, status)) => store_snapshot(&state.db(), id, &raw, &status)?,
            Err(FetchError::Unavailable(reason)) => return Err(ApiError::Unprocessable(reason)),
            Err(FetchError::Transport(e)) => {
                eprintln!("fetch {status_id}: {e}");
                return Err(ApiError::BadGateway("api_unavailable"));
            }
        }
    }
    let _ = state.archive_queue.send(id);
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize, Default)]
pub struct TileQuery {
    folder: Option<String>,
    q: Option<String>,
    tags: Option<String>,
    from: Option<String>,
    to: Option<String>,
    media: Option<String>,
    author: Option<String>,
    order: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

#[derive(Serialize)]
pub struct Tile {
    post_id: i64,
    kind: String,
    index: i64,
    count: i64,
    url: String,
    saved_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    src: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thumbnail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author_screen_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<String>,
}

#[derive(Serialize)]
pub struct TilePage {
    tiles: Vec<Tile>,
    next_cursor: Option<String>,
}

fn like_pattern(q: &str) -> String {
    let escaped = q
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

const MAIN_MEDIA_OF: &str = "SELECT 1 FROM media m WHERE m.post_id = p.id AND m.role = 'main'";

pub async fn tiles(
    State(state): State<AppState>,
    Query(q): Query<TileQuery>,
) -> ApiResult<Json<TilePage>> {
    let mut wh: Vec<String> = vec![];
    let mut args: Vec<SqlValue> = vec![];
    match q.folder.as_deref() {
        None | Some("all") => {}
        Some("unsorted") => wh.push("p.folder_id IS NULL".into()),
        Some(id) => {
            let id: i64 = id
                .parse()
                .map_err(|_| ApiError::BadRequest("invalid_folder"))?;
            wh.push("p.folder_id = ?".into());
            args.push(id.into());
        }
    }
    if let Some(text) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        wh.push(
            "(p.text LIKE ? ESCAPE '\\' OR EXISTS (SELECT 1 FROM post_tags pt JOIN tags t ON t.id = pt.tag_id
              WHERE pt.post_id = p.id AND t.name LIKE ? ESCAPE '\\'))"
                .into(),
        );
        args.extend([like_pattern(text).into(), like_pattern(text).into()]);
    }
    for tag in q
        .tags
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
    {
        let tag: i64 = tag
            .parse()
            .map_err(|_| ApiError::BadRequest("invalid_tags"))?;
        wh.push("EXISTS (SELECT 1 FROM post_tags WHERE post_id = p.id AND tag_id = ?)".into());
        args.push(tag.into());
    }
    for (bound, op) in [(&q.from, ">="), (&q.to, "<")] {
        if let Some(t) = bound {
            wh.push(format!("p.saved_at {op} strftime('%Y-%m-%dT%H:%M:%fZ', ?)"));
            args.push(t.clone().into());
        }
    }
    let media_kinds: Option<&[&str]> = match q.media.as_deref() {
        None => None,
        Some("photo") => Some(&["photo"]),
        Some("video") => Some(&["video", "gif"]),
        Some("text") => Some(&[]),
        Some(_) => return Err(ApiError::BadRequest("invalid_media")),
    };
    match media_kinds {
        Some([]) => wh.push(format!("p.state = 'done' AND NOT EXISTS ({MAIN_MEDIA_OF})")),
        Some(kinds) => wh.push(format!(
            "EXISTS ({MAIN_MEDIA_OF} AND m.kind IN ({}))",
            kinds
                .iter()
                .map(|k| format!("'{k}'"))
                .collect::<Vec<_>>()
                .join(",")
        )),
        None => {}
    }
    if let Some(author) = q.author.as_deref().filter(|s| !s.is_empty()) {
        wh.push("p.author_screen_name = ? COLLATE NOCASE".into());
        args.push(author.to_string().into());
    }
    let asc = q.order.as_deref() == Some("asc");
    if let Some(cursor) = &q.cursor {
        let (saved_at, id) = cursor
            .rsplit_once('|')
            .and_then(|(s, id)| Some((s.to_string(), id.parse::<i64>().ok()?)))
            .ok_or(ApiError::BadRequest("invalid_cursor"))?;
        wh.push(format!(
            "(p.saved_at, p.id) {} (?, ?)",
            if asc { ">" } else { "<" }
        ));
        args.extend([saved_at.into(), id.into()]);
    }
    let limit = q.limit.unwrap_or(30).clamp(1, 200) as usize;
    let dir = if asc { "ASC" } else { "DESC" };
    let where_sql = if wh.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", wh.join(" AND "))
    };
    args.push(((limit + 1) as i64).into());

    let db = state.db();
    struct Row {
        id: i64,
        url: String,
        saved_at: String,
        state: String,
        failure: Option<String>,
        text: String,
        author_name: String,
        author_screen_name: String,
    }
    let mut rows: Vec<Row> = db
        .prepare(&format!(
            "SELECT p.id, p.source_url, p.saved_at, p.state, p.failure, p.text, p.author_name, p.author_screen_name
             FROM posts p {where_sql} ORDER BY p.saved_at {dir}, p.id {dir} LIMIT ?"
        ))?
        .query_map(params_from_iter(args), |r| {
            Ok(Row {
                id: r.get(0)?,
                url: r.get(1)?,
                saved_at: r.get(2)?,
                state: r.get(3)?,
                failure: r.get(4)?,
                text: r.get(5)?,
                author_name: r.get(6)?,
                author_screen_name: r.get(7)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let next_cursor = (rows.len() > limit).then(|| {
        rows.truncate(limit);
        let last = rows.last().expect("limit is at least 1");
        format!("{}|{}", last.saved_at, last.id)
    });

    let ids: Vec<SqlValue> = rows.iter().map(|r| r.id.into()).collect();
    let mut media: HashMap<i64, Vec<MediaRow>> = HashMap::new();
    if !ids.is_empty() {
        let mut stmt = db.prepare(&format!(
            "SELECT post_id, role, kind, file, thumbnail, width, height, remote_url FROM media
             WHERE role = 'main' AND post_id IN ({}) ORDER BY post_id, position",
            vec!["?"; ids.len()].join(",")
        ))?;
        let mut q = stmt.query(params_from_iter(ids))?;
        while let Some(r) = q.next()? {
            media.entry(r.get(0)?).or_default().push(MediaRow {
                role: r.get(1)?,
                kind: r.get(2)?,
                file: r.get(3)?,
                thumbnail: r.get(4)?,
                width: r.get(5)?,
                height: r.get(6)?,
                remote_url: r.get(7)?,
            });
        }
    }

    let mut tiles = vec![];
    for row in rows {
        let base = |kind: &str| Tile {
            post_id: row.id,
            kind: kind.to_string(),
            index: 0,
            count: 1,
            url: row.url.clone(),
            saved_at: row.saved_at.clone(),
            src: None,
            thumbnail: None,
            width: None,
            height: None,
            text: None,
            author_name: None,
            author_screen_name: None,
            failure: None,
        };
        match row.state.as_str() {
            "pending" => tiles.push(base("pending")),
            "failed" => tiles.push(Tile {
                failure: row.failure.clone(),
                ..base("failed")
            }),
            _ => match media.get(&row.id) {
                None => tiles.push(Tile {
                    text: Some(row.text.clone()),
                    author_name: Some(row.author_name.clone()),
                    author_screen_name: Some(row.author_screen_name.clone()),
                    ..base("text")
                }),
                Some(items) => {
                    for (i, m) in items.iter().enumerate() {
                        if media_kinds.is_some_and(|k| !k.contains(&m.kind.as_str())) {
                            continue;
                        }
                        tiles.push(Tile {
                            index: i as i64,
                            count: items.len() as i64,
                            src: Some(media_url(row.id, &m.file)),
                            thumbnail: m.thumbnail.as_deref().map(|t| media_url(row.id, t)),
                            width: m.width,
                            height: m.height,
                            ..base(&m.kind)
                        });
                    }
                }
            },
        }
    }
    Ok(Json(TilePage { tiles, next_cursor }))
}

#[derive(Serialize)]
pub struct TagRef {
    id: i64,
    name: String,
    color: Option<String>,
}

#[derive(Serialize)]
pub struct MediaView {
    kind: String,
    src: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thumbnail: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Serialize)]
pub struct AuthorView {
    name: String,
    screen_name: String,
    avatar: Option<String>,
}

#[derive(Serialize)]
pub struct Stats {
    likes: Option<u64>,
    reposts: Option<u64>,
    quotes: Option<u64>,
    replies: Option<u64>,
    views: Option<u64>,
    bookmarks: Option<u64>,
}

#[derive(Serialize)]
pub struct CardView {
    url: String,
    title: Option<String>,
    description: Option<String>,
    domain: Option<String>,
    image: Option<String>,
}

#[derive(Serialize)]
pub struct QuoteView {
    available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<Box<StatusView>>,
}

#[derive(Serialize)]
pub struct ArticleView {
    content: Value,
    local_urls: HashMap<String, String>,
}

#[derive(Serialize)]
pub struct StatusView {
    url: String,
    text: String,
    lang: Option<String>,
    posted_at: Option<i64>,
    author: AuthorView,
    stats: Stats,
    media: Vec<MediaView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    translation: Option<Translation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quote: Option<QuoteView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    card: Option<CardView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    poll: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    community_note: Option<Note>,
    #[serde(skip_serializing_if = "Option::is_none")]
    article: Option<ArticleView>,
}

#[derive(Serialize)]
pub struct Detail {
    id: i64,
    status_id: String,
    url: String,
    state: String,
    failure: Option<String>,
    folder_id: Option<i64>,
    saved_at: String,
    tags: Vec<TagRef>,
    status: Option<StatusView>,
}

struct MediaRow {
    role: String,
    kind: String,
    file: String,
    thumbnail: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    remote_url: String,
}

struct LocalMedia {
    post_id: i64,
    rows: Vec<MediaRow>,
}

impl LocalMedia {
    fn by_role(&self, role: &str) -> Vec<MediaView> {
        self.rows
            .iter()
            .filter(|m| m.role == role)
            .map(|m| MediaView {
                kind: m.kind.clone(),
                src: media_url(self.post_id, &m.file),
                thumbnail: m.thumbnail.as_deref().map(|t| media_url(self.post_id, t)),
                width: m.width,
                height: m.height,
            })
            .collect()
    }

    fn first(&self, role: &str) -> Option<String> {
        self.by_role(role).into_iter().next().map(|m| m.src)
    }

    fn url_map(&self, role: &str) -> HashMap<String, String> {
        self.rows
            .iter()
            .filter(|m| m.role == role)
            .map(|m| (m.remote_url.clone(), media_url(self.post_id, &m.file)))
            .collect()
    }
}

fn status_view(s: &Status, local: &LocalMedia, quote: bool) -> StatusView {
    let (media_role, avatar_role) = if quote {
        ("quote", "quote_avatar")
    } else {
        ("main", "avatar")
    };
    StatusView {
        url: s.url.clone(),
        text: s.text.clone(),
        lang: s.lang.clone(),
        posted_at: s.created_timestamp,
        author: AuthorView {
            name: s.author.name.clone(),
            screen_name: s.author.screen_name.clone(),
            avatar: local.first(avatar_role),
        },
        stats: Stats {
            likes: s.likes,
            reposts: s.reposts,
            quotes: s.quotes,
            replies: s.replies,
            views: s.views,
            bookmarks: s.bookmarks,
        },
        media: local.by_role(media_role),
        translation: s.translation.clone(),
        quote: if quote {
            None
        } else {
            s.quoted().map(|q| match q {
                Ok(q) => QuoteView {
                    available: true,
                    message: None,
                    status: Some(Box::new(status_view(&q, local, true))),
                },
                Err(message) => QuoteView {
                    available: false,
                    message: Some(message),
                    status: None,
                },
            })
        },
        card: if quote {
            None
        } else {
            s.card.as_ref().map(|c: &Card| CardView {
                url: c.url.clone(),
                title: c.title.clone(),
                description: c.description.clone(),
                domain: c.domain.clone(),
                image: local.first("card"),
            })
        },
        poll: s.poll.clone(),
        community_note: s.community_note.clone(),
        article: (!quote)
            .then(|| s.article.clone())
            .flatten()
            .map(|content| ArticleView {
                content,
                local_urls: local.url_map("article"),
            }),
    }
}

pub async fn detail(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<Json<Detail>> {
    let db = state.db();
    let (status_id, url, post_state, failure, folder_id, saved_at, snapshot): (
        String,
        String,
        String,
        Option<String>,
        Option<i64>,
        String,
        Option<String>,
    ) = db.query_row(
        "SELECT status_id, source_url, state, failure, folder_id, saved_at, snapshot FROM posts WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
    )?;
    let tags = db
        .prepare(
            "SELECT t.id, t.name, t.color FROM tags t JOIN post_tags pt ON pt.tag_id = t.id
             WHERE pt.post_id = ?1 ORDER BY t.key",
        )?
        .query_map([id], |r| {
            Ok(TagRef {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let rows = db
        .prepare(
            "SELECT role, kind, file, thumbnail, width, height, remote_url FROM media
             WHERE post_id = ?1 ORDER BY role, position",
        )?
        .query_map([id], |r| {
            Ok(MediaRow {
                role: r.get(0)?,
                kind: r.get(1)?,
                file: r.get(2)?,
                thumbnail: r.get(3)?,
                width: r.get(4)?,
                height: r.get(5)?,
                remote_url: r.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let local = LocalMedia { post_id: id, rows };
    let status = snapshot
        .and_then(|s| serde_json::from_str::<Status>(&s).ok())
        .map(|s| status_view(&s, &local, false));
    Ok(Json(Detail {
        id,
        status_id,
        url,
        state: post_state,
        failure,
        folder_id,
        saved_at,
        tags,
        status,
    }))
}

#[derive(Deserialize)]
pub struct PostUpdate {
    folder_id: Option<i64>,
}

pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<PostUpdate>,
) -> ApiResult<StatusCode> {
    let db = state.db();
    ensure_folder(&db, input.folder_id)?;
    match db.execute(
        "UPDATE posts SET folder_id = ?1 WHERE id = ?2",
        params![input.folder_id, id],
    )? {
        0 => Err(ApiError::NotFound),
        _ => Ok(StatusCode::NO_CONTENT),
    }
}

#[derive(Deserialize)]
pub struct TagNames {
    names: Vec<String>,
}

pub async fn set_tags(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<TagNames>,
) -> ApiResult<Json<Vec<TagRef>>> {
    {
        let mut db = state.db();
        db.query_row("SELECT 1 FROM posts WHERE id = ?1", [id], |_| Ok(()))?;
        let tx = db.transaction()?;
        tx.execute("DELETE FROM post_tags WHERE post_id = ?1", [id])?;
        for name in &input.names {
            let tag_id = tags::find_or_create(&tx, name)?;
            tx.execute(
                "INSERT OR IGNORE INTO post_tags (post_id, tag_id) VALUES (?1, ?2)",
                [id, tag_id],
            )?;
        }
        tx.commit()?;
    }
    let tags = state
        .db()
        .prepare(
            "SELECT t.id, t.name, t.color FROM tags t JOIN post_tags pt ON pt.tag_id = t.id
             WHERE pt.post_id = ?1 ORDER BY t.key",
        )?
        .query_map([id], |r| {
            Ok(TagRef {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(tags))
}

pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    if state
        .db()
        .execute("DELETE FROM posts WHERE id = ?1", [id])?
        == 0
    {
        return Err(ApiError::NotFound);
    }
    remove_media_dir(&state, id);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct AuthorCount {
    screen_name: String,
    name: String,
    count: i64,
}

pub async fn authors(State(state): State<AppState>) -> ApiResult<Json<Vec<AuthorCount>>> {
    let authors = state
        .db()
        .prepare(
            "SELECT author_screen_name, MAX(author_name), COUNT(*) AS n FROM posts
             WHERE author_screen_name <> '' GROUP BY author_screen_name COLLATE NOCASE
             ORDER BY n DESC, author_screen_name",
        )?
        .query_map([], |r| {
            Ok(AuthorCount {
                screen_name: r.get(0)?,
                name: r.get(1)?,
                count: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Json(authors))
}
