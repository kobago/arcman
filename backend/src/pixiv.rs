use reqwest::Url;
use scraper::Html;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{archive::Download, fxtwitter::FetchError, web::Image};

pub const DEFAULT_BASE_URL: &str = "https://www.pixiv.net";

const HOSTS: [&str; 2] = ["pixiv.net", "phixiv.net"];

/// Pixiv ids are plain numbers like X status ids, so they are saved under a prefix.
const STATUS_PREFIX: &str = "pixiv:";

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 12 && id.bytes().all(|b| b.is_ascii_digit())
}

/// The status id an artwork URL is saved under, such as `pixiv:12345`.
pub fn parse_status_id(input: &str) -> Option<String> {
    let url = Url::parse(input.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    if !HOSTS.contains(&host.strip_prefix("www.").unwrap_or(host)) {
        return None;
    }
    let segs: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let id = match segs.as_slice() {
        ["artworks", id, ..] | [_, "artworks", id, ..] | ["i", id] => id.to_string(),
        ["member_illust.php"] => url
            .query_pairs()
            .find_map(|(k, v)| (k == "illust_id").then(|| v.into_owned()))?,
        _ => return None,
    };
    valid_id(&id).then(|| format!("{STATUS_PREFIX}{id}"))
}

pub fn artwork_id(status_id: &str) -> &str {
    status_id.strip_prefix(STATUS_PREFIX).unwrap_or(status_id)
}

pub fn artwork_url(id: &str) -> String {
    format!("https://www.pixiv.net/artworks/{id}")
}

/// What arcman keeps of an artwork. Saved as the snapshot.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Illust {
    pub id: String,
    pub title: String,
    /// The caption as plain text.
    pub caption: String,
    pub user_name: String,
    pub user_account: String,
    pub avatar: Option<String>,
    pub created_at: Option<i64>,
    pub tags: Vec<String>,
    pub pages: Vec<Image>,
    pub likes: Option<u64>,
    pub bookmarks: Option<u64>,
    pub views: Option<u64>,
}

