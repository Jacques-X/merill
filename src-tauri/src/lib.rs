mod category;
mod clustering;
mod db;
mod models;
mod native_bridge;
mod pipeline;
mod publishers;
mod scraper;
mod semantic_clustering;
mod translate;

// ── iOS Foundation Models bridge ────────────────────────────────────────────
#[cfg(target_os = "ios")]
mod ios_ai {
    use std::ffi::{CStr, CString, c_char, c_void};

    extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    type GenerateSummaryFn =
        unsafe extern "C" fn(*const c_char, *mut c_char, i32) -> bool;
    type GenerateEmbeddingsFn =
        unsafe extern "C" fn(*const c_char, *mut c_char, i32) -> bool;

    fn generate_summary_fn() -> Option<GenerateSummaryFn> {
        let symbol = CString::new("merill_generate_summary").ok()?;
        // RTLD_DEFAULT searches symbols already loaded into the app process.
        let address = unsafe { dlsym((-2isize) as *mut c_void, symbol.as_ptr()) };
        if address.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute::<*mut c_void, GenerateSummaryFn>(address) })
        }
    }

    fn generate_embeddings_fn() -> Option<GenerateEmbeddingsFn> {
        let symbol = CString::new("merill_generate_embeddings").ok()?;
        let address = unsafe { dlsym((-2isize) as *mut c_void, symbol.as_ptr()) };
        if address.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute::<*mut c_void, GenerateEmbeddingsFn>(address) })
        }
    }

    pub fn generate(headlines: &[String], snippets: &[String]) -> Option<(String, String)> {
        let generate_summary = generate_summary_fn()?;
        let input = serde_json::json!({ "headlines": headlines, "snippets": snippets });
        let c_input = CString::new(input.to_string()).ok()?;
        let mut buf = vec![0i8; 32768];

        let ok = unsafe {
            generate_summary(c_input.as_ptr(), buf.as_mut_ptr(), buf.len() as i32)
        };
        if !ok { return None; }

        let s = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_str().ok()?;
        let v: serde_json::Value = serde_json::from_str(s).ok()?;
        Some((
            v["headline"].as_str()?.to_string(),
            v["summary"].as_str()?.to_string(),
        ))
    }

    pub fn embed(texts: &[String]) -> Option<Vec<Vec<f32>>> {
        let generate_embeddings = generate_embeddings_fn()?;
        let c_input = CString::new(serde_json::to_string(texts).ok()?).ok()?;
        let mut buf = vec![0i8; texts.len().saturating_mul(8192).max(32768)];
        let ok = unsafe {
            generate_embeddings(c_input.as_ptr(), buf.as_mut_ptr(), buf.len() as i32)
        };
        if !ok {
            return None;
        }
        let json = unsafe { CStr::from_ptr(buf.as_ptr()) }.to_str().ok()?;
        serde_json::from_str(json).ok()
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

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use tauri::Manager;

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("failed to build HTTP client")
    })
}

type DbPool = Pool<SqliteConnectionManager>;

use models::{Article, ClustersResponse, RefreshResult, StoryCluster};
use publishers::publisher_info;

/// Minimum seconds between full re-scrapes. Within this window,
/// "refresh" just re-reads the DB (re-ordering cards) without hitting the network.
const SCRAPE_COOLDOWN_SECS: u64 = 5 * 60;

pub(crate) struct MerillCore {
    db: DbPool,
    last_scraped: Mutex<Option<Instant>>,
    refresh_status: Mutex<models::RefreshStatus>,
}

impl MerillCore {
    pub(crate) fn open(db_path: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        log::info!("database at {}", db_path.display());

        {
            let init_conn = db::open(db_path).map_err(|e| e.to_string())?;
            match db::prune_old_articles(&init_conn, 7) {
                Ok(n) => log::info!("pruned {} old articles", n),
                Err(e) => log::warn!("pruning failed: {}", e),
            }
        }

        let manager = r2d2_sqlite::SqliteConnectionManager::file(db_path)
            .with_init(db::setup_pragmas);
        let db = r2d2::Pool::builder()
            .max_size(5)
            .build(manager)
            .map_err(|e| e.to_string())?;

        Ok(Self {
            db,
            last_scraped: Mutex::new(None),
            refresh_status: Mutex::new(models::RefreshStatus {
                last_refresh_at: None,
                cooldown_remaining_seconds: 0,
                failed_sources: Vec::new(),
            }),
        })
    }
}

