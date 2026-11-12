use std::collections::HashMap;

use reqwest::Url;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{archive::Download, fxtwitter::FetchError, pixiv};

pub const DEFAULT_BASE_URL: &str = "https://www.reddit.com";

/// Reddit post ids are base36 strings, so they are saved under a prefix.
const STATUS_PREFIX: &str = "reddit:";

const HOSTS: [&str; 5] = [
    "reddit.com",
    "old.reddit.com",
    "new.reddit.com",
    "m.reddit.com",
    "redd.it",
];

const MEDIA_HOSTS: [&str; 4] = [
    "i.redd.it",
    "v.redd.it",
    "preview.redd.it",
    "external-preview.redd.it",
];

/// Reddit asks for a descriptive user agent; a bare one is often blocked.
const USER_AGENT: &str = concat!(
    "arcman/",
    env!("CARGO_PKG_VERSION"),
    " (self-hosted bookmark archiver; +https://github.com/kobago/arcman)"
);

fn valid_post_id(id: &str) -> bool {
    (4..=10).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn media_host(host: &str) -> bool {
    MEDIA_HOSTS.contains(&host)
}

/// The status id a reddit URL is saved under: `reddit:abc123` for posts,
/// `reddit:i.redd.it/photo.jpg` for a direct media link.
pub fn parse_status_id(input: &str) -> Option<String> {
    let url = Url::parse(input.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    let segs: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    if media_host(host) {
        let path = url.path().trim_end_matches('/');
        return (!path.is_empty()).then(|| format!("{STATUS_PREFIX}{host}{path}"));
    }
    if !HOSTS.contains(&host) {
        return None;
    }
    let id = match (host, segs.as_slice()) {
        (_, ["r", _, "comments", id, ..])
        | (_, ["comments", id, ..])
        | (_, ["gallery", id, ..])
        | (_, ["u" | "user", _, "comments", id, ..]) => id,
        ("redd.it", [id]) => id,
        _ => return None,
    };
    let id = id.to_ascii_lowercase();
    valid_post_id(&id).then(|| format!("{STATUS_PREFIX}{id}"))
}

pub fn post_id(status_id: &str) -> &str {
    status_id.strip_prefix(STATUS_PREFIX).unwrap_or(status_id)
}

pub fn is_direct(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(media_host))
        .unwrap_or(false)
}

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

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GalleryData {
    pub items: Vec<GalleryItem>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GalleryItem {
    pub media_id: String,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaMetadata {
    pub status: Option<String>,
    pub e: Option<String>,
    pub s: Option<MediaSource>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaSource {
    pub u: Option<String>,
    pub gif: Option<String>,
    pub mp4: Option<String>,
    pub x: Option<u32>,
    pub y: Option<u32>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaEmbed {
    pub reddit_video: Option<RedditVideo>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RedditVideo {
    pub fallback_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
    pub is_gif: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preview {
    pub images: Vec<PreviewImage>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewImage {
    pub source: Option<PreviewSource>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewSource {
    pub url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// What arcman keeps of a reddit post. Saved as the snapshot.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Post {
    pub id: String,
    pub title: String,
    pub selftext: String,
    pub subreddit: String,
    pub author: String,
    pub permalink: String,
    pub url: String,
    pub url_overridden_by_dest: Option<String>,
    pub created_utc: Option<f64>,
    pub score: Option<i64>,
    pub num_comments: Option<i64>,
    pub over_18: bool,
    pub is_video: bool,
    pub is_gallery: bool,
    pub post_hint: Option<String>,
    pub gallery_data: GalleryData,
    pub media_metadata: HashMap<String, MediaMetadata>,
    pub secure_media: Option<MediaEmbed>,
    pub preview: Option<Preview>,
    /// Media worked out from the RSS feed, a direct link or the post JSON.
    #[serde(alias = "direct_media")]
    pub resolved_media: Vec<Media>,
}

impl Post {
    /// Title and self text together, so keyword search finds either.
    pub fn text(&self) -> String {
        let selftext = self.selftext.trim();
        if selftext.is_empty()
            || selftext == "[removed]"
            || selftext == "[deleted]"
            || selftext == self.title.trim()
        {
            self.title.clone()
        } else {
            format!("{}\n{selftext}", self.title)
        }
    }

    pub fn created_at(&self) -> Option<i64> {
        self.created_utc.map(|t| t as i64)
    }

    pub fn permalink_url(&self) -> String {
        match self.permalink.as_str() {
            "" => self.url.clone(),
            p if p.starts_with("http") => p.to_string(),
            p => format!("https://www.reddit.com{p}"),
        }
    }

    /// The post's original image, when it is one.
    fn image_url(&self) -> Option<String> {
        let original = self
            .url_overridden_by_dest
            .as_deref()
            .filter(|u| !u.is_empty())
            .unwrap_or(&self.url);
        if is_image_url(original) {
            return Some(original.to_string());
        }
        // A link to an image host that reddit only shows through its preview.
        (self.post_hint.as_deref() == Some("image"))
            .then(|| self.preview_url())
            .flatten()
    }

    fn preview_url(&self) -> Option<String> {
        self.preview
            .as_ref()?
            .images
            .first()?
            .source
            .as_ref()?
            .url
            .as_deref()
            .map(unescape)
    }
}

/// Decodes the entities Reddit puts in its RSS feed and API URLs.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let decoded = after.find(';').filter(|end| *end <= 12).and_then(|end| {
            let entity = &after[1..end];
            let c = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" | "#x27" | "#X27" => Some('\''),
                e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                    .ok()
                    .and_then(char::from_u32),
                e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            }?;
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn is_image_url(url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    let path = url.path().to_ascii_lowercase();
    [".jpg", ".jpeg", ".png", ".gif", ".webp", ".bmp", ".avif"]
        .iter()
        .any(|ext| path.ends_with(ext))
}

fn video_id(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    (url.host_str() == Some("v.redd.it"))
        .then(|| {
            url.path_segments()?
                .find(|s| !s.is_empty())
                .map(str::to_string)
        })
        .flatten()
}

fn is_gallery_url(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| {
            let first = u.path_segments()?.find(|s| !s.is_empty())?;
            Some(first == "gallery")
        })
        .unwrap_or(false)
}

/// Reddit serves preview images at whatever size the query asks for.
fn upscale_preview(url: &str) -> String {
    let Ok(mut url) = Url::parse(url) else {
        return url.to_string();
    };
    let host = url.host_str().unwrap_or_default();
    if host != "preview.redd.it" && host != "external-preview.redd.it" {
        return url.to_string();
    }
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "crop" && k != "height" && k != "auto" && k != "width")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .chain([("width".to_string(), "1080".to_string())])
        .collect();
    url.query_pairs_mut().clear().extend_pairs(pairs);
    url.into()
}

fn static_kind(kind: &str) -> &'static str {
    match kind {
        "video" => "video",
        "gif" => "gif",
        _ => "photo",
    }
}

fn download(position: usize, media: Media) -> Download {
    Download {
        role: "main",
        position: position as i64,
        kind: static_kind(&media.kind),
        url: media.url,
        fallback_url: None,
        optional: false,
        thumbnail_url: media.thumbnail,
        width: media.width,
        height: media.height,
        duration: media.duration,
    }
}

fn gallery_media(post: &Post) -> Vec<Media> {
    post.gallery_data
        .items
        .iter()
        .filter_map(|item| {
            let metadata = post.media_metadata.get(&item.media_id)?;
            if matches!(metadata.status.as_deref(), Some("failed" | "invalid")) {
                return None;
            }
            let source = metadata.s.as_ref()?;
            let animated = metadata.e.as_deref() == Some("AnimatedImage");
            let (url, kind) = match (animated, &source.mp4) {
                (true, Some(mp4)) => (unescape(mp4), "gif"),
                // A plain gif animates in an <img>, so it is kept as a photo.
                (true, None) => (
                    unescape(source.gif.as_ref().or(source.u.as_ref())?),
                    "photo",
                ),
                _ => (unescape(source.u.as_ref()?), "photo"),
            };
            Some(Media {
                url,
                kind: kind.to_string(),
                width: source.x,
                height: source.y,
                duration: None,
                thumbnail: None,
            })
        })
        .collect()
}

fn normalize_video_url(url: &str) -> String {
    let Ok(parsed) = Url::parse(url) else {
        return url.to_string();
    };
    if parsed.host_str() != Some("v.redd.it") {
        return url.to_string();
    }
    let last = parsed
        .path_segments()
        .and_then(|mut s| s.rfind(|s| !s.is_empty()));
    match last {
        Some(name) if name.contains('.') => url.to_string(),
        _ => format!("{}/DASH_720.mp4", url.trim_end_matches('/')),
    }
}

fn video_media(post: &Post) -> Option<Media> {
    let video = post.secure_media.as_ref()?.reddit_video.as_ref()?;
    let url = video.fallback_url.as_deref()?;
    Some(Media {
        url: normalize_video_url(url),
        kind: if video.is_gif { "gif" } else { "video" }.to_string(),
        width: video.width,
        height: video.height,
        duration: video.duration.filter(|d| *d > 0.0),
        thumbnail: post.preview_url(),
    })
}

fn single_image_media(post: &Post) -> Option<Media> {
    let url = post.image_url()?;
    let source = post.preview.as_ref()?.images.first()?.source.as_ref();
    Some(Media {
        url,
        kind: "photo".to_string(),
        width: source.and_then(|s| s.width),
        height: source.and_then(|s| s.height),
        duration: None,
        thumbnail: None,
    })
}

fn attr_in_tag<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    // The leading space keeps `width` from matching inside `bandwidth`.
    let key = format!(" {attr}=");
    let pos = tag.find(&key)? + key.len();
    let after = &tag[pos..];
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &after[1..];
    let end = value.find(quote)?;
    Some(&value[..end])
}

struct Rendition {
    url: String,
    width: u32,
    height: u32,
    bandwidth: u64,
}

/// The video renditions a DASH manifest offers, with the presentation duration.
fn mpd_renditions(mpd: &str, base: &str) -> (Vec<Rendition>, Option<f64>) {
    let duration = attr_in_tag(
        mpd.get(..mpd.find('>').unwrap_or(mpd.len())).unwrap_or(""),
        "mediaPresentationDuration",
    )
    .and_then(iso_duration);
    let mut out = vec![];
    let mut rest = mpd;
    while let Some(start) = rest.find("<Representation") {
        let after = &rest[start..];
        let Some(tag_end) = after.find('>') else {
            break;
        };
        let tag = &after[..tag_end];
        let body = &after[tag_end + 1..];
        rest = body;
        let mime = attr_in_tag(tag, "mimeType").unwrap_or_default();
        if !mime.contains("video/mp4") {
            continue;
        }
        let Some(open) = body.find("<BaseURL>") else {
            continue;
        };
        let Some(close) = body[open..].find("</BaseURL>") else {
            continue;
        };
        let file = &body[open + "<BaseURL>".len()..open + close];
        let Ok(url) = Url::parse(base).and_then(|b| b.join(file)) else {
            continue;
        };
        let width = attr_in_tag(tag, "width")
            .and_then(|w| w.parse().ok())
            .unwrap_or(0);
        let height = attr_in_tag(tag, "height")
            .and_then(|h| h.parse().ok())
            .unwrap_or(0);
        let bandwidth = attr_in_tag(tag, "bandwidth")
            .and_then(|b| b.parse().ok())
            .unwrap_or(0);
        out.push(Rendition {
            url: url.into(),
            width,
            height,
            bandwidth,
        });
    }
    (out, duration)
}

fn iso_duration(s: &str) -> Option<f64> {
    let s = s.strip_prefix("PT")?;
    let mut seconds = 0.0;
    let mut number = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() || c == '.' {
            number.push(c);
            continue;
        }
        let value: f64 = number.parse().ok()?;
        number.clear();
        match c {
            'H' => seconds += value * 3600.0,
            'M' => seconds += value * 60.0,
            'S' => seconds += value,
            _ => return None,
        }
    }
    (seconds > 0.0).then_some(seconds)
}

/// Reddit's JSON fallback URL (`DASH_720.mp4`) is often refused now, while the
/// files listed in the DASH manifest are served.
async fn fetch_mpd(http: &reqwest::Client, id: &str) -> Option<Media> {
    let url = format!("https://v.redd.it/{id}/DASHPlaylist.mpd");
    let res = http
        .get(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    let mpd = res.text().await.ok()?;
    let base = format!("https://v.redd.it/{id}/");
    let (renditions, duration) = mpd_renditions(&mpd, &base);
    let best = renditions
        .into_iter()
        .max_by_key(|r| (r.width * r.height, r.bandwidth))?;
    Some(Media {
        url: best.url,
        kind: "video".to_string(),
        width: (best.width > 0).then_some(best.width),
        height: (best.height > 0).then_some(best.height),
        duration,
        thumbnail: None,
    })
}

async fn resolve_video(http: &reqwest::Client, post: &Post) -> Option<Media> {
    let fallback = post
        .secure_media
        .as_ref()?
        .reddit_video
        .as_ref()?
        .fallback_url
        .clone()?;
    let thumbnail = post.preview_url();
    let resolved = match video_id(&fallback) {
        Some(id) => fetch_mpd(http, &id).await,
        None => None,
    };
    let mut media = match (resolved, video_media(post)) {
        (Some(mut media), _) => {
            media.thumbnail = thumbnail;
            media
        }
        (None, Some(media)) => media,
        (None, None) => return None,
    };
    if media.duration.is_none() {
        media.duration = post
            .secure_media
            .as_ref()
            .and_then(|m| m.reddit_video.as_ref())
            .and_then(|v| v.duration)
            .filter(|d| *d > 0.0);
    }
    Some(media)
}

async fn resolve_json_media(http: &reqwest::Client, post: &Post) -> Vec<Media> {
    if post.is_video {
        return resolve_video(http, post).await.into_iter().collect();
    }
    if post.is_gallery {
        let gallery = gallery_media(post);
        if !gallery.is_empty() {
            return gallery;
        }
        if let Some(cover) = post.preview_url() {
            return vec![Media {
                url: upscale_preview(&cover),
                kind: "photo".into(),
                ..Default::default()
            }];
        }
    }
    single_image_media(post).into_iter().collect()
}

/// Every file of the post.
pub fn plan(post: &Post) -> Vec<Download> {
    if !post.resolved_media.is_empty() {
        return post
            .resolved_media
            .clone()
            .into_iter()
            .enumerate()
            .map(|(i, m)| download(i, m))
            .collect();
    }
    let gallery = if post.is_gallery {
        gallery_media(post)
    } else {
        vec![]
    };
    let media = if !gallery.is_empty() {
        gallery
    } else if post.is_video {
        video_media(post).into_iter().collect()
    } else {
        single_image_media(post).into_iter().collect()
    };
    media
        .into_iter()
        .enumerate()
        .map(|(i, m)| download(i, m))
        .collect()
}

fn direct_post(url: &str, status_id: &str) -> Post {
    let clean = url.split(['?', '#']).next().unwrap_or(url);
    let video_link = Url::parse(clean).ok().is_some_and(|u| {
        let segs = u
            .path_segments()
            .map(|s| s.filter(|s| !s.is_empty()).count())
            .unwrap_or(0);
        u.host_str() == Some("v.redd.it") && segs == 1
    });
    let title = clean.rsplit('/').next().unwrap_or_default().to_string();
    let mut post = Post {
        id: post_id(status_id).to_string(),
        title,
        permalink: clean.to_string(),
        url: clean.to_string(),
        ..Default::default()
    };
    if video_link {
        post.is_video = true;
        post.secure_media = Some(MediaEmbed {
            reddit_video: Some(RedditVideo {
                fallback_url: Some(clean.to_string()),
                ..Default::default()
            }),
        });
        return post;
    }
    let ext = clean
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    let kind = match ext.as_str() {
        "mp4" | "webm" | "mov" => "video",
        _ => "photo",
    };
    post.resolved_media = vec![Media {
        url: clean.to_string(),
        kind: kind.to_string(),
        ..Default::default()
    }];
    post
}

fn parse_body(body: &Value) -> Option<Value> {
    Some(
        body.get(0)?
            .get("data")?
            .get("children")?
            .get(0)?
            .get("data")?
            .clone(),
    )
}

fn cookie_header(session: Option<&str>) -> String {
    match session {
        Some(session) => format!("reddit_session={session}; over18=1"),
        None => "over18=1".to_string(),
    }
}

async fn get_json(
    http: &reqwest::Client,
    base_url: &str,
    id: &str,
    session: Option<&str>,
) -> Result<Value, FetchError> {
    let api = format!(
        "{}/comments/{id}.json?raw_json=1&limit=1",
        base_url.trim_end_matches('/')
    );
    let res = http
        .get(&api)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::COOKIE, cookie_header(session))
        .send()
        .await
        .map_err(|e| FetchError::Transport(format!("{api}: {e}")))?;
    match res.status().as_u16() {
        200 => {}
        401 | 403 => return Err(FetchError::Unavailable("private")),
        404 | 410 => return Err(FetchError::Unavailable("not_found")),
        c => return Err(FetchError::Transport(format!("{api} returned {c}"))),
    }
    let body: Value = res
        .json()
        .await
        .map_err(|e| FetchError::Transport(format!("unexpected response: {e}")))?;
    parse_body(&body).ok_or_else(|| FetchError::Transport("unexpected response: no post".into()))
}

async fn get_rss(
    http: &reqwest::Client,
    base_url: &str,
    id: &str,
    session: Option<&str>,
) -> Result<String, FetchError> {
    let api = format!("{}/comments/{id}.rss", base_url.trim_end_matches('/'));
    let res = http
        .get(&api)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(
            reqwest::header::ACCEPT,
            "application/atom+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header(reqwest::header::COOKIE, cookie_header(session))
        .send()
        .await
        .map_err(|e| FetchError::Transport(format!("{api}: {e}")))?;
    match res.status().as_u16() {
        200 => {}
        401 | 403 => return Err(FetchError::Unavailable("private")),
        404 | 410 => return Err(FetchError::Unavailable("not_found")),
        c => return Err(FetchError::Transport(format!("{api} returned {c}"))),
    }
    res.text()
        .await
        .map_err(|e| FetchError::Transport(format!("{api}: {e}")))
}

fn tag_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = xml.find(&open)?;
    let rest = &xml[start + open.len()..];
    let open_end = rest.find('>')?;
    let content = &rest[open_end + 1..];
    let close = format!("</{tag}>");
    let end = content.find(&close)?;
    Some(unescape(&content[..end]))
}

fn tag_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = xml.find(&open)?;
    let rest = &xml[start + open.len()..];
    let end = rest.find('>')?;
    attr_in_tag(&rest[..end], attr).map(unescape)
}

fn first_entry(xml: &str) -> Option<&str> {
    let start = xml.find("<entry>")?;
    let body = &xml[start + "<entry>".len()..];
    let end = body.find("</entry>")?;
    Some(&body[..end])
}

fn media_host_url(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(media_host))
        .unwrap_or(false)
}

/// The body text, the `[link]` target, the preview and any images in the body.
fn rss_content(content: &str) -> (String, Option<String>, Option<String>, Vec<String>) {
    let doc = Html::parse_fragment(content);
    let anchor = Selector::parse("a[href]").expect("valid selector");
    let image = Selector::parse("img[src]").expect("valid selector");
    let body = Selector::parse("div.md").expect("valid selector");
    let link = doc.select(&anchor).find_map(|el| {
        (el.text().collect::<String>().trim() == "[link]")
            .then(|| el.value().attr("href").map(unescape))
            .flatten()
    });
    let preview = doc
        .select(&image)
        .next()
        .and_then(|el| el.value().attr("src"))
        .map(unescape);
    let Some(md) = doc.select(&body).next() else {
        return (String::new(), link, preview, vec![]);
    };
    let text = md
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut inline = vec![];
    for el in md.select(&anchor) {
        if let Some(href) = el.value().attr("href").map(unescape)
            && media_host_url(&href)
            && !href.contains("v.redd.it")
        {
            inline.push(upscale_preview(&href));
        }
    }
    for el in md.select(&image) {
        if let Some(src) = el.value().attr("src").map(unescape) {
            inline.push(upscale_preview(&src));
        }
    }
    inline.dedup();
    (text, link, preview, inline)
}

fn post_from_rss(xml: &str, fallback_id: &str) -> Option<Post> {
    let entry = first_entry(xml)?;
    let id = tag_text(entry, "id")
        .map(|id| id.strip_prefix("t3_").unwrap_or(&id).to_string())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| fallback_id.to_string());
    let title = tag_text(entry, "title").unwrap_or_else(|| id.clone());
    let permalink = tag_attr(entry, "link", "href").unwrap_or_default();
    let author = tag_text(entry, "name")
        .unwrap_or_default()
        .trim_start_matches("/u/")
        .trim_start_matches("u/")
        .to_string();
    let subreddit = tag_attr(entry, "category", "term").unwrap_or_default();
    let created_utc = tag_text(entry, "published")
        .or_else(|| tag_text(entry, "updated"))
        .and_then(|s| pixiv::unix_time(&s))
        .map(|t| t as f64);
    let content = tag_text(entry, "content").unwrap_or_default();
    let thumbnail = tag_attr(entry, "media:thumbnail", "url");
    let (selftext, link, preview, inline) = rss_content(&content);
    let preview = preview.or(thumbnail);
    let mut post = Post {
        id,
        title,
        selftext,
        subreddit,
        author,
        permalink: permalink.clone(),
        url: permalink,
        url_overridden_by_dest: link.clone(),
        created_utc,
        preview: preview.map(|url| Preview {
            images: vec![PreviewImage {
                source: Some(PreviewSource {
                    url: Some(url),
                    ..Default::default()
                }),
            }],
        }),
        ..Default::default()
    };
    match link.as_deref() {
        Some(link) if is_image_url(link) => {
            post.post_hint = Some("image".into());
            post.resolved_media = vec![Media {
                url: link.to_string(),
                kind: "photo".into(),
                ..Default::default()
            }];
        }
        Some(link) if Url::parse(link).is_ok_and(|u| u.host_str() == Some("v.redd.it")) => {
            post.is_video = true;
            post.post_hint = Some("hosted:video".into());
            post.secure_media = Some(MediaEmbed {
                reddit_video: Some(RedditVideo {
                    fallback_url: Some(link.to_string()),
                    ..Default::default()
                }),
            });
        }
        Some(link) if is_gallery_url(link) => {
            post.is_gallery = true;
            post.post_hint = Some("gallery".into());
            if let Some(cover) = post.preview_url() {
                post.resolved_media = vec![Media {
                    url: upscale_preview(&cover),
                    kind: "photo".into(),
                    ..Default::default()
                }];
            }
        }
        _ if !inline.is_empty() => {
            post.resolved_media = inline
                .into_iter()
                .map(|url| Media {
                    url,
                    kind: "photo".into(),
                    ..Default::default()
                })
                .collect();
        }
        _ => {}
    }
    Some(post)
}

