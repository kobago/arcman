use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const HOSTS: [&str; 6] = [
    "x.com",
    "twitter.com",
    "mobile.twitter.com",
    "fixupx.com",
    "fxtwitter.com",
    "vxtwitter.com",
];

pub fn parse_status_id(input: &str) -> Option<String> {
    let url = Url::parse(input.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    if !HOSTS.contains(&host) {
        return None;
    }
    let segs: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let id = match segs.as_slice() {
        [_, "status" | "statuses", id, ..] | ["i", "web", "status", id, ..] => *id,
        _ => return None,
    };
    (!id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit()))
        .then(|| id.to_string())
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Status {
    pub id: String,
    pub url: String,
    pub text: String,
    pub lang: Option<String>,
    pub created_timestamp: Option<i64>,
    pub author: Author,
    pub likes: Option<u64>,
    pub reposts: Option<u64>,
    pub quotes: Option<u64>,
    pub replies: Option<u64>,
    pub views: Option<u64>,
    pub bookmarks: Option<u64>,
    pub media: MediaSet,
    pub quote: Option<Value>,
    pub card: Option<Card>,
    pub poll: Option<Value>,
    pub community_note: Option<Note>,
    pub translation: Option<Translation>,
    pub article: Option<Value>,
}

impl Status {
    pub fn quoted(&self) -> Option<Result<Status, String>> {
        let q = self.quote.as_ref()?;
        if q.get("type").and_then(Value::as_str) == Some("status") {
            Some(serde_json::from_value(q.clone()).map_err(|e| e.to_string()))
        } else {
            let msg = q.get("message").and_then(Value::as_str).unwrap_or_default();
            Some(Err(msg.to_string()))
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Author {
    pub screen_name: String,
    pub name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct MediaSet {
    pub all: Vec<MediaItem>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct MediaItem {
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub thumbnail_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
    pub formats: Vec<Format>,
}

impl MediaItem {
    pub fn best_mp4(&self) -> &str {
        self.formats
            .iter()
            .filter(|f| f.container.as_deref() == Some("mp4"))
            .max_by_key(|f| f.bitrate.unwrap_or(0))
            .map_or(&self.url, |f| &f.url)
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Format {
    pub url: String,
    pub container: Option<String>,
    pub bitrate: Option<u64>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Card {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub domain: Option<String>,
    pub image: Option<CardImage>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct CardImage {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Note {
    pub text: String,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Translation {
    pub text: String,
    pub source_lang: Option<String>,
}

#[derive(Debug)]
pub enum FetchError {
    Unavailable(&'static str),
    Transport(String),
}

pub async fn fetch(
    http: &reqwest::Client,
    base: &str,
    status_id: &str,
) -> Result<(Value, Status), FetchError> {
    let url = format!(
        "{}/2/status/{status_id}?lang=ja",
        base.trim_end_matches('/')
    );
    let res = http
        .get(url)
        .send()
        .await
        .map_err(|e| FetchError::Transport(e.to_string()))?;
    let http_status = res.status().as_u16();
    let body: Value = res
        .json()
        .await
        .map_err(|e| FetchError::Transport(format!("HTTP {http_status}: {e}")))?;
    let code = body
        .get("code")
        .and_then(Value::as_u64)
        .unwrap_or(http_status as u64);
    match code {
        200 => {
            let raw = body.get("status").cloned().unwrap_or(Value::Null);
            let status = serde_json::from_value(raw.clone())
                .map_err(|e| FetchError::Transport(format!("unexpected response: {e}")))?;
            Ok((raw, status))
        }
        404 => Err(FetchError::Unavailable("not_found")),
        401 | 403 => Err(FetchError::Unavailable("private")),
        c => Err(FetchError::Transport(format!("API returned {c}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_status_id_from_every_supported_host() {
        for url in [
            "https://x.com/jack/status/20",
            "https://twitter.com/jack/status/20?s=20",
            "https://mobile.twitter.com/jack/status/20",
            "https://www.twitter.com/jack/status/20/photo/1",
            "https://fixupx.com/jack/status/20",
            "https://fxtwitter.com/jack/status/20#m",
            "https://vxtwitter.com/jack/statuses/20",
            "https://x.com/i/web/status/20",
            "http://x.com/i/status/20",
        ] {
            assert_eq!(parse_status_id(url).as_deref(), Some("20"), "{url}");
        }
    }

    #[test]
    fn rejects_urls_that_are_not_x_posts() {
        for url in [
            "https://example.com/jack/status/20",
            "https://x.com/jack",
            "https://x.com/jack/status/abc",
            "ftp://x.com/jack/status/20",
            "-o/etc/passwd",
            "",
        ] {
            assert_eq!(parse_status_id(url), None, "{url}");
        }
    }

    #[test]
    fn picks_the_highest_bitrate_mp4_and_ignores_hls() {
        let item: MediaItem = serde_json::from_value(serde_json::json!({
            "type": "video",
            "url": "https://v/default.mp4",
            "formats": [
                {"url": "https://v/pl.m3u8", "container": "m3u8"},
                {"url": "https://v/low.mp4", "container": "mp4", "bitrate": 256000},
                {"url": "https://v/high.mp4", "container": "mp4", "bitrate": 10368000},
                {"url": "https://v/mid.mp4", "container": "mp4", "bitrate": 2176000}
            ]
        }))
        .unwrap();
        assert_eq!(item.best_mp4(), "https://v/high.mp4");
    }

    #[test]
    fn falls_back_to_the_default_url_when_no_mp4_format_is_listed() {
        let item = MediaItem {
            url: "https://v/only.mp4".into(),
            ..Default::default()
        };
        assert_eq!(item.best_mp4(), "https://v/only.mp4");
    }

    #[test]
    fn reports_an_unavailable_quote_with_its_message() {
        let s: Status = serde_json::from_value(serde_json::json!({
            "id": "1",
            "quote": {"type": "tombstone", "reason": "deleted", "message": "This post was deleted"}
        }))
        .unwrap();
        assert_eq!(s.quoted().unwrap().unwrap_err(), "This post was deleted");
    }
}
