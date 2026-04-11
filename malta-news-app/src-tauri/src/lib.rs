mod category;
mod clustering;
mod db;
mod models;
mod pipeline;
mod publishers;
mod scraper;
mod translate;

// ── iOS Foundation Models bridge ────────────────────────────────────────────
#[cfg(target_os = "ios")]
mod ios_ai {
    use std::ffi::{CStr, CString, c_char};

    extern "C" {
        /// Implemented in AISummary.swift via @_cdecl.
        /// inputJson  – UTF-8 JSON: {"headlines":["..."],"snippets":["..."]}
        /// outputBuf  – caller-allocated buffer; receives UTF-8 JSON: {"headline":"...","summary":"..."}
        /// bufLen     – size of outputBuf in bytes
        /// Returns true on success.
        fn merill_generate_summary(
            input_json: *const c_char,
            output_buf: *mut c_char,
            buf_len:    i32,
        ) -> bool;
    }

    pub fn generate(headlines: &[String], snippets: &[String]) -> Option<(String, String)> {
        let input = serde_json::json!({ "headlines": headlines, "snippets": snippets });
        let c_input = CString::new(input.to_string()).ok()?;
        let mut buf = vec![0i8; 32768];

        let ok = unsafe {
            merill_generate_summary(c_input.as_ptr(), buf.as_mut_ptr(), buf.len() as i32)
        };
        if !ok { return None; }

        let s = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_str().ok()?;
        let v: serde_json::Value = serde_json::from_str(s).ok()?;
        Some((
            v["headline"].as_str()?.to_string(),
            v["summary"].as_str()?.to_string(),
        ))
    }
}

fn generate_summary_impl(headlines: &[String], snippets: &[String]) -> (String, String) {
    #[cfg(target_os = "ios")]
    {
        if let Some(result) = ios_ai::generate(headlines, snippets) {
            return result;
        }
    }
    (
        headlines.first().cloned().unwrap_or_default(),
        snippets.iter().find(|s| !s.is_empty()).cloned().unwrap_or_default(),
    )
}

use rusqlite::Connection;
use std::sync::Mutex;
use std::time::Instant;
use tauri::Manager;

use models::{Article, ClustersResponse, RefreshResult, StoryCluster};
use publishers::publisher_info;

/// Minimum seconds between full re-scrapes. Within this window,
/// "refresh" just re-reads the DB (re-ordering cards) without hitting the network.
const SCRAPE_COOLDOWN_SECS: u64 = 5 * 60;

struct AppState {
    db: Mutex<Connection>,
    last_scraped: Mutex<Option<Instant>>,
}

// ── Tauri Commands ──────────────────────────────────────────────────────────

#[tauri::command]
async fn get_clusters(
    state: tauri::State<'_, AppState>,
    blindspots_only: bool,
) -> Result<ClustersResponse, String> {
    let conn = state.db.lock().unwrap();
    let raw = db::load_clusters_light(&conn, blindspots_only).map_err(|e| e.to_string())?;

    // Build a fast lookup for custom publishers so their is_global flag is correct.
    let custom_pub_map: std::collections::HashMap<String, models::PublisherInfo> =
        db::get_custom_publishers(&conn)
            .unwrap_or_default()
            .into_iter()
            .map(|p| {
                let info = models::PublisherInfo {
                    id: p.id.clone(),
                    name: p.name,
                    bias_category: models::BiasCategory::CommercialIndependent,
                    logo_url: favicon_from_url(&p.rss_url),
                    is_global: p.is_global,
                };
                (p.id, info)
            })
            .collect();

    let clusters: Vec<StoryCluster> = raw
        .into_iter()
        .map(|c| {
            let arts: Vec<Article> = c.articles
                .into_iter()
                .map(|a| {
                    let translated = if a.translated_headline.is_empty() {
                        a.headline.clone()
                    } else {
                        a.translated_headline
                    };
                    let pub_info = custom_pub_map
                        .get(&a.publisher_id)
                        .cloned()
                        .unwrap_or_else(|| publisher_info(&a.publisher_id));
                    Article {
                        id: a.id,
                        publisher_id: a.publisher_id.clone(),
                        publisher: pub_info,
                        original_url: a.original_url,
                        original_headline: a.headline,
                        translated_headline: translated,
                        snippet: a.snippet,
                        body_text: String::new(),
                        image_url: a.image_url,
                        language: a.language,
                        published_at: a.published_at,
                        story_cluster_id: a.cluster_id,
                        category: a.category,
                    }
                })
                .collect();
            StoryCluster {
                id: c.id,
                primary_headline: c.headline,
                first_reported_at: c.first_reported,
                last_updated: c.last_updated,
                is_blindspot: c.is_blindspot,
                ai_headline: c.ai_headline,
                ai_summary: c.ai_summary,
                articles: arts,
            }
        })
        .collect();

    Ok(ClustersResponse { clusters })
}