pub async fn fetch(
    http: &reqwest::Client,
    base_url: &str,
    url: &str,
    status_id: &str,
    session: Option<&str>,
) -> Result<(Value, Post), FetchError> {
    if is_direct(url) {
        let mut post = direct_post(url, status_id);
        if post.is_video {
            post.resolved_media = resolve_video(http, &post).await.into_iter().collect();
        }
        let raw = serde_json::to_value(&post).expect("a reddit post serializes");
        return Ok((raw, post));
    }
    let id = post_id(status_id);
    // Logged-in `.json` has the full gallery and video data, but Reddit refuses
    // unauthenticated JSON now, so the public RSS feed is the fallback.
    if let Some(session) = session {
        match get_json(http, base_url, id, Some(session)).await {
            Ok(raw) => {
                if !raw["removed_by_category"].is_null() {
                    return Err(FetchError::Unavailable("not_found"));
                }
                let mut post: Post = serde_json::from_value(raw)
                    .map_err(|e| FetchError::Transport(format!("unexpected response: {e}")))?;
                post.resolved_media = resolve_json_media(http, &post).await;
                let raw = serde_json::to_value(&post).expect("a reddit post serializes");
                return Ok((raw, post));
            }
            Err(FetchError::Unavailable("not_found")) => {
                return Err(FetchError::Unavailable("not_found"));
            }
            // A rejected or expired session still leaves the public feed.
            Err(_) => {}
        }
    }
    let xml = match get_rss(http, base_url, id, session).await {
        Ok(xml) => xml,
        // A stale session cookie can be worse than none; try the anonymous feed once.
        Err(FetchError::Unavailable("private")) if session.is_some() => {
            get_rss(http, base_url, id, None).await?
        }
        Err(e) => return Err(e),
    };
    let mut post = post_from_rss(&xml, id).ok_or(FetchError::Unavailable("not_found"))?;
    if post.is_video {
        post.resolved_media = resolve_video(http, &post).await.into_iter().collect();
    }
    let raw = serde_json::to_value(&post).expect("a reddit post serializes");
    Ok((raw, post))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_the_post_id_from_every_supported_url_form() {
        for url in [
            "https://www.reddit.com/r/pics/comments/abc123/a_title/",
            "https://old.reddit.com/r/pics/comments/abc123/",
            "https://reddit.com/comments/abc123",
            "https://www.reddit.com/gallery/abc123",
            "https://www.reddit.com/user/alice/comments/abc123/x/",
            "https://redd.it/abc123",
            "https://www.reddit.com/r/pics/comments/ABC123/",
        ] {
            assert_eq!(
                parse_status_id(url).as_deref(),
                Some("reddit:abc123"),
                "{url}"
            );
        }
    }

    #[test]
    fn keeps_a_direct_media_url_under_its_own_id() {
        assert_eq!(
            parse_status_id("https://i.redd.it/photo.jpg").as_deref(),
            Some("reddit:i.redd.it/photo.jpg")
        );
        assert_eq!(
            parse_status_id("https://v.redd.it/abc123/DASH_720.mp4?source=fallback").as_deref(),
            Some("reddit:v.redd.it/abc123/DASH_720.mp4")
        );
    }

    #[test]
    fn rejects_urls_that_are_not_posts() {
        for url in [
            "https://www.reddit.com/",
            "https://www.reddit.com/r/pics/",
            "https://www.reddit.com/r/pics/s/abc123",
            "https://example.com/r/pics/comments/abc123/",
            "ftp://redd.it/abc123",
        ] {
            assert_eq!(parse_status_id(url), None, "{url}");
        }
    }

    #[test]
    fn plans_a_gallery_in_its_display_order() {
        let post: Post = serde_json::from_value(json!({
            "id": "abc123",
            "title": "cats",
            "selftext": "[removed]",
            "is_gallery": true,
            "gallery_data": {"items": [{"media_id": "one"}, {"media_id": "two"}, {"media_id": "gone"}]},
            "media_metadata": {
                "one": {"status": "valid", "e": "Image", "s": {"u": "https://preview.redd.it/a.jpg?format=pjpg&amp;s=1", "x": 800, "y": 600}},
                "two": {"status": "valid", "e": "AnimatedImage", "s": {"u": "https://preview.redd.it/b.gif", "mp4": "https://preview.redd.it/b.mp4?format=mp4&amp;s=2", "x": 400, "y": 300}},
                "gone": {"status": "failed"}
            }
        }))
        .unwrap();
        assert_eq!(post.text(), "cats");
        let plan = plan(&post);
        assert_eq!(
            plan.iter()
                .map(|d| (d.kind, d.file_name(), d.width, d.height))
                .collect::<Vec<_>>(),
            [
                ("photo", "main-0.jpg".to_string(), Some(800), Some(600)),
                ("gif", "main-1.mp4".to_string(), Some(400), Some(300)),
            ]
        );
        assert_eq!(plan[0].url, "https://preview.redd.it/a.jpg?format=pjpg&s=1");
        assert_eq!(plan[1].url, "https://preview.redd.it/b.mp4?format=mp4&s=2");
    }

    #[test]
    fn plans_a_hosted_video_with_its_poster() {
        let post: Post = serde_json::from_value(json!({
            "id": "abc123",
            "title": "waves",
            "is_video": true,
            "secure_media": {"reddit_video": {
                "fallback_url": "https://v.redd.it/abc123/DASH_720.mp4?source=fallback",
                "width": 1280, "height": 720, "duration": 25.5, "is_gif": false
            }},
            "preview": {"images": [{"source": {"url": "https://preview.redd.it/p.jpg?width=1280&amp;s=3", "width": 1280, "height": 720}}]}
        }))
        .unwrap();
        let [media] = plan(&post).try_into().unwrap();
        assert_eq!(media.kind, "video");
        assert_eq!(
            media.url,
            "https://v.redd.it/abc123/DASH_720.mp4?source=fallback"
        );
        assert_eq!(media.duration, Some(25.5));
        assert_eq!(
            media.thumbnail_url.as_deref(),
            Some("https://preview.redd.it/p.jpg?width=1280&s=3")
        );
    }

    #[test]
    fn plans_a_single_image_post() {
        let post: Post = serde_json::from_value(json!({
            "id": "abc123",
            "title": "one photo",
            "post_hint": "image",
            "url_overridden_by_dest": "https://i.redd.it/photo.jpg",
            "preview": {"images": [{"source": {"url": "https://preview.redd.it/p.jpg?width=1080&amp;s=4", "width": 1080, "height": 1920}}]}
        }))
        .unwrap();
        let [media] = plan(&post).try_into().unwrap();
        assert_eq!(media.kind, "photo");
        assert_eq!(media.url, "https://i.redd.it/photo.jpg");
        assert_eq!((media.width, media.height), (Some(1080), Some(1920)));
    }

    #[test]
    fn a_direct_media_link_needs_no_post() {
        let post = direct_post("https://i.redd.it/photo.jpg", "reddit:i.redd.it/photo.jpg");
        let [media] = plan(&post).try_into().unwrap();
        assert_eq!(media.kind, "photo");
        assert_eq!(media.url, "https://i.redd.it/photo.jpg");
    }

    #[test]
    fn posts_are_known_by_their_permalink() {
        let post = Post {
            permalink: "/r/pics/comments/abc123/a_title/".into(),
            url: "https://www.reddit.com/r/pics/comments/abc123/a_title/".into(),
            ..Default::default()
        };
        assert_eq!(
            post.permalink_url(),
            "https://www.reddit.com/r/pics/comments/abc123/a_title/"
        );
    }

    #[test]
    fn reads_a_public_post_feed_into_a_post_with_its_image() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><feed xmlns="http://www.w3.org/2005/Atom" xmlns:media="http://search.yahoo.com/mrss/">
<entry><author><name>/u/shdusns82</name></author><category term="celebnsfw" label="r/celebnsfw"/>
<content type="html">&lt;table&gt; &lt;tr&gt;&lt;td&gt; &lt;a href=&quot;https://www.reddit.com/r/celebnsfw/comments/1h74e1v/elizabeth_olsen/&quot;&gt; &lt;img src=&quot;https://preview.redd.it/cover.jpeg?width=640&amp;amp;crop=smart&amp;amp;auto=webp&amp;amp;s=1&quot; alt=&quot;Elizabeth Olsen&quot; /&gt; &lt;/a&gt; &lt;/td&gt;&lt;td&gt; &lt;!-- SC_OFF --&gt;&lt;div class=&quot;md&quot;&gt;&lt;p&gt;Elizabeth Olsen&lt;/p&gt;&lt;/div&gt;&lt;!-- SC_ON --&gt; &amp;#32; submitted by &amp;#32; &lt;a href=&quot;https://www.reddit.com/user/shdusns82&quot;&gt; /u/shdusns82 &lt;/a&gt; &lt;br/&gt; &lt;span&gt;&lt;a href=&quot;https://i.redd.it/sqv9z9zpnz4e1.jpeg&quot;&gt;[link]&lt;/a&gt;&lt;/span&gt; &lt;/td&gt;&lt;/tr&gt;&lt;/table&gt;</content>
<id>t3_1h74e1v</id><media:thumbnail url="https://preview.redd.it/cover.jpeg?width=640&amp;amp;s=1" />
<link href="https://www.reddit.com/r/celebnsfw/comments/1h74e1v/elizabeth_olsen/" />
<updated>2024-12-05T08:13:22+00:00</updated><published>2024-12-05T08:13:22+00:00</published><title>Elizabeth Olsen</title></entry></feed>"#;
        let post = post_from_rss(xml, "1h74e1v").unwrap();
        assert_eq!(post.id, "1h74e1v");
        assert_eq!(post.title, "Elizabeth Olsen");
        assert_eq!(post.text(), "Elizabeth Olsen");
        assert_eq!(post.author, "shdusns82");
        assert_eq!(post.subreddit, "celebnsfw");
        assert_eq!(post.created_at(), Some(1_733_386_402));
        assert_eq!(
            post.permalink_url(),
            "https://www.reddit.com/r/celebnsfw/comments/1h74e1v/elizabeth_olsen/"
        );
        let plan = plan(&post);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].kind, "photo");
        assert_eq!(plan[0].url, "https://i.redd.it/sqv9z9zpnz4e1.jpeg");
    }

    #[test]
    fn a_gallery_feed_only_has_a_preview_so_it_is_saved_as_one_photo() {
        let xml = r#"<feed><entry><author><name>/u/alice</name></author><category term="pics" label="r/pics"/>
<content type="html">&lt;table&gt;&lt;tr&gt;&lt;td&gt;&lt;a href=&quot;https://www.reddit.com/r/pics/comments/abc123/x/&quot;&gt;&lt;img src=&quot;https://preview.redd.it/a.jpeg?width=140&amp;amp;height=140&amp;amp;crop=1:1,smart&amp;amp;auto=webp&amp;amp;s=2&quot; /&gt;&lt;/a&gt;&lt;/td&gt;&lt;td&gt;&lt;span&gt;&lt;a href=&quot;https://www.reddit.com/gallery/abc123&quot;&gt;[link]&lt;/a&gt;&lt;/span&gt;&lt;/td&gt;&lt;/tr&gt;&lt;/table&gt;</content>
<id>t3_abc123</id><link href="https://www.reddit.com/r/pics/comments/abc123/x/" /><published>2026-11-12T08:19:37+00:00</published><title>photos</title></entry></feed>"#;
        let post = post_from_rss(xml, "abc123").unwrap();
        assert!(post.is_gallery);
        let plan = plan(&post);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].kind, "photo");
        assert_eq!(plan[0].url, "https://preview.redd.it/a.jpeg?s=2&width=1080");
    }

    #[test]
    fn reads_the_best_video_from_a_dash_manifest() {
        let mpd = r#"<MPD mediaPresentationDuration="PT1M34S"><Period>
            <AdaptationSet contentType="video">
                <Representation bandwidth="258632" height="392" mimeType="video/mp4" width="220"><BaseURL>CMAF_220.mp4</BaseURL></Representation>
                <Representation bandwidth="824338" height="640" mimeType="video/mp4" width="360"><BaseURL>CMAF_360.mp4</BaseURL></Representation>
                <Representation bandwidth="132979" mimeType="audio/mp4"><BaseURL>CMAF_AUDIO_128.mp4</BaseURL></Representation>
            </AdaptationSet></Period></MPD>"#;
        let (renditions, duration) = mpd_renditions(mpd, "https://v.redd.it/abc123/");
        assert_eq!(duration, Some(94.0));
        let best = renditions
            .into_iter()
            .max_by_key(|r| (r.width * r.height, r.bandwidth))
            .unwrap();
        assert_eq!(best.url, "https://v.redd.it/abc123/CMAF_360.mp4");
        assert_eq!((best.width, best.height), (360, 640));
    }

    #[test]
    fn a_video_page_url_falls_back_to_the_default_mp4() {
        assert_eq!(
            normalize_video_url("https://v.redd.it/abc123"),
            "https://v.redd.it/abc123/DASH_720.mp4"
        );
        assert_eq!(
            normalize_video_url("https://v.redd.it/abc123/DASH_720.mp4?source=fallback"),
            "https://v.redd.it/abc123/DASH_720.mp4?source=fallback"
        );
        assert_eq!(
            normalize_video_url("https://i.redd.it/photo.jpg"),
            "https://i.redd.it/photo.jpg"
        );
    }

    #[test]
    fn preview_links_are_asked_for_at_a_printable_size() {
        assert_eq!(
            upscale_preview(
                "https://preview.redd.it/a.jpeg?width=140&height=140&crop=1:1,smart&auto=webp&s=2"
            ),
            "https://preview.redd.it/a.jpeg?s=2&width=1080"
        );
        assert_eq!(
            upscale_preview("https://i.redd.it/a.jpeg"),
            "https://i.redd.it/a.jpeg"
        );
    }
}
