use std::collections::HashMap;

use reqwest::Url;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{archive::Download, fxtwitter::FetchError, pixiv};

pub const DEFAULT_BASE_URL: &str = "https://imgur.com";

/// Imgur ids are short base62 strings like `Pcc90bT`, so they are saved under a prefix.
const STATUS_PREFIX: &str = "imgur:";

/// The page is drawn by script, but its head still carries the post as JSON.
const POST_DATA: &str = "window.postDataJSON=";

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; arcman/",
    env!("CARGO_PKG_VERSION"),
    "; +https://github.com/kobago/arcman)"
);

fn valid_id(id: &str) -> bool {
    (5..=10).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Gallery URLs end in a slug: `/gallery/nitto-ridge-...-Pcc90bT`.
fn id_from_segment(segment: &str) -> Option<&str> {
    if valid_id(segment) {
        return Some(segment);
    }
    let (_, id) = segment.rsplit_once('-')?;
    valid_id(id).then_some(id)
}

/// The status id an imgur URL is saved under, such as `imgur:Pcc90bT`.
pub fn parse_status_id(input: &str) -> Option<String> {
    let url = Url::parse(input.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    let segs: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let id = match host {
        "i.imgur.com" => {
            let file = segs.first()?;
            let stem = file.rsplit_once('.').map_or(*file, |(stem, _)| stem);
            (!stem.is_empty()
                && stem.len() <= 40
                && stem
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
            .then_some(stem)?
        }
        "imgur.com" | "m.imgur.com" => match segs.as_slice() {
            ["gallery" | "a", rest @ ..] => id_from_segment(rest.last()?)?,
            [id] => {
                if !valid_id(id) {
                    return None;
                }
                id
            }
            _ => return None,
        },
        _ => return None,
    };
    Some(format!("{STATUS_PREFIX}{id}"))
}

pub fn post_id(status_id: &str) -> &str {
    status_id.strip_prefix(STATUS_PREFIX).unwrap_or(status_id)
}

/// A direct media file that needs no metadata lookup.
pub fn is_direct(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h == "i.imgur.com"))
        .unwrap_or(false)
}

fn is_gallery(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| {
            let first = u.path_segments()?.find(|s| !s.is_empty())?;
            Some(first == "gallery" || first == "a")
        })
        .unwrap_or(false)
}

fn is_album(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| {
            let first = u.path_segments()?.find(|s| !s.is_empty())?;
            Some(first == "a")
        })
        .unwrap_or(false)
}

/// One file of a post. `kind` is one of the media kinds arcman stores.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Media {
    pub url: String,
    pub kind: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
}

/// What arcman keeps of an imgur post. Saved as the snapshot.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Post {
    pub id: String,
    pub title: String,
    pub description: String,
    pub account: Option<String>,
    pub avatar: Option<String>,
    pub created_at: Option<i64>,
    pub views: Option<u64>,
    pub upvotes: Option<u64>,
    pub comments: Option<u64>,
    pub media: Vec<Media>,
}

impl Post {
    /// Title and description together, so keyword search finds either.
    pub fn text(&self) -> String {
        match self.description.trim() {
            "" => self.title.clone(),
            d => format!("{}\n{d}", self.title),
        }
    }
}

fn clean_url(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).to_string()
}

fn is_gif_url(url: &str) -> bool {
    let url = clean_url(url).to_ascii_lowercase();
    url.ends_with(".gif") || url.ends_with(".gifv")
}

/// Browsers play imgur's gifs from the smaller mp4 rendition.
fn mp4_url(url: &str) -> String {
    let url = clean_url(url);
    for ext in [".gifv", ".gif"] {
        if url.to_ascii_lowercase().ends_with(ext) {
            return format!("{}.mp4", &url[..url.len() - ext.len()]);
        }
    }
    url
}

/// Imgur names a video's poster after the id with an `h` suffix.
fn video_thumbnail(url: &str) -> Option<String> {
    let url = clean_url(url);
    let (dir, file) = url.rsplit_once('/')?;
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    Some(format!("{dir}/{stem}h.jpg"))
}

