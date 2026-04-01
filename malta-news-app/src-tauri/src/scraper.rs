use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};

use crate::models::{CustomPublisherDef, PublisherDef, RawArticle, ScrapeMethod};
use crate::publishers::all_publisher_defs;

fn stable_id(url: &str) -> String {
    let hash = Sha256::digest(url.as_bytes());
    hex::encode(&hash[..8])
}

fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_good_image(url: &str) -> bool {
    let lower = url.to_lowercase();
    !(lower.contains("gravatar.com")
        || lower.contains("pixel")
        || lower.contains("tracking")
        || lower.contains("spacer")
        || lower.contains("blank.gif")
        || lower.contains("1x1")
        || lower.contains("/emoji/")
        || lower.contains("/smilies/")
        || lower.contains("wp-includes/")
        || lower.contains("data:image")
        || lower.contains("/avatar")
        || lower.contains("logo")
        || lower.ends_with(".svg")
        || lower.ends_with(".ico"))
}

/// Extract the first good image URL from HTML content.
fn extract_image_url(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let mut search_from = 0;

    while let Some(img_pos) = lower[search_from..].find("<img ") {
        let abs_pos = search_from + img_pos;
        let after_img = &html[abs_pos..];
        let after_img_lower = &lower[abs_pos..];

        let src_url = after_img_lower
            .find("src=\"")
            .and_then(|pos| {
                let start = pos + 5;
                after_img[start..].find('"').map(|end| &after_img[start..start + end])
            })
            .or_else(|| {
                after_img_lower.find("src='").and_then(|pos| {
                    let start = pos + 5;
                    after_img[start..].find('\'').map(|end| &after_img[start..start + end])
                })
            });

        if let Some(url) = src_url {
            if url.starts_with("http") && is_good_image(url) {
                return Some(url.to_string());
            }
        }

        search_from = abs_pos + 5;
        if search_from >= lower.len() {
            break;
        }
    }
    None
}

/// Extract content attribute value from a meta tag string.
fn extract_meta_content<'a>(tag: &'a str, original_tag: &'a str) -> Option<&'a str> {
    // Try quoted: content="..." or content='...'
    for (prefix, quote) in [("content=\"", '"'), ("content='", '\'')] {
        if let Some(pos) = tag.find(prefix) {
            let start = pos + prefix.len();
            if let Some(end) = original_tag[start..].find(quote) {
                return Some(&original_tag[start..start + end]);
            }
        }
    }
    // Try unquoted: content=https://...
    if let Some(pos) = tag.find("content=") {
        let start = pos + 8;
        let rest = &original_tag[start..];
        let end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '"' || c == '\'')
            .unwrap_or(rest.len());
        if end > 0 {
            return Some(&rest[..end]);
        }
    }
    None
}

/// Extract image URL from meta tags (og:image, twitter:image, etc).
pub fn extract_meta_image(html: &str) -> Option<String> {
    // Normalize whitespace so <meta\nproperty=...> becomes <meta property=...>
    // Only normalize within the head section (first 100KB) to avoid huge allocations.
    let normalized = html
        .chars()
        .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
        .collect::<String>();
    let lower = normalized.to_lowercase();
    let mut search_from = 0;

    let mut og_image: Option<String> = None;
    let mut twitter_image: Option<String> = None;

    while let Some(meta_pos) = lower[search_from..].find("<meta ") {
        let abs_pos = search_from + meta_pos;
        let tag_end = match lower[abs_pos..].find('>') {
            Some(e) => abs_pos + e + 1,
            None => break,
        };
        let tag = &lower[abs_pos..tag_end];
        let original_tag = &normalized[abs_pos..tag_end];

        // og:image (but not og:image:width, og:image:height, etc.)
        if og_image.is_none()
            && tag.contains("og:image")
            && !tag.contains("og:image:")
        {
            if let Some(url) = extract_meta_content(tag, original_tag) {
                if url.starts_with("http") && is_good_image(url) {
                    og_image = Some(url.to_string());
                }
            }
        }

        // twitter:image or twitter:image:src
        if twitter_image.is_none()
            && (tag.contains("twitter:image:src") || tag.contains("twitter:image"))
        {
            if let Some(url) = extract_meta_content(tag, original_tag) {
                if url.starts_with("http") && is_good_image(url) {
                    twitter_image = Some(url.to_string());
                }
            }
        }

        search_from = tag_end;
    }

    og_image.or(twitter_image)
}

fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("failed to build HTTP client")
}

// ── RSS scraping ────────────────────────────────────────────────────────────

async fn fetch_rss(publisher: &PublisherDef, rss_url: &str) -> Result<Vec<RawArticle>> {
    let client = build_client();
    let resp = client
        .get(rss_url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {}", rss_url))?;
    let body = resp.bytes().await?;
    let feed = feed_rs::parser::parse(&body[..])
        .with_context(|| format!("failed to parse feed for {}", publisher.id))?;

    let cutoff = Utc::now() - Duration::hours(48);

    let mut articles = Vec::new();
    for entry in feed.entries {
        let title = match &entry.title {
            Some(t) => t.content.trim().to_string(),
            None => continue,
        };
        if title.is_empty() {
            continue;
        }

        let pub_date = entry.published.or(entry.updated);
        if let Some(date) = pub_date {
            if date < cutoff {
                continue;
            }
        }

        let url = match entry.links.first() {
            Some(link) => link.href.clone(),
            None => continue,
        };

        let raw_summary = entry
            .summary
            .as_ref()
            .map(|s| s.content.clone())
            .unwrap_or_default();
        let snippet = strip_html(&raw_summary);
        // Strip common RSS boilerplate ("The post ... appeared first on ...")
        let snippet = if let Some(pos) = snippet.find("The post ") {
            snippet[..pos].trim().to_string()
        } else {
            snippet
        };
        let snippet = if snippet.len() > 500 {
            snippet[..500].to_string()
        } else {
            snippet
        };

        let image_url = extract_image_url(&raw_summary)
            .or_else(|| {
                entry
                    .content
                    .as_ref()
                    .and_then(|c| c.body.as_ref())
                    .and_then(|body| extract_image_url(body))
            })
            .or_else(|| {
                for media_obj in &entry.media {
                    for content in &media_obj.content {
                        if let Some(ref url) = content.url {
                            let uri = url.as_str();
                            if is_good_image(uri) {
                                return Some(uri.to_string());
                            }
                        }
                    }
                    for thumb in &media_obj.thumbnails {
                        let uri = &thumb.image.uri;
                        if is_good_image(uri) {
                            return Some(uri.clone());
                        }
                    }
                }
                None
            })
            .unwrap_or_default();

        let published_at = pub_date.unwrap_or_else(Utc::now).to_rfc3339();

        articles.push(RawArticle {
            id: stable_id(&url),
            publisher_id: publisher.id.to_string(),
            original_url: url,
            original_headline: title,
            translated_headline: String::new(),
            body_snippet: snippet,
            body_text: String::new(),
            image_url,
            language: publisher.primary_language.to_string(),
            published_at,
            category: String::new(), // classified after translation
        });
    }

    log::info!(
        "[rss] {}: {} articles",
        publisher.id,
        articles.len()
    );
    Ok(articles)
}

// ── HTML scraping ───────────────────────────────────────────────────────────

async fn fetch_html(
    publisher: &PublisherDef,
    url: &str,
    article_sel: &str,
    headline_sel: &str,
    image_sel: &str,
    link_attr: &str,
    base_url: &str,
) -> Result<Vec<RawArticle>> {
    let client = build_client();
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {}", url))?;

    let status = resp.status();
    let body = resp.text().await?;

    if !status.is_success() {
        log::warn!("[html] {}: HTTP {} for {}", publisher.id, status, url);
        // Still try to parse — some sites return 200-like content on error pages
    }

    // Parse HTML synchronously in a block so `document` is dropped before any await.
    let articles = {
        let document = Html::parse_document(&body);

        let article_selector = Selector::parse(article_sel)
            .map_err(|e| anyhow::anyhow!("bad article selector '{}': {:?}", article_sel, e))?;
        let headline_selector = Selector::parse(headline_sel).ok();
        let image_selector = Selector::parse(image_sel).ok();

        let mut articles = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();

        for element in document.select(&article_selector) {
            let raw_href = match element.value().attr(link_attr) {
                Some(h) => h.to_string(),
                None => continue,
            };

            let article_url = if raw_href.starts_with("http") {
                raw_href.clone()
            } else if raw_href.starts_with('/') {
                format!("{}{}", base_url, raw_href)
            } else {
                continue;
            };

            if is_nav_link(&article_url) {
                continue;
            }

            if !seen_urls.insert(article_url.clone()) {
                continue;
            }

            let headline = headline_selector
                .as_ref()
                .and_then(|sel| {
                    element.select(sel).next().map(|el| {
                        el.text().collect::<Vec<_>>().join(" ").trim().to_string()
                    })
                })
                .or_else(|| {
                    let text: String = element.text().collect::<Vec<_>>().join(" ");
                    let trimmed = text.trim().to_string();
                    if trimmed.len() > 10 { Some(trimmed) } else { None }
                })
                .unwrap_or_default();

            if headline.is_empty() || headline.len() < 10 {
                continue;
            }

            let headline = if headline.len() > 200 {
                headline[..200].to_string()
            } else {
                headline
            };

            let image_url = image_selector
                .as_ref()
                .and_then(|sel| {
                    element.select(sel).next().and_then(|img| {
                        img.value()
                            .attr("data-original") // Malta Independent lazy images
                            .or_else(|| img.value().attr("data-src"))
                            .or_else(|| img.value().attr("data-lazy-src"))
                            .or_else(|| img.value().attr("src"))
                            .filter(|s| !s.contains("loader") && !s.contains("placeholder"))
                            .map(|s| {
                                if s.starts_with("http") {
                                    s.to_string()
                                } else if s.starts_with("//") {
                                    format!("https:{}", s)
                                } else if s.starts_with('/') {
                                    format!("{}{}", base_url, s)
                                } else {
                                    s.to_string()
                                }
                            })
                            .filter(|u| is_good_image(u))
                    })
                })
                .unwrap_or_default();

            articles.push(RawArticle {
                id: stable_id(&article_url),
                publisher_id: publisher.id.to_string(),
                original_url: article_url,
                original_headline: headline,
                translated_headline: String::new(),
                body_snippet: String::new(),
                body_text: String::new(),
                image_url,
                language: publisher.primary_language.to_string(),
                published_at: Utc::now().to_rfc3339(),
                category: String::new(),
            });
        }
        articles
    }; // `document` is dropped here

    log::info!(
        "[html] {}: {} articles from {}",
        publisher.id,
        articles.len(),
        url
    );
    Ok(articles)
}

/// Check if a headline is a non-article (roundup, gallery, front pages, etc.).
fn is_non_article_headline(title: &str) -> bool {
    let lower = title.to_lowercase();
    lower.contains("front page")
        || lower.contains("years ago")
        || lower.contains("obituar")
        || lower.contains("in memoriam")
        || lower.contains("sudoku")
        || lower.contains("crossword")
        || lower.contains("horoscope")
        || lower.starts_with("watch:")
        || lower.starts_with("podcast:")
        || lower.starts_with("gallery:")
        || lower.starts_with("quiz:")
}

/// Check if a URL is a navigation/category link rather than an article.
fn is_nav_link(url: &str) -> bool {
    let lower = url.to_lowercase();
    // Exclude obvious non-article paths
    lower.ends_with("/news")
        || lower.ends_with("/news/")
        || lower.ends_with("/sport")
        || lower.ends_with("/sport/")
        || lower.contains("/category/")
        || lower.contains("/tag/")
        || lower.contains("/author/")
        || lower.contains("/page/")
        || lower.contains("/search")
        || lower.contains("/about")
        || lower.contains("/contact")
        || lower.contains("/privacy")
        || lower.contains("/terms")
        || lower.contains("/login")
        || lower.contains("/register")
        || lower.contains("/subscribe")
        || lower.contains("#")
        || lower.contains("javascript:")
        || lower.ends_with("/")
            && lower.matches('/').count() <= 3 // e.g. https://example.com/ or https://example.com/news/
}

// ── Shared helpers ──────────────────────────────────────────────────────────

/// Extract article body text from an HTML page by pulling <p> content.
pub fn extract_body_text(html: &str) -> String {
    // Parse with the scraper crate — drop before any await.
    let document = Html::parse_document(html);

    // Try <article> first, then common body selectors, then fall back to all <p>.
    let containers = [
        "article",
        ".article-body",
        ".article-content",
        ".entry-content",
        ".post-content",
        ".story-body",
        "[itemprop=\"articleBody\"]",
    ];

    let p_sel = Selector::parse("p").unwrap();

    for container_sel_str in &containers {
        if let Ok(sel) = Selector::parse(container_sel_str) {
            if let Some(container) = document.select(&sel).next() {
                let paragraphs: Vec<String> = container
                    .select(&p_sel)
                    .map(|p| p.text().collect::<Vec<_>>().join(" ").trim().to_string())
                    .filter(|t| t.len() > 30) // skip tiny paragraphs (captions, etc.)
                    .collect();
                if paragraphs.len() >= 2 {
                    return paragraphs.join("\n\n");
                }
            }
        }
    }

    // Fallback: all <p> tags in the document with decent length.
    let paragraphs: Vec<String> = document
        .select(&p_sel)
        .map(|p| p.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .filter(|t| t.len() > 40)
        .collect();

    if paragraphs.len() >= 2 {
        paragraphs.join("\n\n")
    } else {
        String::new()
    }
}

async fn fetch_sitemap(publisher: &PublisherDef, sitemap_url: &str) -> Result<Vec<RawArticle>> {
    let client = build_client();
    let resp = client
        .get(sitemap_url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {}", sitemap_url))?;
    let body = resp.text().await?;

    let cutoff = Utc::now() - Duration::hours(48);

    let mut articles = Vec::new();

    for url_block in body.split("<url>").skip(1) {
        let url_end = url_block.find("</url>").unwrap_or(url_block.len());
        let block = &url_block[..url_end];

        let loc = extract_xml_tag(block, "loc").unwrap_or_default();
        if loc.is_empty() {
            continue;
        }

        let title = extract_xml_tag(block, "news:title")
            .map(|t| html_escape::decode_html_entities(&t).to_string())
            .unwrap_or_default();
        if title.is_empty() || title.len() < 10 {
            continue;
        }

        // Skip non-article content (roundups, galleries, etc.)
        if is_non_article_headline(&title) {
            continue;
        }

        let pub_date_str = extract_xml_tag(block, "news:publication_date").unwrap_or_default();
        let published_at = if !pub_date_str.is_empty() {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&pub_date_str) {
                let dt_utc = dt.with_timezone(&chrono::Utc);
                if dt_utc < cutoff {
                    continue;
                }
                dt_utc.to_rfc3339()
            } else if let Ok(d) = chrono::NaiveDate::parse_from_str(&pub_date_str, "%Y-%m-%d") {
                let dt = d.and_hms_opt(0, 0, 0).unwrap().and_utc();
                if dt < cutoff {
                    continue;
                }
                dt.to_rfc3339()
            } else {
                Utc::now().to_rfc3339()
            }
        } else {
            Utc::now().to_rfc3339()
        };

        let image_url = extract_xml_tag(block, "image:loc").unwrap_or_default();

        articles.push(RawArticle {
            id: stable_id(&loc),
            publisher_id: publisher.id.to_string(),
            original_url: loc,
            original_headline: title,
            translated_headline: String::new(),
            body_snippet: String::new(),
            body_text: String::new(),
            image_url,
            language: publisher.primary_language.to_string(),
            published_at,
            category: String::new(),
        });
    }

    log::info!(
        "[sitemap] {}: {} articles",
        publisher.id,
        articles.len()
    );
    Ok(articles)
}

/// Extract text content from the first occurrence of an XML tag.
fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let start_pos = xml.find(&open)?;
    let after_open = &xml[start_pos + open.len()..];
    let content_start = after_open.find('>')? + 1;
    let content = &after_open[content_start..];
    let end_pos = content.find(&close)?;
    let text = content[..end_pos].trim();
    let text = if text.starts_with("<![CDATA[") && text.ends_with("]]>") {
        &text[9..text.len() - 3]
    } else {
        text
    };
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

async fn fetch_publisher(publisher: &PublisherDef) -> Result<Vec<RawArticle>> {
    match &publisher.scrape {
        ScrapeMethod::Rss { url } => fetch_rss(publisher, url).await,
        ScrapeMethod::Html {
            url,
            article_sel,
            headline_sel,
            image_sel,
            link_attr,
            base_url,
        } => {
            fetch_html(
                publisher,
                url,
                article_sel,
                headline_sel,
                image_sel,
                link_attr,
                base_url,
            )
            .await
        }
        ScrapeMethod::Sitemap { url } => fetch_sitemap(publisher, url).await,
    }
}

/// Scrape a user-added RSS feed (no static PublisherDef required).
async fn fetch_rss_dynamic(id: &str, rss_url: &str) -> Result<Vec<RawArticle>> {
    let client = build_client();
    let resp = client
        .get(rss_url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {}", rss_url))?;
    let body = resp.bytes().await?;
    let feed = feed_rs::parser::parse(&body[..])
        .with_context(|| format!("failed to parse feed for {}", id))?;

    let cutoff = Utc::now() - Duration::hours(48);
    let mut articles = Vec::new();

    for entry in feed.entries {
        let title = match &entry.title {
            Some(t) => t.content.trim().to_string(),
            None => continue,
        };
        if title.is_empty() { continue; }

        let pub_date = entry.published.or(entry.updated);
        if let Some(date) = pub_date {
            if date < cutoff { continue; }
        }

        let url = match entry.links.first() {
            Some(link) => link.href.clone(),
            None => continue,
        };

        let raw_summary = entry.summary.as_ref().map(|s| s.content.clone()).unwrap_or_default();
        let snippet = strip_html(&raw_summary);
        let snippet = if let Some(pos) = snippet.find("The post ") { snippet[..pos].trim().to_string() } else { snippet };
        let snippet = if snippet.len() > 500 { snippet[..500].to_string() } else { snippet };

        let image_url = extract_image_url(&raw_summary).unwrap_or_default();
        let published_at = pub_date.unwrap_or_else(Utc::now).to_rfc3339();

        articles.push(RawArticle {
            id: stable_id(&url),
            publisher_id: id.to_string(),
            original_url: url,
            original_headline: title,
            translated_headline: String::new(),
            body_snippet: snippet,
            body_text: String::new(),
            image_url,
            language: "en".to_string(),
            published_at,
            category: String::new(),
        });
    }

    log::info!("[rss/custom] {}: {} articles", id, articles.len());
    Ok(articles)
}

/// Scrape all publishers in parallel.
/// Returns (articles, failed_publisher_ids).
pub async fn scrape_all(custom_pubs: &[CustomPublisherDef]) -> (Vec<RawArticle>, Vec<String>) {
    let publishers = all_publisher_defs();
    let static_futures: Vec<_> = publishers.iter().map(|p| fetch_publisher(p)).collect();
    let custom_futures: Vec<_> = custom_pubs.iter().map(|p| fetch_rss_dynamic(&p.id, &p.rss_url)).collect();

    let (static_results, custom_results) = futures::future::join(
        futures::future::join_all(static_futures),
        futures::future::join_all(custom_futures),
    ).await;

    let mut all_articles = Vec::new();
    let mut seen_urls = std::collections::HashSet::new();
    let mut failed: Vec<String> = Vec::new();

    for (pub_def, result) in publishers.iter().zip(static_results) {
        match result {
            Ok(articles) => {
                for a in articles {
                    if seen_urls.insert(a.original_url.clone()) { all_articles.push(a); }
                }
                log::info!("scraped {} OK", pub_def.id);
            }
            Err(e) => {
                log::warn!("scraper failed for {}: {}", pub_def.id, e);
                failed.push(pub_def.id.to_string());
            }
        }
    }

    for (pub_def, result) in custom_pubs.iter().zip(custom_results) {
        match result {
            Ok(articles) => {
                for a in articles {
                    if seen_urls.insert(a.original_url.clone()) { all_articles.push(a); }
                }
                log::info!("scraped custom {} OK", pub_def.id);
            }
            Err(e) => {
                log::warn!("scraper failed for custom {}: {}", pub_def.id, e);
                failed.push(pub_def.name.clone());
            }
        }
    }

    log::info!("total articles scraped: {}", all_articles.len());
    (all_articles, failed)
}
