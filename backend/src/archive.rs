use std::path::Path;

use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use tokio::{io::AsyncWriteExt, sync::mpsc};

use crate::{
    AppState,
    fxtwitter::{MediaItem, Status},
};

#[derive(Debug, Clone, PartialEq)]
pub struct Download {
    pub role: &'static str,
    pub position: i64,
    pub kind: &'static str,
    pub url: String,
    pub thumbnail_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
}

impl Download {
    fn photo(role: &'static str, position: i64, url: &str) -> Self {
        Self {
            role,
            position,
            kind: "photo",
            url: url.to_string(),
            thumbnail_url: None,
            width: None,
            height: None,
            duration: None,
        }
    }

    pub fn file_name(&self) -> String {
        format!(
            "{}-{}.{}",
            self.role,
            self.position,
            extension(&self.url, self.kind)
        )
    }

    pub fn thumbnail_name(&self) -> Option<String> {
        let url = self.thumbnail_url.as_deref()?;
        Some(format!(
            "{}-{}-thumb.{}",
            self.role,
            self.position,
            extension(url, "photo")
        ))
    }
}

fn extension(url: &str, kind: &str) -> String {
    let parsed = reqwest::Url::parse(url).ok();
    let from_path = parsed.as_ref().and_then(|u| {
        u.path_segments()?
            .next_back()?
            .rsplit_once('.')
            .map(|(_, e)| e.to_string())
    });
    let from_query = || {
        parsed
            .as_ref()?
            .query_pairs()
            .find_map(|(k, v)| (k == "format").then(|| v.into_owned()))
    };
    from_path
        .or_else(from_query)
        .filter(|e| !e.is_empty() && e.len() <= 5 && e.bytes().all(|b| b.is_ascii_alphanumeric()))
        .unwrap_or_else(|| if kind == "photo" { "jpg" } else { "mp4" }.to_string())
}

fn media_downloads(role: &'static str, items: &[MediaItem]) -> Vec<Download> {
    items
        .iter()
        .filter_map(|m| {
            let kind = match m.kind.as_str() {
                "photo" => "photo",
                "video" => "video",
                "gif" => "gif",
                _ => return None,
            };
            Some((kind, m))
        })
        .enumerate()
        .map(|(i, (kind, m))| Download {
            role,
            position: i as i64,
            kind,
            url: if kind == "photo" {
                m.url.clone()
            } else {
                m.best_mp4().to_string()
            },
            thumbnail_url: m.thumbnail_url.clone().filter(|_| kind != "photo"),
            width: m.width,
            height: m.height,
            duration: m.duration.filter(|d| *d > 0.0),
        })
        .collect()
}

// Not every URL in an article: its body also links to outside sites that must not be fetched.
fn collect_image_urls(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) if s.starts_with("https://pbs.twimg.com/") && !out.contains(s) => {
            out.push(s.clone())
        }
        Value::Array(a) => a.iter().for_each(|x| collect_image_urls(x, out)),
        Value::Object(o) => o.values().for_each(|x| collect_image_urls(x, out)),
        _ => {}
    }
}

pub fn plan(status: &Status) -> Vec<Download> {
    let mut out = media_downloads("main", &status.media.all);
    if let Some(url) = &status.author.avatar_url {
        out.push(Download::photo("avatar", 0, url));
    }
    if let Some(Ok(quote)) = status.quoted() {
        out.extend(media_downloads("quote", &quote.media.all));
        if let Some(url) = &quote.author.avatar_url {
            out.push(Download::photo("quote_avatar", 0, url));
        }
    }
    if let Some(image) = status.card.as_ref().and_then(|c| c.image.as_ref()) {
        let mut d = Download::photo("card", 0, &image.url);
        (d.width, d.height) = (image.width, image.height);
        out.push(d);
    }
    if let Some(article) = &status.article {
        let mut urls = vec![];
        collect_image_urls(article, &mut urls);
        out.extend(
            urls.iter()
                .enumerate()
                .map(|(i, u)| Download::photo("article", i as i64, u)),
        );
    }
    out
}

async fn download(http: &reqwest::Client, url: &str, dest: &Path) -> Result<(), String> {
    let mut res = http
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("{url}: {e}"))?;
    let part = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&part)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(chunk) = res.chunk().await.map_err(|e| format!("{url}: {e}"))? {
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
    }
    file.flush().await.map_err(|e| e.to_string())?;
    tokio::fs::rename(&part, dest)
        .await
        .map_err(|e| e.to_string())
}