fn direct_post(url: &str, status_id: &str) -> Post {
    let clean = clean_url(url);
    let ext = clean
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    let (kind, url) = match ext.as_str() {
        "gifv" => ("gif", mp4_url(&clean)),
        "mp4" | "webm" | "mov" => ("video", clean.clone()),
        _ => ("photo", clean.clone()),
    };
    let title = clean.rsplit('/').next().unwrap_or_default().to_string();
    Post {
        id: post_id(status_id).to_string(),
        title,
        media: vec![Media {
            thumbnail: (kind != "photo").then(|| video_thumbnail(&url)).flatten(),
            url,
            kind: kind.to_string(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// The JSON imgur puts in the page for a gallery, album or single video.
fn post_data(html: &str) -> Option<Value> {
    let start = html.find(POST_DATA)? + POST_DATA.len();
    let rest = html[start..].trim_start();
    let end = rest.find("</script>")?;
    let literal = rest[..end].trim().trim_end_matches(';');
    // The value is a JavaScript string holding JSON: decode the string, then the JSON.
    let json: String = serde_json::from_str(literal).ok()?;
    serde_json::from_str(&json).ok()
}

fn media_from_data(media: &Value) -> Option<Media> {
    let url = media["url"].as_str()?.to_string();
    let mime = media["mime_type"].as_str().unwrap_or_default();
    let animated = media["metadata"]["is_animated"].as_bool().unwrap_or(false);
    let has_sound = media["metadata"]["has_sound"].as_bool().unwrap_or(false);
    let kind = match media["type"].as_str()? {
        "video" => {
            if animated && !has_sound {
                "gif"
            } else {
                "video"
            }
        }
        _ if mime.starts_with("image/") => "photo",
        _ => return None,
    };
    Some(Media {
        thumbnail: (kind != "photo").then(|| video_thumbnail(&url)).flatten(),
        duration: media["metadata"]["duration"].as_f64().filter(|d| *d > 0.0),
        width: media["width"].as_u64().and_then(|w| w.try_into().ok()),
        height: media["height"].as_u64().and_then(|h| h.try_into().ok()),
        kind: kind.to_string(),
        url,
    })
}

fn post_from_data(data: &Value, fallback_id: &str) -> Option<Post> {
    let media: Vec<Media> = data["media"]
        .as_array()?
        .iter()
        .filter_map(media_from_data)
        .collect();
    if media.is_empty() {
        return None;
    }
    Some(Post {
        id: data["id"].as_str().unwrap_or(fallback_id).to_string(),
        title: data["title"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string(),
        description: data["description"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string(),
        account: data["account"]["username"].as_str().map(str::to_string),
        avatar: data["account"]["avatar_url"].as_str().map(str::to_string),
        created_at: data["created_at"].as_str().and_then(pixiv::unix_time),
        views: data["view_count"].as_u64(),
        upvotes: data["upvote_count"].as_u64(),
        comments: data["comment_count"].as_u64(),
        media,
    })
}

fn meta_map(doc: &Html) -> HashMap<String, String> {
    let select = Selector::parse("meta[content]").expect("valid selector");
    let mut meta = HashMap::new();
    for el in doc.select(&select) {
        let value = el.value();
        if let Some(key) = value.attr("property").or_else(|| value.attr("name")) {
            meta.entry(key.trim().to_ascii_lowercase())
                .or_insert_with(|| value.attr("content").unwrap_or_default().to_string());
        }
    }
    meta
}

fn first(meta: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| meta.get(*k))
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
}

fn page_title(doc: &Html, meta: &HashMap<String, String>, id: &str) -> String {
    let title = first(meta, &["og:title"])
        .filter(|t| t != "imgur.com")
        .or_else(|| {
            let select = Selector::parse("title").expect("valid selector");
            doc.select(&select)
                .next()
                .map(|t| t.text().collect::<String>().trim().to_string())
        })
        .unwrap_or_default();
    let title = [" - GIF on Imgur", " - Album on Imgur", " - Imgur"]
        .iter()
        .find_map(|suffix| title.strip_suffix(suffix))
        .unwrap_or(&title);
    match title {
        "" | "Imgur: The magic of the Internet" => id.to_string(),
        t => t.to_string(),
    }
}

/// A single image or video page, without the gallery JSON.
fn post_from_page(html: &str, id: &str) -> Option<Post> {
    let doc = Html::parse_document(html);
    let meta = meta_map(&doc);
    let image = first(
        &meta,
        &[
            "og:image:secure_url",
            "og:image",
            "twitter:image",
            "twitter:image:src",
        ],
    )
    .map(|url| clean_url(&url));
    let video = first(
        &meta,
        &["og:video:secure_url", "og:video", "twitter:player:stream"],
    );
    let description = first(
        &meta,
        &["og:description", "twitter:description", "description"],
    )
    .filter(|d| !d.starts_with("Discover the magic of the internet at Imgur"))
    .unwrap_or_default();
    let media = if let Some(video) = video {
        let gif = is_gif_url(&video);
        Media {
            url: mp4_url(&video),
            kind: if gif { "gif" } else { "video" }.to_string(),
            width: first(&meta, &["og:video:width"]).and_then(|w| w.parse().ok()),
            height: first(&meta, &["og:video:height"]).and_then(|h| h.parse().ok()),
            thumbnail: image.clone(),
            duration: None,
        }
    } else {
        Media {
            url: image?,
            kind: "photo".to_string(),
            ..Default::default()
        }
    };
    Some(Post {
        id: id.to_string(),
        title: page_title(&doc, &meta, id),
        description,
        media: vec![media],
        ..Default::default()
    })
}

async fn get_page(http: &reqwest::Client, page_url: &str) -> Result<String, FetchError> {
    let res = http
        .get(page_url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|e| FetchError::Transport(format!("{page_url}: {e}")))?;
    match res.status().as_u16() {
        200..=299 => {}
        401 | 403 => return Err(FetchError::Unavailable("private")),
        404 | 410 => return Err(FetchError::Unavailable("not_found")),
        c => return Err(FetchError::Transport(format!("{page_url} returned {c}"))),
    }
    res.text()
        .await
        .map_err(|e| FetchError::Transport(format!("{page_url}: {e}")))
}

pub async fn fetch(
    http: &reqwest::Client,
    base_url: &str,
    url: &str,
    status_id: &str,
) -> Result<(Value, Post), FetchError> {
    let id = post_id(status_id);
    if is_direct(url) {
        let post = direct_post(url, status_id);
        let raw = serde_json::to_value(&post).expect("an imgur post serializes");
        return Ok((raw, post));
    }
    // `imgur.com/{id}` is the share form for single images but a gallery is only served
    // under `/gallery/{id}`, so the other form is tried when the page has no post data.
    let base = base_url.trim_end_matches('/');
    let mut paths = if is_album(url) {
        vec![format!("/a/{id}"), format!("/gallery/{id}")]
    } else if is_gallery(url) {
        vec![format!("/gallery/{id}"), format!("/a/{id}")]
    } else {
        vec![format!("/{id}"), format!("/gallery/{id}")]
    };
    paths.dedup();
    let mut last = FetchError::Unavailable("not_found");
    for path in paths {
        let page_url = format!("{base}{path}");
        match get_page(http, &page_url).await {
            Ok(html) => {
                if let Some(post) = post_data(&html)
                    .and_then(|data| post_from_data(&data, id))
                    .or_else(|| post_from_page(&html, id))
                {
                    let raw = serde_json::to_value(&post).expect("an imgur post serializes");
                    return Ok((raw, post));
                }
            }
            Err(e @ FetchError::Unavailable(_)) => last = e,
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn static_kind(kind: &str) -> &'static str {
    match kind {
        "video" => "video",
        "gif" => "gif",
        _ => "photo",
    }
}

/// Every file of the post, then the uploader's icon.
pub fn plan(post: &Post) -> Vec<Download> {
    let mut out: Vec<Download> = post
        .media
        .iter()
        .enumerate()
        .map(|(i, m)| Download {
            role: "main",
            position: i as i64,
            kind: static_kind(&m.kind),
            url: m.url.clone(),
            fallback_url: None,
            optional: false,
            thumbnail_url: m.thumbnail.clone(),
            width: m.width,
            height: m.height,
            duration: m.duration,
        })
        .collect();
    if let Some(url) = &post.avatar {
        out.push(Download {
            optional: true,
            ..Download::photo("avatar", 0, url)
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_the_post_id_from_gallery_album_and_image_urls() {
        for url in [
            "https://imgur.com/gallery/Pcc90bT",
            "https://imgur.com/gallery/nitto-ridge-grapplers-245-65-17-dirtcom-trailswing-Pcc90bT",
            "https://www.imgur.com/a/Pcc90bT",
            "https://m.imgur.com/Pcc90bT",
            "https://i.imgur.com/Pcc90bT.jpeg",
            "https://i.imgur.com/Pcc90bT.gifv",
        ] {
            assert_eq!(
                parse_status_id(url).as_deref(),
                Some("imgur:Pcc90bT"),
                "{url}"
            );
        }
    }

    #[test]
    fn rejects_urls_that_are_not_posts() {
        for url in [
            "https://imgur.com/",
            "https://imgur.com/gallery",
            "https://imgur.com/t/gifs",
            "https://imgur.com/user/someone",
            "https://example.com/gallery/Pcc90bT",
            "ftp://imgur.com/gallery/Pcc90bT",
        ] {
            assert_eq!(parse_status_id(url), None, "{url}");
        }
    }

    #[test]
    fn reads_the_gallery_json_and_keeps_every_image_and_video() {
        let data = json!({
            "id": "Pcc90bT",
            "title": "album title",
            "description": "album description",
            "created_at": "2025-08-15T05:03:21Z",
            "view_count": 10,
            "upvote_count": 3,
            "comment_count": 2,
            "account": {"username": "echo7tango", "avatar_url": "https://i.imgur.com/a_d.png"},
            "media": [
                {"id": "one", "type": "image", "mime_type": "image/jpeg",
                 "url": "https://i.imgur.com/one.jpeg", "width": 2048, "height": 1536, "metadata": {}},
                {"id": "two", "type": "video", "mime_type": "video/mp4",
                 "url": "https://i.imgur.com/two.mp4", "width": 1280, "height": 720,
                 "metadata": {"is_animated": true, "has_sound": false, "duration": 4.5}},
                {"id": "three", "type": "video", "mime_type": "video/mp4",
                 "url": "https://i.imgur.com/three.mp4", "width": 640, "height": 360,
                 "metadata": {"is_animated": false, "has_sound": true, "duration": 12.0}}
            ]
        });
        let post = post_from_data(&data, "Pcc90bT").unwrap();
        assert_eq!(post.text(), "album title\nalbum description");
        assert_eq!(post.account.as_deref(), Some("echo7tango"));
        assert_eq!(post.created_at, Some(1_755_234_201));
        let plan = plan(&post);
        assert_eq!(
            plan.iter()
                .filter(|d| d.role == "main")
                .map(|d| (d.role, d.kind, d.file_name(), d.duration))
                .collect::<Vec<_>>(),
            [
                ("main", "photo", "main-0.jpeg".to_string(), None),
                ("main", "gif", "main-1.mp4".to_string(), Some(4.5)),
                ("main", "video", "main-2.mp4".to_string(), Some(12.0)),
            ]
        );
        assert_eq!(
            plan[1].thumbnail_url.as_deref(),
            Some("https://i.imgur.com/twoh.jpg")
        );
        assert_eq!(plan[3].role, "avatar");
        assert!(plan[3].optional);
    }

    #[test]
    fn reads_the_json_out_of_the_script_tag() {
        let json = json!({"id": "abcde", "media": []}).to_string();
        let literal = serde_json::to_string(&json).unwrap();
        let html = format!("<html><script>window.postDataJSON={literal}</script></html>");
        assert_eq!(post_data(&html).unwrap()["id"], "abcde");
    }

    #[test]
    fn falls_back_to_the_open_graph_tags_for_a_single_page() {
        let html = r#"<html><head>
            <meta property="og:title" content="Greetings GIF Creators! (HUGE)">
            <meta property="og:description" content="Discover the magic of the internet at Imgur, a community powered entertainment destination.">
            <meta property="og:video" content="https://i.imgur.com/zvATqgs.gif">
            <meta property="og:video:width" content="660">
            <meta property="og:video:height" content="370">
            <meta property="og:image" content="https://i.imgur.com/zvATqgs.jpg?fbplay">
        </head></html>"#;
        let post = post_from_page(html, "zvATqgs").unwrap();
        assert_eq!(post.title, "Greetings GIF Creators! (HUGE)");
        assert_eq!(post.description, "");
        let [media] = post.media.as_slice() else {
            panic!("one media item")
        };
        assert_eq!(media.url, "https://i.imgur.com/zvATqgs.mp4");
        assert_eq!(media.kind, "gif");
        assert_eq!(
            media.thumbnail.as_deref(),
            Some("https://i.imgur.com/zvATqgs.jpg")
        );
        assert_eq!((media.width, media.height), (Some(660), Some(370)));
    }

    #[test]
    fn a_direct_media_link_needs_no_page() {
        let post = direct_post("https://i.imgur.com/abcde.gifv", "imgur:abcde");
        assert_eq!(post.media[0].url, "https://i.imgur.com/abcde.mp4");
        assert_eq!(post.media[0].kind, "gif");
        assert_eq!(
            post.media[0].thumbnail.as_deref(),
            Some("https://i.imgur.com/abcdeh.jpg")
        );
        let image = direct_post("https://i.imgur.com/abcde.png?x=1", "imgur:abcde");
        assert_eq!(image.media[0].url, "https://i.imgur.com/abcde.png");
        assert_eq!(image.media[0].kind, "photo");
    }
}