// ── Tauri Commands ──────────────────────────────────────────────────────────

fn normalize_story_text(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalized_story_url(value: &str) -> String {
    let without_fragment = value.split('#').next().unwrap_or(value);
    let without_query = without_fragment.split('?').next().unwrap_or(without_fragment);
    without_query.trim_end_matches('/').to_lowercase()
}

fn story_key_for_articles(articles: &[Article]) -> String {
    let representative = articles.iter().min_by(|a, b| {
        a.published_at
            .cmp(&b.published_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    let seed = representative
        .map(|article| {
            format!(
                "{}|{}",
                normalized_story_url(&article.original_url),
                normalize_story_text(&article.original_headline)
            )
        })
        .unwrap_or_default();
    use sha2::{Digest, Sha256};
    format!("story_{:x}", Sha256::digest(seed.as_bytes()))
}

fn comparison_terms(value: &str) -> HashSet<String> {
    const STOP: &[&str] = &[
        "about", "after", "before", "from", "have", "malta", "maltese", "news", "said",
        "says", "that", "their", "this", "with", "will", "għal", "mill", "jgħid", "qed",
    ];
    normalize_story_text(value)
        .split_whitespace()
        .filter(|word| word.chars().count() >= 4 && !STOP.contains(word))
        .map(str::to_string)
        .collect()
}

fn perspective_groups(articles: &[Article]) -> Vec<models::PerspectiveGroup> {
    let mut grouped: HashMap<models::BiasCategory, Vec<&Article>> = HashMap::new();
    for article in articles {
        grouped
            .entry(article.publisher.bias_category)
            .or_default()
            .push(article);
    }

    let mut groups: Vec<_> = grouped
        .into_iter()
        .map(|(bias_category, group_articles)| {
            let term_sets: Vec<HashSet<String>> = group_articles
                .iter()
                .map(|article| comparison_terms(&article.original_headline))
                .collect();
            let common = term_sets
                .first()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|term| term_sets.iter().all(|set| set.contains(term)))
                .collect::<HashSet<_>>();
            let group_terms: HashSet<String> =
                term_sets.iter().flat_map(|set| set.iter().cloned()).collect();
            let other_terms: HashSet<String> = articles
                .iter()
                .filter(|article| article.publisher.bias_category != bias_category)
                .flat_map(|article| comparison_terms(&article.original_headline))
                .collect();
            let mut common_terms: Vec<String> = common.into_iter().collect();
            let mut distinct_terms: Vec<String> = group_terms
                .difference(&other_terms)
                .cloned()
                .collect();
            common_terms.sort();
            distinct_terms.sort();
            common_terms.truncate(6);
            distinct_terms.truncate(6);

            models::PerspectiveGroup {
                bias_category,
                common_terms,
                distinct_terms,
                articles: group_articles
                    .into_iter()
                    .map(|article| models::PerspectiveArticle {
                        article_id: article.id.clone(),
                        publisher_id: article.publisher_id.clone(),
                        publisher_name: article.publisher.name.clone(),
                        headline: article.original_headline.clone(),
                        snippet: article.snippet.clone(),
                        published_at: article.published_at.clone(),
                    })
                    .collect(),
            }
        })
        .collect();
    groups.sort_by_key(|group| format!("{:?}", group.bias_category));
    groups
}

fn blindspot_explanation(articles: &[Article]) -> models::BlindspotExplanation {
    let mut covered_categories: Vec<_> = articles
        .iter()
        .map(|article| article.publisher.bias_category)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    covered_categories.sort_by_key(|category| format!("{:?}", category));
    let missing_independent_coverage = !covered_categories.iter().any(|category| {
        matches!(
            category,
            models::BiasCategory::CommercialIndependent
                | models::BiasCategory::InvestigativeIndependent
        )
    });
    models::BlindspotExplanation {
        covered_categories,
        missing_independent_coverage,
        publisher_count: articles
            .iter()
            .map(|article| article.publisher_id.as_str())
            .collect::<HashSet<_>>()
            .len(),
    }
}

fn cluster_rows_to_response(
    rows: Vec<db::ClusterRowLight>,
    custom_pub_map: &HashMap<String, models::PublisherInfo>,
    saved_story_keys: &HashMap<String, String>,
) -> ClustersResponse {
    let clusters = rows
        .into_iter()
        .map(|c| {
            let articles: Vec<Article> = c
                .articles
                .into_iter()
                .map(|a| {
                    let translated = if a.translated_headline.is_empty() {
                        a.headline.clone()
                    } else {
                        a.translated_headline
                    };
                    let publisher = custom_pub_map
                        .get(&a.publisher_id)
                        .cloned()
                        .unwrap_or_else(|| publisher_info(&a.publisher_id));
                    Article {
                        id: a.id,
                        publisher_id: a.publisher_id,
                        publisher,
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
                story_key: articles
                    .iter()
                    .find_map(|article| saved_story_keys.get(&article.id).cloned())
                    .unwrap_or_else(|| story_key_for_articles(&articles)),
                primary_headline: c.headline,
                first_reported_at: c.first_reported,
                last_updated: c.last_updated,
                is_blindspot: c.is_blindspot,
                ai_headline: c.ai_headline,
                ai_summary: c.ai_summary,
                blindspot_explanation: blindspot_explanation(&articles),
                perspective_groups: perspective_groups(&articles),
                articles,
            }
        })
        .collect();
    ClustersResponse { clusters }
}

fn custom_publisher_map(conn: &rusqlite::Connection) -> HashMap<String, models::PublisherInfo> {
    db::get_custom_publishers(conn)
        .unwrap_or_default()
        .into_iter()
        .map(|publisher| {
            let logo_url = custom_publisher_logo(conn, &publisher);
            let info = models::PublisherInfo {
                id: publisher.id.clone(),
                name: publisher.name,
                bias_category: models::BiasCategory::CommercialIndependent,
                logo_url,
                is_global: publisher.is_global,
            };
            (publisher.id, info)
        })
        .collect()
}

pub(crate) async fn get_clusters_core(
    state: &MerillCore,
    blindspots_only: bool,
) -> Result<ClustersResponse, String> {
    let conn = state.db.get().map_err(|e| e.to_string())?;
    let raw = db::load_clusters_light(&conn, blindspots_only).map_err(|e| e.to_string())?;
    let saved_story_keys = db::saved_story_keys_by_article(&conn).map_err(|e| e.to_string())?;

    Ok(cluster_rows_to_response(
        raw,
        &custom_publisher_map(&conn),
        &saved_story_keys,
    ))
}

pub(crate) fn get_saved_stories_core(state: &MerillCore) -> Result<ClustersResponse, String> {
    let conn = state.db.get().map_err(|e| e.to_string())?;
    let rows = db::load_saved_clusters_light(&conn).map_err(|e| e.to_string())?;
    let saved_story_keys = db::saved_story_keys_by_article(&conn).map_err(|e| e.to_string())?;
    Ok(cluster_rows_to_response(
        rows,
        &custom_publisher_map(&conn),
        &saved_story_keys,
    ))
}

pub(crate) fn search_stories_core(
    state: &MerillCore,
    query: String,
) -> Result<ClustersResponse, String> {
    if query.trim().is_empty() {
        return Ok(ClustersResponse { clusters: Vec::new() });
    }
    let conn = state.db.get().map_err(|e| e.to_string())?;
    let rows = db::search_clusters_light(&conn, &query).map_err(|e| e.to_string())?;
    let saved_story_keys = db::saved_story_keys_by_article(&conn).map_err(|e| e.to_string())?;
    Ok(cluster_rows_to_response(
        rows,
        &custom_publisher_map(&conn),
        &saved_story_keys,
    ))
}

pub(crate) fn save_story_core(
    state: &MerillCore,
    story_key: String,
    article_ids: Vec<String>,
) -> Result<(), String> {
    let mut conn = state.db.get().map_err(|e| e.to_string())?;
    db::save_story(&mut conn, &story_key, &article_ids).map_err(|e| e.to_string())
}

pub(crate) fn unsave_story_core(state: &MerillCore, story_key: String) -> Result<(), String> {
    let conn = state.db.get().map_err(|e| e.to_string())?;
    db::unsave_story(&conn, &story_key).map_err(|e| e.to_string())
}

pub(crate) fn get_refresh_status_core(state: &MerillCore) -> models::RefreshStatus {
    let mut status = state.refresh_status.lock().unwrap().clone();
    status.cooldown_remaining_seconds = state
        .last_scraped
        .lock()
        .unwrap()
        .map(|last| SCRAPE_COOLDOWN_SECS.saturating_sub(last.elapsed().as_secs()))
        .unwrap_or(0);
    status
}

#[tauri::command]
fn get_saved_stories(state: tauri::State<'_, MerillCore>) -> Result<ClustersResponse, String> {
    get_saved_stories_core(&state)
}

#[tauri::command]
fn search_stories(
    state: tauri::State<'_, MerillCore>,
    query: String,
) -> Result<ClustersResponse, String> {
    search_stories_core(&state, query)
}

#[tauri::command]
fn save_story(
    state: tauri::State<'_, MerillCore>,
    story_key: String,
    article_ids: Vec<String>,
) -> Result<(), String> {
    save_story_core(&state, story_key, article_ids)
}

#[tauri::command]
fn unsave_story(
    state: tauri::State<'_, MerillCore>,
    story_key: String,
) -> Result<(), String> {
    unsave_story_core(&state, story_key)
}

#[tauri::command]
fn get_refresh_status(state: tauri::State<'_, MerillCore>) -> models::RefreshStatus {
    get_refresh_status_core(&state)
}

#[tauri::command]
async fn get_clusters(
    state: tauri::State<'_, MerillCore>,
    blindspots_only: bool,
) -> Result<ClustersResponse, String> {
    get_clusters_core(&state, blindspots_only).await
}

pub(crate) async fn refresh_feed_core(state: &MerillCore) -> Result<RefreshResult, String> {
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
                    failed_sources: state.refresh_status.lock().unwrap().failed_sources.clone(),
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
    {
        let mut status = state.refresh_status.lock().unwrap();
        status.last_refresh_at = Some(chrono::Utc::now().to_rfc3339());
        status.failed_sources = result.failed_sources.clone();
        status.cooldown_remaining_seconds = SCRAPE_COOLDOWN_SECS;
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
async fn refresh_feed(state: tauri::State<'_, MerillCore>) -> Result<RefreshResult, String> {
    refresh_feed_core(&state).await
}

pub(crate) async fn fetch_article_body_core(
    state: &MerillCore,
    article_id: String,
    url: String,
) -> Result<models::ArticleBody, String> {
    // Check if we already have body text cached in the DB.
    {
        let conn = state.db.get().map_err(|e| e.to_string())?;
        let existing: Option<(String, String)> = conn
            .query_row(
                "SELECT body_text, image_url FROM articles WHERE id = ?1",
                rusqlite::params![article_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        if let Some((body, image)) = existing {
            // Return cached body if available — the caller already has the image from the
            // article's image_url field, so we don't need both to skip the network fetch.
            if !body.is_empty() {
                return Ok(models::ArticleBody {
                    body_text: body,
                    image_url: image,
                });
            }
        }
    }

    // Fetch the page and extract body + image.
    let resp = http_client().get(&url).send().await.map_err(|e| e.to_string())?;
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
        let conn = state.db.get().map_err(|e| e.to_string())?;
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

#[tauri::command]
async fn fetch_article_body(
    state: tauri::State<'_, MerillCore>,
    article_id: String,
    url: String,
) -> Result<models::ArticleBody, String> {
    fetch_article_body_core(&state, article_id, url).await
}

#[derive(serde::Serialize)]
pub(crate) struct SummaryResult {
    headline: String,
    summary: String,
}

/// Generate (or return cached) AI headline + summary for a cluster.
/// On iOS 26+ uses Foundation Models; on other platforms returns the
/// best existing headline and first non-empty snippet.
pub(crate) async fn generate_cluster_summary_core(
    state: &MerillCore,
    cluster_id: String,
    headlines: Vec<String>,
    snippets: Vec<String>,
) -> Result<SummaryResult, String> {
    // Return cached result if already generated.
    {
        let conn = state.db.get().map_err(|e| e.to_string())?;
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
        let conn = state.db.get().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE clusters SET ai_headline = ?1, ai_summary = ?2 WHERE id = ?3",
            rusqlite::params![headline, summary, cluster_id],
        ).map_err(|e| e.to_string())?;
    }

    Ok(SummaryResult { headline, summary })
}

#[tauri::command]
async fn generate_cluster_summary(
    state: tauri::State<'_, MerillCore>,
    cluster_id: String,
    headlines: Vec<String>,
    snippets: Vec<String>,
) -> Result<SummaryResult, String> {
    generate_cluster_summary_core(&state, cluster_id, headlines, snippets).await
}

pub(crate) fn get_publishers_core(state: &MerillCore) -> Vec<models::PublisherInfo> {
    let mut list: Vec<models::PublisherInfo> = publishers::all_publisher_defs()
        .iter()
        .map(|p| publishers::publisher_info(p.id))
        .collect();

    if let Ok(conn) = state.db.get() {
        if let Ok(custom) = db::get_custom_publishers(&conn) {
            for p in custom {
                let logo_url = custom_publisher_logo(&conn, &p);
                list.push(models::PublisherInfo {
                    id: p.id.clone(),
                    name: p.name,
                    bias_category: models::BiasCategory::CommercialIndependent,
                    logo_url,
                    is_global: p.is_global,
                });
            }
        }
    }
    list
}

#[tauri::command]
fn get_publishers(state: tauri::State<'_, MerillCore>) -> Vec<models::PublisherInfo> {
    get_publishers_core(&state)
}

pub(crate) async fn add_custom_publisher_core(
    state: &MerillCore,
    url: String,
    name: String,
    is_global: bool,
) -> Result<models::PublisherInfo, String> {
    // Normalise: accept bare domains like "bbc.com" or "//bbc.com"
    let url = if url.starts_with("http://") || url.starts_with("https://") {
        url
    } else if url.starts_with("//") {
        format!("https:{}", url)
    } else {
        format!("https://{}", url.trim_start_matches('/'))
    };

    let resp = http_client().get(&url).send().await
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
    enum Found {
        Rss {
            scrape_url: String,
            title: Option<String>,
            site_url: String,
        },
        Sitemap {
            scrape_url: String,
            site_url: String,
        },
        Html {
            scrape_url: String,
            selector: String,
            site_url: String,
        },
    }

    let found: Found = if let Ok(feed) = feed_rs::parser::parse(&bytes[..]) {
        let site_url = feed_site_url(&feed, &final_url);
        Found::Rss {
            scrape_url: final_url.clone(),
            title: feed.title.map(|t| t.content),
            site_url,
        }
    } else if content_type.contains("html") || content_type.is_empty() {
        let html = String::from_utf8_lossy(&bytes);

        // ── Method 2: RSS via <link> discovery or common path probe ──────
        let rss_url = discover_feed_url(&html, &final_url)
            .or(probe_common_feed_paths(&final_url).await);

        if let Some(feed_url) = rss_url {
            let resp2 = http_client().get(&feed_url).send().await
                .map_err(|e| format!("Found feed link but could not fetch it: {}", e))?;
            let bytes2 = resp2.bytes().await.map_err(|e| e.to_string())?;
            let feed = feed_rs::parser::parse(&bytes2[..])
                .map_err(|_| "Found a feed link but could not parse it".to_string())?;
            Found::Rss {
                scrape_url: feed_url,
                title: feed.title.map(|t| t.content),
                site_url: final_url.clone(),
            }
        } else {
            // ── Method 3: Google News sitemap ────────────────────────────
            match probe_sitemap_paths(&final_url).await {
                Some(sitemap_url) => Found::Sitemap {
                    scrape_url: sitemap_url,
                    site_url: final_url.clone(),
                },
                None => {
                    // ── Method 4: HTML auto-detect ───────────────────────
                    match scraper::auto_detect_article_sel(&html) {
                        Some(selector) => Found::Html {
                            scrape_url: final_url.clone(),
                            selector,
                            site_url: final_url.clone(),
                        },
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

    let (scrape_url, site_url, scrape_method, scrape_config, auto_name) = match found {
        Found::Rss { scrape_url, title, site_url } => {
            (scrape_url, site_url, "rss".to_string(), String::new(), title)
        }
        Found::Sitemap { scrape_url, site_url } => {
            (scrape_url, site_url, "sitemap".to_string(), String::new(), page_title())
        }
        Found::Html { scrape_url, selector, site_url } => {
            (scrape_url, site_url, "html".to_string(), selector, page_title())
        }
    };
    let logo_url = if site_url == final_url
        && (content_type.contains("html") || content_type.is_empty())
    {
        discover_icon_url(&String::from_utf8_lossy(&bytes), &site_url)
            .unwrap_or_else(|| favicon_from_url(&site_url))
    } else {
        discover_publisher_icon(&site_url).await
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
        site_url,
        logo_url: logo_url.clone(),
        scrape_method,
        scrape_config,
        is_global,
    };
    let conn = state.db.get().map_err(|e| e.to_string())?;
    db::insert_custom_publisher(&conn, &def).map_err(|e| e.to_string())?;

    // Reset scrape cooldown so the next refresh actually fetches the new publisher
    *state.last_scraped.lock().unwrap() = None;

    Ok(models::PublisherInfo {
        id,
        name: resolved_name,
        bias_category: models::BiasCategory::CommercialIndependent,
        logo_url,
        is_global,
    })
}

#[tauri::command]
async fn add_custom_publisher(
    state: tauri::State<'_, MerillCore>,
    url: String,
    name: String,
    is_global: bool,
) -> Result<models::PublisherInfo, String> {
    add_custom_publisher_core(&state, url, name, is_global).await
}

/// Derive a favicon URL from any URL by keeping only the scheme + host.
/// e.g. "https://bbc.com/news/rss.xml" → "https://bbc.com/favicon.ico"
fn favicon_from_url(url: &str) -> String {
    url_origin(url)
        .map(|origin| format!("{}/favicon.ico", origin.trim_end_matches('/')))
        .unwrap_or_default()
}

fn custom_publisher_logo(
    conn: &rusqlite::Connection,
    publisher: &models::CustomPublisherDef,
) -> String {
    if !publisher.logo_url.is_empty() {
        return publisher.logo_url.clone();
    }
    if !publisher.site_url.is_empty() {
        return favicon_from_url(&publisher.site_url);
    }
    let fallback_url = db::latest_article_url_for_publisher(conn, &publisher.id)
        .ok()
        .flatten()
        .unwrap_or_else(|| publisher.rss_url.clone());
    favicon_from_url(&fallback_url)
}

fn url_origin(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let mut origin = format!("{}://{}", parsed.scheme(), host);
    if let Some(port) = parsed.port() {
        origin.push_str(&format!(":{}", port));
    }
    Some(origin)
}

fn resolve_url(base_url: &str, href: &str) -> Option<String> {
    reqwest::Url::parse(base_url)
        .ok()?
        .join(href.trim())
        .ok()
        .map(Into::into)
}

fn feed_site_url(feed: &feed_rs::model::Feed, feed_url: &str) -> String {
    feed.links
        .iter()
        .find(|link| link.rel.as_deref().is_none_or(|rel| rel == "alternate"))
        .or_else(|| feed.links.first())
        .and_then(|link| resolve_url(feed_url, &link.href))
        .or_else(|| url_origin(feed_url))
        .unwrap_or_else(|| feed_url.to_string())
}

async fn discover_publisher_icon(site_url: &str) -> String {
    let fallback = favicon_from_url(site_url);
    let Ok(response) = http_client().get(site_url).send().await else {
        return fallback;
    };
    let final_url = response.url().to_string();
    let Ok(html) = response.text().await else {
        return fallback;
    };
    discover_icon_url(&html, &final_url).unwrap_or(fallback)
}

fn discover_icon_url(html: &str, base_url: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let mut search = 0;
    let mut best: Option<(u8, String)> = None;
    while let Some(pos) = lower[search..].find("<link") {
        let abs = search + pos;
        let end = lower[abs..]
            .find('>')
            .map(|offset| abs + offset + 1)
            .unwrap_or(html.len());
        let tag = &html[abs..end];
        let rel = extract_attr(tag, "rel").unwrap_or_default().to_lowercase();
        if rel.split_whitespace().any(|value| value.contains("icon")) {
            if let Some(href) = extract_attr(tag, "href") {
                if let Some(icon_url) = resolve_url(base_url, &href) {
                    let tag_lower = tag.to_lowercase();
                    let score = if rel.contains("apple-touch-icon")
                        || tag_lower.contains("192x192")
                        || tag_lower.contains("180x180")
                    {
                        3
                    } else if href.to_lowercase().ends_with(".png") {
                        2
                    } else {
                        1
                    };
                    if best.as_ref().is_none_or(|(best_score, _)| score > *best_score) {
                        best = Some((score, icon_url));
                    }
                }
            }
        }
        search = end;
    }
    best.map(|(_, url)| url)
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
                return resolve_url(base_url, &href);
            }
        }
        search = end;
    }
    None
}

/// Try well-known feed paths on the same origin in parallel.
/// Returns the first URL that responds with a valid RSS/Atom feed.
async fn probe_common_feed_paths(base_url: &str) -> Option<String> {
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
        async move {
            if let Ok(resp) = http_client().get(&probe).send().await {
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
async fn probe_sitemap_paths(base_url: &str) -> Option<String> {
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
        async move {
            if let Ok(resp) = http_client().get(&probe).send().await {
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
pub(crate) async fn split_cluster_core(
    state: &MerillCore,
    article_id: String,
    headline: String,
    published_at: String,
) -> Result<String, String> {
    let new_cluster_id = uuid::Uuid::new_v4().to_string();
    let conn = state.db.get().map_err(|e| e.to_string())?;
    db::split_article_to_cluster(&conn, &article_id, &new_cluster_id, &headline, &published_at)
        .map_err(|e| e.to_string())?;
    Ok(new_cluster_id)
}

#[tauri::command]
async fn split_cluster(
    state: tauri::State<'_, MerillCore>,
    article_id: String,
    headline: String,
    published_at: String,
) -> Result<String, String> {
    split_cluster_core(&state, article_id, headline, published_at).await
}

/// Undo a user-initiated cluster split: moves the article back to its prior cluster
/// and removes the cannot-link constraint so future reclusters can re-merge it.
#[tauri::command]
fn revert_split(
    state: tauri::State<'_, MerillCore>,
    article_id: String,
) -> Result<(), String> {
    let conn = state.db.get().map_err(|e| e.to_string())?;
    db::revert_user_split(&conn, &article_id).map_err(|e| e.to_string())
}

/// Delete all articles and clusters and reset the scrape cooldown.
pub(crate) fn wipe_all_data_core(state: &MerillCore) -> Result<(), String> {
    let conn = state.db.get().map_err(|e| e.to_string())?;
    db::wipe_all_data(&conn).map_err(|e| e.to_string())?;
    *state.last_scraped.lock().unwrap() = None;
    Ok(())
}

#[tauri::command]
fn wipe_all_data(state: tauri::State<'_, MerillCore>) -> Result<(), String> {
    wipe_all_data_core(&state)
}

/// Wipe all cluster assignments and re-cluster every article in the DB from scratch.
pub(crate) fn force_recluster_core(state: &MerillCore) -> Result<String, String> {
    // Reset scrape cooldown so a subsequent normal refresh also runs.
    *state.last_scraped.lock().unwrap() = None;
    let result = pipeline::recluster_all(&state.db).map_err(|e| e.to_string())?;
    Ok(format!("{} clusters created", result.clusters_created))
}

#[tauri::command]
fn force_recluster(state: tauri::State<'_, MerillCore>) -> Result<String, String> {
    force_recluster_core(&state)
}

pub(crate) fn remove_custom_publisher_core(
    state: &MerillCore,
    id: String,
) -> Result<(), String> {
    let conn = state.db.get().map_err(|e| e.to_string())?;
    db::delete_custom_publisher(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_custom_publisher(
    state: tauri::State<'_, MerillCore>,
    id: String,
) -> Result<(), String> {
    remove_custom_publisher_core(&state, id)
}

pub(crate) async fn translate_summary_core(text: String, to: String) -> Result<String, String> {
    let from = if to == "mt" { "en" } else { "mt" };
    translate::translate_long_text(http_client(), &text, from, &to)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn translate_summary(text: String, to: String) -> Result<String, String> {
    translate_summary_core(text, to).await
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
            let db_path = data_dir.join("merill.db");
            app.manage(MerillCore::open(&db_path).expect("failed to open database"));

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_clusters, get_saved_stories, search_stories, save_story, unsave_story, get_refresh_status, get_publishers, refresh_feed, fetch_article_body, translate_summary, generate_cluster_summary, add_custom_publisher, remove_custom_publisher, split_cluster, revert_split, force_recluster, wipe_all_data])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod product_tests {
    use super::*;

    fn article(id: &str, published_at: &str, publisher: models::PublisherInfo) -> Article {
        Article {
            id: id.to_string(),
            publisher_id: publisher.id.clone(),
            publisher,
            original_url: format!("https://example.com/news/{id}?tracking=1"),
            original_headline: "Harbour project approved in Valletta".to_string(),
            translated_headline: String::new(),
            snippet: String::new(),
            body_text: String::new(),
            image_url: String::new(),
            language: "en".to_string(),
            published_at: published_at.to_string(),
            story_cluster_id: "cluster".to_string(),
            category: "local".to_string(),
        }
    }

    #[test]
    fn story_key_is_stable_across_article_order() {
        let publisher = publishers::publisher_info("times_of_malta");
        let first = article("first", "2026-01-01T00:00:00Z", publisher.clone());
        let second = article("second", "2026-01-01T01:00:00Z", publisher);
        assert_eq!(
            story_key_for_articles(&[first.clone(), second.clone()]),
            story_key_for_articles(&[second, first])
        );
    }

    #[test]
    fn blindspot_explanation_detects_missing_independent_coverage() {
        let article = article("party", "2026-01-01T00:00:00Z", publishers::publisher_info("one_news"));
        let explanation = blindspot_explanation(&[article]);
        assert!(explanation.missing_independent_coverage);
        assert_eq!(explanation.publisher_count, 1);
    }

    #[test]
    fn publisher_icon_discovery_resolves_relative_urls() {
        let html = r#"
            <html>
              <head>
                <link rel="icon" href="/assets/favicon.ico">
                <link rel="apple-touch-icon" sizes="180x180" href="images/touch.png">
              </head>
            </html>
        "#;

        assert_eq!(
            discover_icon_url(html, "https://news.example.com/section/"),
            Some("https://news.example.com/section/images/touch.png".to_string())
        );
    }
}
