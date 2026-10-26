use std::collections::HashMap;

use reqwest::Url;
use serde::Deserialize;

use crate::{
    fxtwitter::FetchError,
    pixiv,
    web::{Image, Page},
};

pub const DEFAULT_BASE_URL: &str = "https://api.asmr.one";

/// ASMR Online and its mirrors; the pages are drawn by script, so their HTML has no metadata.
const HOSTS: [&str; 4] = ["asmr.one", "asmr-100.com", "asmr-200.com", "asmr-300.com"];

/// The work id a work page is for: `RJ01201653` is work 1201653. Works without an RJ code,
/// such as `/work/100000036/DLSITE/VJ01001518`, are known by the id before their source.
pub fn parse_work_id(input: &str) -> Option<u64> {
    let url = Url::parse(input.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    if !HOSTS.contains(&host.strip_prefix("www.").unwrap_or(host)) {
        return None;
    }
    let segs: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let digits = match segs.as_slice() {
        ["work", code] => code
            .strip_prefix("RJ")
            .or_else(|| code.strip_prefix("rj"))?,
        ["work", id, _source, _code] => id,
        _ => return None,
    };
    if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Named {
    name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Tag {
    name: String,
    i18n: HashMap<String, Named>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct WorkInfo {
    title: String,
    /// The circle's name.
    name: String,
    release: String,
    vas: Vec<Named>,
    tags: Vec<Tag>,
    main_cover_url: Option<String>,
}

/// A work as a bookmarked page, with the columns a page does not hold.
#[derive(Debug)]
pub struct Work {
    pub page: Page,
    pub circle: String,
    pub released_at: Option<i64>,
    pub tags: Vec<String>,
}

fn work(info: WorkInfo, url: &str) -> Work {
    let cv = info
        .vas
        .iter()
        .map(|v| v.name.as_str())
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let description = [
        info.name.clone(),
        if cv.is_empty() {
            cv
        } else {
            format!("CV: {cv}")
        },
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" / ");
    Work {
        page: Page {
            url: url.to_string(),
            title: info.title,
            description: (!description.is_empty()).then_some(description),
            site_name: Some("ASMR Online".into()),
            // DLsite covers are 560×420.
            image: info.main_cover_url.map(|url| Image {
                url,
                width: Some(4),
                height: Some(3),
            }),
            icon: None,
            lang: None,
        },
        circle: info.name,
        // Works come out at midnight in Japan, the day DLsite gives.
        released_at: pixiv::unix_time(&format!("{}T00:00:00+09:00", info.release)),
        tags: info
            .tags
            .into_iter()
            .map(|t| {
                t.i18n
                    .get("ja-jp")
                    .map(|n| n.name.clone())
                    .filter(|n| !n.is_empty())
                    .unwrap_or(t.name)
            })
            .collect(),
    }
}

pub async fn fetch(
    http: &reqwest::Client,
    base_url: &str,
    url: &str,
    id: u64,
) -> Result<Work, FetchError> {
    let api = format!("{}/api/workInfo/{id}", base_url.trim_end_matches('/'));
    let res = http
        .get(&api)
        .send()
        .await
        .map_err(|e| FetchError::Transport(e.to_string()))?;
    match res.status().as_u16() {
        200 => {}
        404 => return Err(FetchError::Unavailable("not_found")),
        c => return Err(FetchError::Transport(format!("{api} returned {c}"))),
    }
    let info: WorkInfo = res
        .json()
        .await
        .map_err(|e| FetchError::Transport(format!("unexpected response: {e}")))?;
    Ok(work(info, url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_work_id_from_asmr_online_and_its_mirrors() {
        for url in [
            "https://www.asmr.one/work/RJ01201653",
            "https://asmr.one/work/RJ01201653?tab=tracks",
            "https://asmr-200.com/work/RJ01201653/",
            "http://www.asmr-100.com/work/rj01201653",
        ] {
            assert_eq!(parse_work_id(url), Some(1201653), "{url}");
        }
        assert_eq!(
            parse_work_id("https://www.asmr.one/work/100000036/DLSITE/VJ01001518"),
            Some(100000036)
        );
        for url in [
            "https://www.asmr.one/",
            "https://www.asmr.one/works?keyword=x",
            "https://www.asmr.one/work/RJ",
            "https://www.asmr.one/work/RJ12ab",
            "https://www.asmr.one/work/RJ01201653/tracks",
            "https://www.asmr.one/work/VJ01001518",
            "https://www.asmr.one/work/abc/DLSITE/VJ01001518",
            "https://example.com/work/RJ01201653",
        ] {
            assert_eq!(parse_work_id(url), None, "{url}");
        }
    }

    #[test]
    fn a_work_becomes_a_page_with_its_circle_voices_cover_and_japanese_tags() {
        let info: WorkInfo = serde_json::from_value(serde_json::json!({
            "id": 1201653,
            "title": "幼馴染の囁き",
            "name": "おいしいおこめ",
            "release": "2024-05-31",
            "vas": [{"id": "x", "name": "餅梨あむ"}, {"id": "y", "name": "夢咲"}],
            "tags": [
                {"name": "cosplay/角色扮演", "i18n": {"ja-jp": {"name": "コスプレ"}}},
                {"name": "耳舐め", "i18n": {}}
            ],
            "mainCoverUrl": "https://api.asmr.one/api/cover/1201653.jpg?type=main"
        }))
        .unwrap();
        let w = work(info, "https://www.asmr.one/work/RJ01201653");
        assert_eq!(w.page.title, "幼馴染の囁き");
        assert_eq!(
            w.page.description.as_deref(),
            Some("おいしいおこめ / CV: 餅梨あむ, 夢咲")
        );
        assert_eq!(
            w.page.image.unwrap().url,
            "https://api.asmr.one/api/cover/1201653.jpg?type=main"
        );
        assert_eq!(w.circle, "おいしいおこめ");
        assert_eq!(w.released_at, Some(1717081200));
        assert_eq!(w.tags, ["コスプレ", "耳舐め"]);
    }
}
