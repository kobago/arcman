use std::time::Duration;

use reqwest::{Url, header};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{archive::Download, fxtwitter::FetchError};

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; arcman/",
    env!("CARGO_PKG_VERSION"),
    "; +https://github.com/kobago/arcman)"
);
/// Metadata lives in the head, so the rest of a long page is not read.
const MAX_HTML_BYTES: usize = 1 << 20;
const TRACKING_PARAMS: [&str; 6] = ["fbclid", "gclid", "igshid", "mc_cid", "mc_eid", "yclid"];

/// The URL a bookmark is known by: without its fragment and tracking parameters.
pub fn normalize(input: &str) -> Option<String> {
    let mut url = Url::parse(input.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_fragment(None);
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !k.starts_with("utm_") && !TRACKING_PARAMS.contains(&k.as_ref()))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        url.set_query(None);
    } else if url.query_pairs().count() != kept.len() {
        url.query_pairs_mut().clear().extend_pairs(kept);
    }
    Some(url.into())
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Image {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// What arcman keeps of a page: its metadata, not the page itself. Saved as the snapshot.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Page {
    /// Where the page ended up after redirects.
    pub url: String,
    pub title: String,
    pub description: Option<String>,
    pub site_name: Option<String>,
    pub image: Option<Image>,
    pub icon: Option<String>,
    pub lang: Option<String>,
}

impl Page {
    pub fn host(&self) -> String {
        Url::parse(&self.url)
            .ok()
            .and_then(|u| {
                u.host_str()
                    .map(|h| h.trim_start_matches("www.").to_string())
            })
            .unwrap_or_default()
    }

    /// Title and description together, so keyword search finds either.
    pub fn text(&self) -> String {
        match &self.description {
            Some(d) => format!("{}\n{d}", self.title),
            None => self.title.clone(),
        }
    }
}

fn clean(s: &str) -> Option<String> {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (!s.is_empty()).then(|| s.chars().take(1000).collect())
}

fn absolute(base: &Url, href: &str) -> Option<String> {
    let url = base.join(href.trim()).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.into())
}

fn file_name(url: &Url) -> String {
    url.path_segments()
        .and_then(|mut s| s.rfind(|s| !s.is_empty()).map(str::to_string))
        .unwrap_or_else(|| url.host_str().unwrap_or_default().to_string())
}

pub fn parse(html: &str, base: &Url) -> Page {
    let doc = Html::parse_document(html);
    let select = |css: &str| Selector::parse(css).expect("valid selector");
    let mut meta = std::collections::HashMap::new();
    for el in doc.select(&select("meta[content]")) {
        let v = el.value();
        if let Some(key) = v.attr("property").or_else(|| v.attr("name")) {
            meta.entry(key.trim().to_ascii_lowercase())
                .or_insert_with(|| v.attr("content").unwrap_or_default().to_string());
        }
    }
    let first = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| meta.get(*k).and_then(|v| clean(v)))
    };
    let title_tag = doc
        .select(&select("title"))
        .next()
        .and_then(|t| clean(&t.text().collect::<String>()));
    let image = first(&[
        "og:image:secure_url",
        "og:image",
        "og:image:url",
        "twitter:image",
        "twitter:image:src",
    ])
    .and_then(|u| absolute(base, &u))
    .map(|url| Image {
        url,
        width: first(&["og:image:width"]).and_then(|w| w.parse().ok()),
        height: first(&["og:image:height"]).and_then(|h| h.parse().ok()),
    });
    let icons: Vec<(String, String)> = doc
        .select(&select("link[rel][href]"))
        .map(|el| {
            let rel = el
                .value()
                .attr("rel")
                .unwrap_or_default()
                .to_ascii_lowercase();
            (rel, el.value().attr("href").unwrap_or_default().to_string())
        })
        .collect();
    let icon = ["apple-touch-icon", "icon"]
        .iter()
        .find_map(|want| {
            icons
                .iter()
                .find(|(rel, _)| rel.split_whitespace().any(|r| r == *want))
        })
        .and_then(|(_, href)| absolute(base, href))
        .or_else(|| absolute(base, "/favicon.ico"));
    Page {
        url: base.to_string(),
        title: first(&["og:title", "twitter:title"])
            .or(title_tag)
            .unwrap_or_else(|| file_name(base)),
        description: first(&["og:description", "twitter:description", "description"]),
        site_name: first(&["og:site_name", "application-name"]),
        image,
        icon,
        lang: doc
            .select(&select("html[lang]"))
            .next()
            .and_then(|h| clean(h.value().attr("lang").unwrap_or_default())),
    }
}

fn charset_label(content_type: &str) -> Option<String> {
    let lower = content_type.to_ascii_lowercase();
    let rest = &lower[lower.find("charset=")? + "charset=".len()..];
    let label: String = rest
        .trim_start_matches(['"', '\''])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
        .collect();
    (!label.is_empty()).then_some(label)
}

/// Decodes by the header's charset, else the one a `<meta>` declares near the top, else UTF-8.
pub fn decode(bytes: &[u8], content_type: &str) -> String {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]);
    let encoding = charset_label(content_type)
        .or_else(|| charset_label(&head))
        .and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    encoding.decode(bytes).0.into_owned()
}

