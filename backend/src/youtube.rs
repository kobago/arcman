use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;

use crate::{archive::Download, fxtwitter::FetchError};

pub const DEFAULT_OEMBED_URL: &str = "https://www.youtube.com/oembed";

const HOSTS: [&str; 4] = [
    "youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtube-nocookie.com",
];

#[derive(Debug, PartialEq)]
pub struct Video {
    pub id: String,
    pub shorts: bool,
}

impl Video {
    pub fn url(&self) -> String {
        if self.shorts {
            format!("https://www.youtube.com/shorts/{}", self.id)
        } else {
            format!("https://www.youtube.com/watch?v={}", self.id)
        }
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn parse_video(input: &str) -> Option<Video> {
    let url = Url::parse(input.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    let segs: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let (id, shorts) = if host == "youtu.be" {
        (segs.first()?.to_string(), false)
    } else if HOSTS.contains(&host) {
        match segs.as_slice() {
            ["watch"] => (
                url.query_pairs()
                    .find_map(|(k, v)| (k == "v").then(|| v.into_owned()))?,
                false,
            ),
            ["shorts", id, ..] => (id.to_string(), true),
            ["live" | "embed" | "v", id, ..] => (id.to_string(), false),
            _ => return None,
        }
    } else {
        return None;
    };
    valid_id(&id).then_some(Video { id, shorts })
}

/// The fields arcman reads from YouTube's oEmbed response; the whole response is kept as the snapshot.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Oembed {
    pub title: String,
    pub author_name: String,
    pub author_url: String,
    pub thumbnail_url: Option<String>,
}

impl Oembed {
    /// The channel handle (or channel id for channels without one), used like an X screen name.
    pub fn channel_id(&self) -> String {
        Url::parse(&self.author_url)
            .ok()
            .and_then(|u| {
                u.path_segments()?
                    .rfind(|s| !s.is_empty())
                    .map(str::to_string)
            })
            .map(|s| s.trim_start_matches('@').to_string())
            .unwrap_or_default()
    }
}

pub async fn fetch(
    http: &reqwest::Client,
    oembed_url: &str,
    video_id: &str,
) -> Result<(Value, Oembed), FetchError> {
    let watch = format!("https://www.youtube.com/watch?v={video_id}");
    let url = Url::parse_with_params(oembed_url, [("format", "json"), ("url", watch.as_str())])
        .map_err(|e| FetchError::Transport(format!("{oembed_url}: {e}")))?;
    let res = http
        .get(url)
        .send()
        .await
        .map_err(|e| FetchError::Transport(e.to_string()))?;
    match res.status().as_u16() {
        200 => {}
        400 | 404 => return Err(FetchError::Unavailable("not_found")),
        401 | 403 => return Err(FetchError::Unavailable("private")),
        c => return Err(FetchError::Transport(format!("oEmbed returned {c}"))),
    }
    let raw: Value = res
        .json()
        .await
        .map_err(|e| FetchError::Transport(format!("unexpected response: {e}")))?;
    let oembed = serde_json::from_value(raw.clone())
        .map_err(|e| FetchError::Transport(format!("unexpected response: {e}")))?;
    Ok((raw, oembed))
}

/// The thumbnail is the only file kept: the largest size when the video has one, else oEmbed's.
pub fn plan(oembed: &Oembed, shorts: bool) -> Vec<Download> {
    let Some(thumb) = &oembed.thumbnail_url else {
        return vec![];
    };
    let largest = thumb.replace("/hqdefault.", "/maxresdefault.");
    // Thumbnails are letterboxed to 4:3, so tiles crop them back to the video's own shape.
    let (width, height) = if shorts { (9, 16) } else { (16, 9) };
    vec![Download {
        role: "main",
        position: 0,
        kind: "photo",
        fallback_url: (largest != *thumb).then(|| thumb.clone()),
        url: largest,
        optional: false,
        thumbnail_url: None,
        width: Some(width),
        height: Some(height),
        duration: None,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_video_id_from_every_supported_url_form() {
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?feature=share&v=dQw4w9WgXcQ&t=42",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=x",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
            "https://www.youtube.com/live/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
            "http://www.youtube.com/v/dQw4w9WgXcQ",
        ] {
            assert_eq!(
                parse_video(url),
                Some(Video {
                    id: "dQw4w9WgXcQ".into(),
                    shorts: false
                }),
                "{url}"
            );
        }
        assert_eq!(
            parse_video("https://www.youtube.com/shorts/W8VZmVf4d4M"),
            Some(Video {
                id: "W8VZmVf4d4M".into(),
                shorts: true
            })
        );
    }

    #[test]
    fn rejects_urls_that_are_not_videos() {
        for url in [
            "https://www.youtube.com/",
            "https://www.youtube.com/@RickAstleyYT",
            "https://www.youtube.com/playlist?list=PL123",
            "https://www.youtube.com/watch?v=short",
            "https://www.youtube.com/watch?v=dQw4w9WgXc!",
            "https://youtu.be/",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "ftp://youtu.be/dQw4w9WgXcQ",
        ] {
            assert_eq!(parse_video(url), None, "{url}");
        }
    }

    #[test]
    fn the_channel_id_is_the_handle_or_the_last_path_segment() {
        let channel = |author_url: &str| {
            Oembed {
                author_url: author_url.into(),
                ..Default::default()
            }
            .channel_id()
        };
        assert_eq!(
            channel("https://www.youtube.com/@RickAstleyYT"),
            "RickAstleyYT"
        );
        assert_eq!(channel("https://www.youtube.com/channel/UC123"), "UC123");
        assert_eq!(channel(""), "");
    }

    #[test]
    fn plans_the_largest_thumbnail_with_the_oembed_one_as_fallback() {
        let oembed = Oembed {
            thumbnail_url: Some("https://i.ytimg.com/vi/x/hqdefault.jpg".into()),
            ..Default::default()
        };
        let [d] = plan(&oembed, false).try_into().unwrap();
        assert_eq!(d.url, "https://i.ytimg.com/vi/x/maxresdefault.jpg");
        assert_eq!(
            d.fallback_url.as_deref(),
            Some("https://i.ytimg.com/vi/x/hqdefault.jpg")
        );
        assert_eq!((d.width, d.height), (Some(16), Some(9)));
        let [short] = plan(&oembed, true).try_into().unwrap();
        assert_eq!((short.width, short.height), (Some(9), Some(16)));
    }
}