async fn archive_post(state: &AppState, post_id: i64) {
    let snapshot: Option<String> = match state
        .db()
        .query_row(
            "SELECT snapshot FROM posts WHERE id = ?1 AND state = 'pending'",
            [post_id],
            |r| r.get(0),
        )
        .optional()
    {
        Ok(Some(s)) => s,
        Ok(None) => return,
        Err(e) => return eprintln!("archive {post_id}: {e}"),
    };
    let downloads = snapshot
        .and_then(|s| serde_json::from_str::<Status>(&s).ok())
        .map(|s| plan(&s));
    let Some(downloads) = downloads else {
        return finish(state, post_id, Err("snapshot_missing".into()), &[]);
    };
    let dir = state.media_dir.join(post_id.to_string());
    let mut result = tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string());
    for d in &downloads {
        if result.is_err() {
            break;
        }
        result = download(&state.http, &d.url, &dir.join(d.file_name())).await;
        if let (Ok(()), Some(thumb), Some(name)) = (&result, &d.thumbnail_url, d.thumbnail_name()) {
            result = download(&state.http, thumb, &dir.join(name)).await;
        }
    }
    if let Err(e) = &result {
        eprintln!("archive {post_id}: {e}");
    }
    finish(
        state,
        post_id,
        result.map_err(|_| "media_download".into()),
        &downloads,
    );
}

fn finish(state: &AppState, post_id: i64, result: Result<(), String>, downloads: &[Download]) {
    let outcome = (|| -> rusqlite::Result<bool> {
        let mut db = state.db();
        let tx = db.transaction()?;
        let updated = match &result {
            Ok(()) => {
                tx.execute("DELETE FROM media WHERE post_id = ?1", [post_id])?;
                for d in downloads {
                    tx.execute(
                        "INSERT INTO media (post_id, role, position, kind, file, thumbnail, width, height, duration, remote_url)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![post_id, d.role, d.position, d.kind, d.file_name(), d.thumbnail_name(), d.width, d.height, d.duration, d.url],
                    )?;
                }
                tx.execute(
                    "UPDATE posts SET state = 'done', failure = NULL WHERE id = ?1",
                    [post_id],
                )?
            }
            Err(reason) => tx.execute(
                "UPDATE posts SET state = 'failed', failure = ?1 WHERE id = ?2",
                params![reason, post_id],
            )?,
        };
        tx.commit()?;
        Ok(updated > 0)
    })();
    match outcome {
        Ok(true) => {}
        Ok(false) => crate::posts::remove_media_dir(state, post_id),
        Err(e) => eprintln!("archive {post_id}: {e}"),
    }
}

pub fn spawn_worker(state: AppState, mut queue: mpsc::UnboundedReceiver<i64>) {
    tokio::spawn(async move {
        let resumed: Vec<i64> = state
            .db()
            .prepare("SELECT id FROM posts WHERE state = 'pending' ORDER BY id")
            .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
            .unwrap_or_default();
        for id in resumed {
            archive_post(&state, id).await;
        }
        while let Some(id) = queue.recv().await {
            archive_post(&state, id).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(v: Value) -> Status {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn plans_main_media_avatar_quote_card_and_article_images() {
        let s = status(json!({
            "author": {"avatar_url": "https://pbs.twimg.com/profile_images/a.jpg"},
            "media": {"all": [
                {"type": "photo", "url": "https://pbs.twimg.com/media/p.jpg?name=orig", "width": 4, "height": 3},
                {"type": "mosaic_photo", "url": "https://mosaic"},
                {"type": "video", "url": "https://video.twimg.com/v.mp4", "thumbnail_url": "https://pbs.twimg.com/t.jpg",
                 "formats": [{"url": "https://video.twimg.com/hi.mp4", "container": "mp4", "bitrate": 9}]}
            ]},
            "quote": {"type": "status", "author": {"avatar_url": "https://pbs.twimg.com/q.jpg"},
                      "media": {"all": [{"type": "gif", "url": "https://video.twimg.com/g.mp4"}]}},
            "card": {"url": "https://n", "image": {"url": "https://pbs.twimg.com/card_img/1/x?format=png&name=800x419"}},
            "article": {"cover_media": {"media_info": {"original_img_url": "https://pbs.twimg.com/media/c.jpg"}},
                        "media_entities": [{"media_info": {"original_img_url": "https://pbs.twimg.com/media/c.jpg"}}]}
        }));
        let names: Vec<(&str, String)> = plan(&s).iter().map(|d| (d.role, d.file_name())).collect();
        assert_eq!(
            names,
            [
                ("main", "main-0.jpg".to_string()),
                ("main", "main-1.mp4".to_string()),
                ("avatar", "avatar-0.jpg".to_string()),
                ("quote", "quote-0.mp4".to_string()),
                ("quote_avatar", "quote_avatar-0.jpg".to_string()),
                ("card", "card-0.png".to_string()),
                ("article", "article-0.jpg".to_string()),
            ]
        );
        let video = &plan(&s)[1];
        assert_eq!(video.url, "https://video.twimg.com/hi.mp4");
        assert_eq!(video.thumbnail_name().as_deref(), Some("main-1-thumb.jpg"));
    }

    #[test]
    fn skips_quote_media_when_the_quote_is_unavailable() {
        let s = status(json!({"quote": {"type": "tombstone", "message": "gone"}}));
        assert!(plan(&s).is_empty());
    }
}