pub async fn fetch(http: &reqwest::Client, url: &str) -> Result<(Value, Page), FetchError> {
    let transport = |e: reqwest::Error| FetchError::Transport(e.to_string());
    let mut res = http
        .get(url)
        .header(header::USER_AGENT, USER_AGENT)
        .header(
            header::ACCEPT,
            "text/html,application/xhtml+xml;q=0.9,*/*;q=0.8",
        )
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(transport)?;
    match res.status().as_u16() {
        200..=299 => {}
        404 | 410 => return Err(FetchError::Unavailable("not_found")),
        c => return Err(FetchError::Transport(format!("{url} returned {c}"))),
    }
    let base = res.url().clone();
    let content_type = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let page = if content_type.starts_with("image/") {
        Page {
            url: base.to_string(),
            title: file_name(&base),
            image: Some(Image {
                url: base.to_string(),
                ..Default::default()
            }),
            ..Default::default()
        }
    } else if content_type.is_empty() || content_type.contains("html") {
        let mut body = vec![];
        while body.len() < MAX_HTML_BYTES
            && let Some(chunk) = res.chunk().await.map_err(transport)?
        {
            body.extend_from_slice(&chunk);
        }
        parse(&decode(&body, &content_type), &base)
    } else {
        Page {
            url: base.to_string(),
            title: file_name(&base),
            ..Default::default()
        }
    };
    let raw = serde_json::to_value(&page).expect("a page serializes");
    Ok((raw, page))
}

/// The page's image and icon. Pages often point at images that are gone or not images,
/// so neither may fail the bookmark.
pub fn plan(page: &Page) -> Vec<Download> {
    let mut out = vec![];
    if let Some(image) = &page.image {
        // Most share images are 1.91:1, the size a page usually leaves out.
        let (width, height) = match (image.width, image.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
            _ => (1200, 630),
        };
        out.push(Download {
            width: Some(width),
            height: Some(height),
            optional: true,
            ..Download::photo("main", 0, &image.url)
        });
    }
    if let Some(icon) = &page.icon {
        out.push(Download {
            optional: true,
            ..Download::photo("avatar", 0, icon)
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizing_drops_the_fragment_and_tracking_parameters_only() {
        assert_eq!(
            normalize("https://Example.com/a?utm_source=x&id=3&fbclid=y#top").as_deref(),
            Some("https://example.com/a?id=3")
        );
        assert_eq!(
            normalize("https://example.com/a?utm_medium=x").as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(
            normalize("https://example.com/a?b=1&a=2").as_deref(),
            Some("https://example.com/a?b=1&a=2")
        );
        assert_eq!(normalize("ftp://example.com/a"), None);
        assert_eq!(normalize("not a url"), None);
        assert_eq!(normalize("mailto:a@example.com"), None);
    }

    #[test]
    fn reads_open_graph_tags_before_the_plain_ones() {
        let base = Url::parse("https://www.example.com/posts/1").unwrap();
        let page = parse(
            r#"<html lang="ja"><head>
              <title>Plain title</title>
              <meta name="description" content="plain description">
              <meta property="og:title" content="  OG
                 title ">
              <meta property="og:description" content="OG description">
              <meta property="og:site_name" content="Example">
              <meta property="og:image" content="/img/cover.png">
              <meta property="og:image:width" content="800">
              <meta property="og:image:height" content="400">
              <link rel="shortcut icon" href="/favicon.png">
              <link rel="apple-touch-icon" href="https://cdn.example.com/touch.png">
            </head></html>"#,
            &base,
        );
        assert_eq!(
            page,
            Page {
                url: "https://www.example.com/posts/1".into(),
                title: "OG title".into(),
                description: Some("OG description".into()),
                site_name: Some("Example".into()),
                image: Some(Image {
                    url: "https://www.example.com/img/cover.png".into(),
                    width: Some(800),
                    height: Some(400),
                }),
                icon: Some("https://cdn.example.com/touch.png".into()),
                lang: Some("ja".into()),
            }
        );
        assert_eq!(page.host(), "example.com");
        assert_eq!(page.text(), "OG title\nOG description");
    }

    #[test]
    fn falls_back_to_the_title_tag_the_favicon_and_the_file_name() {
        let base = Url::parse("https://example.com/docs/guide.html").unwrap();
        let page = parse(
            "<title>Guide</title><meta name=description content=About>",
            &base,
        );
        assert_eq!(page.title, "Guide");
        assert_eq!(page.description.as_deref(), Some("About"));
        assert_eq!(
            page.icon.as_deref(),
            Some("https://example.com/favicon.ico")
        );
        assert_eq!(page.image, None);
        assert_eq!(parse("", &base).title, "guide.html");
    }

    #[test]
    fn decodes_by_the_declared_charset() {
        let (sjis, _, _) =
            encoding_rs::SHIFT_JIS.encode("<meta charset=\"Shift_JIS\"><title>日本語</title>");
        assert!(decode(&sjis, "text/html").contains("日本語"));
        let (euc, _, _) = encoding_rs::EUC_JP.encode("<title>日本語</title>");
        assert!(decode(&euc, "text/html; charset=EUC-JP").contains("日本語"));
        assert!(decode("日本語".as_bytes(), "").contains("日本語"));
    }

    #[test]
    fn plans_an_optional_image_with_a_share_image_shape_and_the_icon() {
        let page = Page {
            image: Some(Image {
                url: "https://example.com/i.png".into(),
                ..Default::default()
            }),
            icon: Some("https://example.com/favicon.ico".into()),
            ..Default::default()
        };
        let plan = plan(&page);
        assert_eq!(
            plan.iter()
                .map(|d| (d.role, d.file_name(), d.optional))
                .collect::<Vec<_>>(),
            [
                ("main", "main-0.png".to_string(), true),
                ("avatar", "avatar-0.ico".to_string(), true)
            ]
        );
        assert_eq!((plan[0].width, plan[0].height), (Some(1200), Some(630)));
    }
}