#[tauri::command]
async fn refresh_feed(state: tauri::State<'_, AppState>) -> Result<RefreshResult, String> {
    {
        let last = state.last_scraped.lock().unwrap();
        if let Some(ts) = *last {
            if ts.elapsed().as_secs() < SCRAPE_COOLDOWN_SECS {
                log::info!(
                    "scrape cooldown active ({:.0}s remaining), skipping re-scrape",
                    SCRAPE_COOLDOWN_SECS as f64 - ts.elapsed().as_secs_f64()
                );
                return Ok(RefreshResult {
                    message: "Refreshed from cache".to_string(),
                    failed_sources: Vec::new(),
                });
            }
        }
    }

    let result = pipeline::run(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    {
        let mut last = state.last_scraped.lock().unwrap();
        *last = Some(Instant::now());
    }

    Ok(RefreshResult {
        message: format!(
            "Scraped {}, {} new, {} clusters created",
            result.articles_scraped, result.articles_new, result.clusters_created
        ),
        failed_sources: result.failed_sources,
    })
}

#[tauri::command]
async fn fetch_article_body(
    state: tauri::State<'_, AppState>,
    article_id: String,
    url: String,
) -> Result<models::ArticleBody, String> {
    // Check if we already have body text cached in the DB.
    {
        let conn = state.db.lock().unwrap();
        let existing: Option<(String, String)> = conn
            .query_row(
                "SELECT body_text, image_url FROM articles WHERE id = ?1",
                rusqlite::params![article_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        if let Some((body, image)) = existing {
            // Only return early if we have both body text AND an image
            if !body.is_empty() && !image.is_empty() {
                return Ok(models::ArticleBody {
                    body_text: body,
                    image_url: image,
                });
            }
        }
    }

    // Fetch the page and extract body + image.
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let html = resp.text().await.map_err(|e| e.to_string())?;

    let body_text = scraper::extract_body_text(&html);
    let og_image = {
        let end = if html.len() > 100_000 {
            html.floor_char_boundary(100_000)
        } else {
            html.len()
        };
        scraper::extract_meta_image(&html[..end])
    };

    // Cache in DB.
    {
        let conn = state.db.lock().unwrap();
        if !body_text.is_empty() {
            conn.execute(
                "UPDATE articles SET body_text = ?1 WHERE id = ?2",
                rusqlite::params![body_text, article_id],
            )
            .ok();
        }
        if let Some(ref img) = og_image {
            conn.execute(
                "UPDATE articles SET image_url = ?1 WHERE id = ?2 AND (image_url IS NULL OR image_url = '')",
                rusqlite::params![img, article_id],
            )
            .ok();
        }
    }

    let image_url = og_image.unwrap_or_default();
    Ok(models::ArticleBody {
        body_text,
        image_url,
    })
}

#[derive(serde::Serialize)]
struct SummaryResult {
    headline: String,
    summary: String,
}

/// Generate (or return cached) AI headline + summary for a cluster.
/// On iOS 26+ uses Foundation Models; on other platforms returns the
/// best existing headline and first non-empty snippet.
#[tauri::command]
async fn generate_cluster_summary(
    state: tauri::State<'_, AppState>,
    cluster_id: String,
    headlines: Vec<String>,
    snippets: Vec<String>,
) -> Result<SummaryResult, String> {
    // Return cached result if already generated.
    {
        let conn = state.db.lock().unwrap();
        let cached: Option<(String, String)> = conn.query_row(
            "SELECT ai_headline, ai_summary FROM clusters WHERE id = ?1 AND ai_headline != ''",
            rusqlite::params![cluster_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();
        if let Some((h, s)) = cached {
            return Ok(SummaryResult { headline: h, summary: s });
        }
    }

    // Run the (potentially blocking) model call off the async executor.
    let (headline, summary) = tokio::task::spawn_blocking({
        let headlines = headlines.clone();
        let snippets  = snippets.clone();
        move || generate_summary_impl(&headlines, &snippets)
    }).await.map_err(|e| e.to_string())?;

    // Cache.
    {
        let conn = state.db.lock().unwrap();
        conn.execute(
            "UPDATE clusters SET ai_headline = ?1, ai_summary = ?2 WHERE id = ?3",
            rusqlite::params![headline, summary, cluster_id],
        ).map_err(|e| e.to_string())?;
    }

    Ok(SummaryResult { headline, summary })
}

#[tauri::command]
fn get_publishers(state: tauri::State<'_, AppState>) -> Vec<models::PublisherInfo> {
    let mut list: Vec<models::PublisherInfo> = publishers::all_publisher_defs()
        .iter()
        .map(|p| publishers::publisher_info(p.id))
        .collect();

    if let Ok(custom) = db::get_custom_publishers(&state.db.lock().unwrap()) {
        for p in custom {
            list.push(models::PublisherInfo {
                id: p.id,
                name: p.name,
                bias_category: models::BiasCategory::CommercialIndependent,
                logo_url: favicon_from_url(&p.rss_url),
                is_global: p.is_global,
            });
        }
    }
    list
}

#[tauri::command]
async fn add_custom_publisher(
    state: tauri::State<'_, AppState>,
    url: String,
    name: String,
    is_global: bool,
) -> Result<models::PublisherInfo, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    // Normalise: accept bare domains like "bbc.com" or "//bbc.com"
    let url = if url.starts_with("http://") || url.starts_with("https://") {
        url
    } else if url.starts_with("//") {
        format!("https:{}", url)
    } else {
        format!("https://{}", url.trim_start_matches('/'))
    };

    let resp = client.get(&url).send().await
        .map_err(|e| format!("Could not reach URL: {}", e))?;
    // Use the final URL after any redirects (e.g. bbc.com → bbc.co.uk)
    let final_url = resp.url().to_string();
    let content_type = resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;

    // ── Method 1: direct RSS/Atom ──────────────────────────────────────────
    enum Found { Rss { scrape_url: String, title: Option<String> }, Sitemap(String), Html { scrape_url: String, selector: String } }

    let found: Found = if let Ok(feed) = feed_rs::parser::parse(&bytes[..]) {
        Found::Rss { scrape_url: final_url.clone(), title: feed.title.map(|t| t.content) }
    } else if content_type.contains("html") || content_type.is_empty() {
        let html = String::from_utf8_lossy(&bytes);

        // ── Method 2: RSS via <link> discovery or common path probe ──────
        let rss_url = discover_feed_url(&html, &final_url)
            .or(probe_common_feed_paths(&client, &final_url).await);

        if let Some(feed_url) = rss_url {
            let resp2 = client.get(&feed_url).send().await
                .map_err(|e| format!("Found feed link but could not fetch it: {}", e))?;
            let bytes2 = resp2.bytes().await.map_err(|e| e.to_string())?;
            let feed = feed_rs::parser::parse(&bytes2[..])
                .map_err(|_| "Found a feed link but could not parse it".to_string())?;
            Found::Rss { scrape_url: feed_url, title: feed.title.map(|t| t.content) }
        } else {
            // ── Method 3: Google News sitemap ────────────────────────────
            match probe_sitemap_paths(&client, &final_url).await {
                Some(sitemap_url) => Found::Sitemap(sitemap_url),
                None => {
                    // ── Method 4: HTML auto-detect ───────────────────────
                    match scraper::auto_detect_article_sel(&html) {
                        Some(selector) => Found::Html { scrape_url: final_url.clone(), selector },
                        None => return Err(
                            "Could not find a feed, sitemap, or recognisable article structure at this URL.".to_string()
                        ),
                    }
                }
            }
        }
    } else {
        return Err("URL does not appear to be a feed or a news website.".to_string());
    };

    // ── Extract name and build the DB record ──────────────────────────────
    let page_title = || {
        // Best-effort title from the HTML we already have
        let html = String::from_utf8_lossy(&bytes);
        let lower = html.to_lowercase();
        lower.find("<title>").and_then(|s| {
            let start = s + 7;
            lower[start..].find("</title>").map(|e| html[start..start + e].trim().to_string())
        })
    };

    let (scrape_url, scrape_method, scrape_config, auto_name) = match found {
        Found::Rss { scrape_url, title } => (scrape_url, "rss".to_string(), String::new(), title),
        Found::Sitemap(sitemap_url) => (sitemap_url, "sitemap".to_string(), String::new(), page_title()),
        Found::Html { scrape_url, selector } => (scrape_url.clone(), "html".to_string(), selector, page_title()),
    };

    let resolved_name = if name.trim().is_empty() {
        auto_name.unwrap_or_else(|| {
            final_url.trim_end_matches('/').split('/').nth(2).unwrap_or("Unknown").to_string()
        })
    } else {
        name.trim().to_string()
    };

    log::info!("adding custom publisher '{}' via {} ({})", resolved_name, scrape_method, scrape_url);

    let id = format!("custom_{}", uuid::Uuid::new_v4().simple());
    let def = models::CustomPublisherDef {
        id: id.clone(),
        name: resolved_name.clone(),
        rss_url: scrape_url,
        scrape_method,
        scrape_config,
        is_global,
    };
    db::insert_custom_publisher(&state.db.lock().unwrap(), &def).map_err(|e| e.to_string())?;

    // Reset scrape cooldown so the next refresh actually fetches the new publisher
    *state.last_scraped.lock().unwrap() = None;

    Ok(models::PublisherInfo {
        id,
        name: resolved_name,
        bias_category: models::BiasCategory::CommercialIndependent,
        logo_url: favicon_from_url(&final_url),
        is_global,
    })
}

/// Derive a favicon URL from any URL by keeping only the scheme + host.
/// e.g. "https://bbc.com/news/rss.xml" → "https://bbc.com/favicon.ico"
fn favicon_from_url(url: &str) -> String {
    let after_scheme = url.find("://").map(|i| i + 3).unwrap_or(0);
    let host_end = url[after_scheme..]
        .find('/')
        .map(|i| i + after_scheme)
        .unwrap_or(url.len());
    if host_end > after_scheme {
        format!("{}/favicon.ico", &url[..host_end])
    } else {
        String::new()
    }
}

/// Scan HTML for <link rel="alternate" type="application/rss+xml" href="...">
/// or type="application/atom+xml". Returns an absolute URL if found.
fn discover_feed_url(html: &str, base_url: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let mut search = 0;
    while let Some(pos) = lower[search..].find("<link") {
        let abs = search + pos;
        let end = lower[abs..].find('>').map(|e| abs + e + 1).unwrap_or(html.len());
        let tag = &lower[abs..end];
        if (tag.contains("application/rss+xml") || tag.contains("application/atom+xml"))
            && tag.contains("alternate")
        {
            // Extract href value from the original (non-lowercased) tag
            let orig_tag = &html[abs..end];
            if let Some(href) = extract_attr(orig_tag, "href") {
                // Resolve relative URLs
                let absolute = if href.starts_with("http://") || href.starts_with("https://") {
                    href
                } else {
                    let path = href.trim_start_matches('/');
                    // Get origin from base_url
                    let origin = base_url.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
                    format!("{}/{}", origin.trim_end_matches('/'), path)
                };
                return Some(absolute);
            }
        }
        search = end;
    }
    None
}

/// Try well-known feed paths on the same origin in parallel.
/// Returns the first URL that responds with a valid RSS/Atom feed.
async fn probe_common_feed_paths(client: &reqwest::Client, base_url: &str) -> Option<String> {
    let origin = base_url.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
    let origin = origin.trim_end_matches('/');
    let paths = [
        "/feed",
        "/rss",
        "/rss.xml",
        "/feed.xml",
        "/feeds/rss.xml",
        "/atom.xml",
        "/index.xml",
        "/news/rss.xml",
        "/feeds/all.rss.xml",
    ];

    let futs: Vec<_> = paths.iter().map(|path| {
        let probe = format!("{}{}", origin, path);
        let client = client.clone();
        async move {
            if let Ok(resp) = client.get(&probe).send().await {
                if resp.status().is_success() {
                    if let Ok(bytes) = resp.bytes().await {
                        if feed_rs::parser::parse(&bytes[..]).is_ok() {
                            return Some(probe);
                        }
                    }
                }
            }
            None
        }
    }).collect();

    let results = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        futures::future::join_all(futs),
    ).await.unwrap_or_default();
    // Return in path-order so we prefer /feed over /rss.xml etc.
    results.into_iter().find_map(|r| r)
}

/// Probe common Google News sitemap paths. Returns the first URL that has <news:title> entries.
async fn probe_sitemap_paths(client: &reqwest::Client, base_url: &str) -> Option<String> {
    let origin = base_url.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
    let origin = origin.trim_end_matches('/');
    let paths = [
        "/sitemap_news.xml",
        "/news-sitemap.xml",
        "/sitemap.xml",
        "/sitemap_latest.xml",
        "/sitemap_index.xml",
    ];

    let futs: Vec<_> = paths.iter().map(|path| {
        let probe = format!("{}{}", origin, path);
        let client = client.clone();
        async move {
            if let Ok(resp) = client.get(&probe).send().await {
                if resp.status().is_success() {
                    if let Ok(body) = resp.text().await {
                        // Must have at least one <news:title> to count as a news sitemap
                        if body.contains("<news:title") {
                            return Some(probe);
                        }
                    }
                }
            }
            None
        }
    }).collect();

    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        futures::future::join_all(futs),
    ).await.unwrap_or_default().into_iter().find_map(|r| r)
}

fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{}=", attr);
    let lower_tag = tag.to_lowercase();
    let pos = lower_tag.find(&needle)?;
    let after = &tag[pos + needle.len()..];
    let (quote, rest) = if after.starts_with('"') {
        ('"', &after[1..])
    } else if after.starts_with('\'') {
        ('\'', &after[1..])
    } else {
        return None;
    };
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// Move a single article to its own new cluster (user-initiated split).
/// Returns the new cluster_id so the frontend can optimistically remove the row.
#[tauri::command]
async fn split_cluster(
    state: tauri::State<'_, AppState>,
    article_id: String,
    headline: String,
    published_at: String,
) -> Result<String, String> {
    let new_cluster_id = uuid::Uuid::new_v4().to_string();
    let conn = state.db.lock().unwrap();
    db::split_article_to_cluster(&conn, &article_id, &new_cluster_id, &headline, &published_at)
        .map_err(|e| e.to_string())?;
    Ok(new_cluster_id)
}

/// Wipe all cluster assignments and re-cluster every article in the DB from scratch.
#[tauri::command]
fn force_recluster(state: tauri::State<'_, AppState>) -> Result<String, String> {
    // Reset scrape cooldown so a subsequent normal refresh also runs.
    *state.last_scraped.lock().unwrap() = None;
    let result = pipeline::recluster_all(&state.db).map_err(|e| e.to_string())?;
    Ok(format!("{} clusters created", result.clusters_created))
}

#[tauri::command]
fn remove_custom_publisher(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    db::delete_custom_publisher(&state.db.lock().unwrap(), &id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn translate_summary(text: String, to: String) -> Result<String, String> {
    let from = if to == "mt" { "en" } else { "mt" };
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    translate::translate_text(&client, &text, from, &to)
        .await
        .map_err(|e| e.to_string())
}

// ── App Setup ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(move |app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Debug)
                        .build(),
                )?;
            }

            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir).ok();
            let db_path = data_dir.join("merill.db");
            log::info!("database at {}", db_path.display());
            let conn = db::open(&db_path).expect("failed to open database");

            // Prune articles older than 7 days at every startup to keep the DB small.
            match db::prune_old_articles(&conn, 7) {
                Ok(n) => log::info!("pruned {} old articles", n),
                Err(e) => log::warn!("pruning failed: {}", e),
            }

            app.manage(AppState {
                db: Mutex::new(conn),
                last_scraped: Mutex::new(None),
            });

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_clusters, get_publishers, refresh_feed, fetch_article_body, translate_summary, generate_cluster_summary, add_custom_publisher, remove_custom_publisher, split_cluster, force_recluster])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