impl Illust {
    /// Title and caption together, so keyword search finds either.
    pub fn text(&self) -> String {
        match self.caption.as_str() {
            "" => self.title.clone(),
            c => format!("{}\n{c}", self.title),
        }
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    error: bool,
    body: Option<T>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Body {
    illust_id: String,
    illust_title: String,
    illust_comment: String,
    user_name: String,
    user_account: String,
    create_date: String,
    urls: Urls,
    width: Option<u32>,
    height: Option<u32>,
    page_count: u32,
    tags: Tags,
    like_count: Option<u64>,
    bookmark_count: Option<u64>,
    view_count: Option<u64>,
    user_illusts: Value,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Urls {
    original: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Tags {
    tags: Vec<Tag>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Tag {
    tag: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct PageBody {
    urls: Urls,
    width: Option<u32>,
    height: Option<u32>,
}

/// Captions are HTML with `<br />` line breaks and linked URLs.
fn plain_text(html: &str) -> String {
    let html = html
        .replace("<br />", "\n")
        .replace("<br/>", "\n")
        .replace("<br>", "\n");
    Html::parse_fragment(&html)
        .root_element()
        .text()
        .collect::<String>()
        .trim()
        .to_string()
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Reads `2007-09-09T22:14:00+09:00`, the form pixiv gives its dates in.
pub(crate) fn unix_time(s: &str) -> Option<i64> {
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (date, time) = (
        days_from_civil(num(0..4)?, num(5..7)?, num(8..10)?),
        num(11..13)? * 3600 + num(14..16)? * 60 + num(17..19)?,
    );
    let offset = match s.get(19..)? {
        "Z" => 0,
        tz => {
            let sign = match tz.get(..1)? {
                "+" => 1,
                "-" => -1,
                _ => return None,
            };
            sign * (tz.get(1..3)?.parse::<i64>().ok()? * 3600
                + tz.get(4..6)?.parse::<i64>().ok()? * 60)
        }
    };
    Some(date * 86400 + time - offset)
}

async fn get<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    session: Option<&str>,
    url: &str,
) -> Result<T, FetchError> {
    let mut req = http.get(url);
    if let Some(session) = session {
        req = req.header(reqwest::header::COOKIE, format!("PHPSESSID={session}"));
    }
    let res = req
        .send()
        .await
        .map_err(|e| FetchError::Transport(e.to_string()))?;
    let code = res.status().as_u16();
    if !matches!(code, 200 | 400 | 401 | 403 | 404) {
        return Err(FetchError::Transport(format!("{url} returned {code}")));
    }
    let envelope: Envelope<T> = res
        .json()
        .await
        .map_err(|e| FetchError::Transport(format!("unexpected response: {e}")))?;
    match (code, envelope) {
        (
            200,
            Envelope {
                error: false,
                body: Some(body),
            },
        ) => Ok(body),
        (401 | 403, _) => Err(FetchError::Unavailable("private")),
        _ => Err(FetchError::Unavailable("not_found")),
    }
}

pub async fn fetch(
    http: &reqwest::Client,
    base_url: &str,
    session: Option<&str>,
    status_id: &str,
) -> Result<(Value, Illust), FetchError> {
    let id = artwork_id(status_id);
    let base = base_url.trim_end_matches('/');
    let body: Body = get(http, session, &format!("{base}/ajax/illust/{id}")).await?;
    // R-18 works come without their image URLs unless a signed-in session that shows them is sent.
    let Some(original) = body.urls.original.clone() else {
        return Err(FetchError::Unavailable("login_required"));
    };
    let pages = if body.page_count > 1 {
        let pages: Vec<PageBody> =
            get(http, session, &format!("{base}/ajax/illust/{id}/pages")).await?;
        pages
            .into_iter()
            .filter_map(|p| {
                Some(Image {
                    url: p.urls.original?,
                    width: p.width,
                    height: p.height,
                })
            })
            .collect()
    } else {
        vec![Image {
            url: original,
            width: body.width,
            height: body.height,
        }]
    };
    let illust = Illust {
        avatar: body.user_illusts[&body.illust_id]["profileImageUrl"]
            .as_str()
            .map(str::to_string),
        id: body.illust_id,
        title: body.illust_title,
        caption: plain_text(&body.illust_comment),
        user_name: body.user_name,
        user_account: body.user_account,
        created_at: unix_time(&body.create_date),
        tags: body.tags.tags.into_iter().map(|t| t.tag).collect(),
        pages,
        likes: body.like_count,
        bookmarks: body.bookmark_count,
        views: body.view_count,
    };
    let raw = serde_json::to_value(&illust).expect("an illust serializes");
    Ok((raw, illust))
}

/// Every page at its original size, and the artist's icon.
pub fn plan(illust: &Illust) -> Vec<Download> {
    let mut out: Vec<Download> = illust
        .pages
        .iter()
        .enumerate()
        .map(|(i, p)| Download {
            width: p.width,
            height: p.height,
            ..Download::photo("main", i as i64, &p.url)
        })
        .collect();
    if let Some(url) = &illust.avatar {
        out.push(Download {
            optional: true,
            ..Download::photo("avatar", 0, url)
        });
    }
    out
}

/// pixiv's image servers refuse requests that do not come from pixiv's pages.
pub fn referer(url: &str) -> Option<&'static str> {
    let host = Url::parse(url).ok()?.host_str()?.to_string();
    (host == "pximg.net" || host.ends_with(".pximg.net")).then_some("https://www.pixiv.net/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_artwork_id_from_every_supported_url_form() {
        for url in [
            "https://www.pixiv.net/artworks/20",
            "https://pixiv.net/en/artworks/20",
            "https://www.pixiv.net/artworks/20#manga",
            "https://www.pixiv.net/i/20",
            "https://www.pixiv.net/member_illust.php?mode=medium&illust_id=20",
            "https://www.phixiv.net/artworks/20/2",
        ] {
            assert_eq!(parse_status_id(url).as_deref(), Some("pixiv:20"), "{url}");
        }
    }

    #[test]
    fn rejects_urls_that_are_not_artworks() {
        for url in [
            "https://www.pixiv.net/",
            "https://www.pixiv.net/users/10",
            "https://www.pixiv.net/novel/show.php?id=20",
            "https://www.pixiv.net/artworks/abc",
            "https://example.com/artworks/20",
            "ftp://www.pixiv.net/artworks/20",
        ] {
            assert_eq!(parse_status_id(url), None, "{url}");
        }
    }

    #[test]
    fn the_artwork_id_is_the_status_id_without_its_prefix() {
        assert_eq!(artwork_id("pixiv:20"), "20");
    }

    #[test]
    fn captions_become_plain_text_with_their_line_breaks() {
        assert_eq!(
            plain_text(
                r#"line one<br />line <b>two</b><br><a href="/jump.php?x">https://a.example/</a> "#
            ),
            "line one\nline two\nhttps://a.example/"
        );
    }

    #[test]
    fn reads_dates_with_their_offset() {
        assert_eq!(unix_time("2007-09-09T22:14:00+09:00"), Some(1_189_343_640));
        assert_eq!(unix_time("2007-09-09T13:14:00+00:00"), Some(1_189_343_640));
        assert_eq!(unix_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_time(""), None);
    }

    #[test]
    fn plans_every_page_and_an_optional_icon() {
        let illust = Illust {
            pages: (0..2)
                .map(|i| Image {
                    url: format!("https://i.pximg.net/img-original/img/x/20_p{i}.png"),
                    width: Some(400),
                    height: Some(600),
                })
                .collect(),
            avatar: Some("https://i.pximg.net/user-profile/a_50.jpg".into()),
            ..Default::default()
        };
        let plan = plan(&illust);
        assert_eq!(
            plan.iter()
                .map(|d| (d.role, d.file_name(), d.optional))
                .collect::<Vec<_>>(),
            [
                ("main", "main-0.png".to_string(), false),
                ("main", "main-1.png".to_string(), false),
                ("avatar", "avatar-0.jpg".to_string(), true),
            ]
        );
        assert_eq!((plan[1].width, plan[1].height), (Some(400), Some(600)));
    }

    #[test]
    fn only_pixiv_image_servers_get_a_referer() {
        assert_eq!(
            referer("https://i.pximg.net/img-original/a.jpg"),
            Some("https://www.pixiv.net/")
        );
        assert_eq!(referer("https://pbs.twimg.com/media/a.jpg"), None);
        assert_eq!(referer("https://evilpximg.net/a.jpg"), None);
    }
}
