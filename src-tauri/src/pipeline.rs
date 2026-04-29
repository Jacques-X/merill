use anyhow::Result;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::collections::{HashMap, HashSet};

use crate::category;
use crate::clustering;
use crate::db;
use crate::models::RawArticle;
use crate::scraper;
use crate::translate;

// ── Cluster text helpers ─────────────────────────────────────────────────────

/// Extract word-like tokens from the URL slug and return them space-joined.
/// Skips numeric-only segments and slugs with fewer than 2 word tokens. (#4)
fn url_slug_words(url: &str) -> String {
    let after_scheme = url.find("://").map(|i| i + 3).unwrap_or(0);
    let host_end = url[after_scheme..]
        .find('/')
        .map(|i| after_scheme + i)
        .unwrap_or(url.len());
    let path = &url[host_end..];

    let slug = path
        .split('/')
        .filter(|s| !s.is_empty())
        .last()
        .unwrap_or("");
    let slug = slug.split('.').next().unwrap_or(slug); // strip extension

    let words: Vec<&str> = slug
        .split(|c: char| c == '-' || c == '_')
        .filter(|w| w.len() > 3 && !w.chars().all(|c| c.is_ascii_digit()))
        .collect();

    if words.len() < 2 {
        String::new()
    } else {
        words.join(" ")
    }
}

/// Build the text used for TF-IDF clustering:
///   "[headline] . [snippet] . [url-slug-words]"
/// Falls back to headline-only when snippet and slug are empty. (#4)
fn cluster_text(headline: &str, snippet: &str, url: &str) -> String {
    let mut parts = vec![headline.to_string()];
    let s = snippet.trim();
    if !s.is_empty() {
        parts.push(s.to_string());
    }
    let slug = url_slug_words(url);
    if !slug.is_empty() {
        parts.push(slug);
    }
    parts.join(" . ")
}

/// Extract capitalised words and numbers from a Maltese headline to append to
/// the English cluster text. Guards against translators mangling proper nouns. (#19)
fn mt_entity_words(mt_headline: &str) -> String {
    let words: Vec<&str> = mt_headline
        .split_whitespace()
        .filter(|w| {
            // Strip leading punctuation to get a clean first char
            let clean = w.trim_matches(|c: char| !c.is_alphanumeric());
            if clean.len() < 3 {
                return false;
            }
            let first = clean.chars().next().unwrap();
            first.is_uppercase() || clean.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        .collect();
    words.join(" ")
}

/// Determine the dominant category for a set of per-article categories.
/// Returns "general" if articles span multiple categories.
fn dominant_category(cats: &[&str]) -> String {
    if cats.is_empty() {
        return "general".to_string();
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for &c in cats {
        *counts.entry(c).or_insert(0) += 1;
    }
    if counts.len() == 1 {
        return cats[0].to_string();
    }
    counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .filter(|(_, c)| *c >= 2) // require at least 2 votes
        .map(|(k, _)| k.to_string())
        .unwrap_or_else(|| "general".to_string())
}

pub struct PipelineResult {
    pub articles_scraped: usize,
    pub articles_new: usize,
    pub clusters_created: usize,
    pub failed_sources: Vec<String>,
}

/// Store → cluster by keywords → blindspot.
fn process(
    db: &Pool<SqliteConnectionManager>,
    raw_articles: Vec<RawArticle>,
) -> Result<PipelineResult> {
    let scraped_count = raw_articles.len();

    // 1. Store new articles in a single transaction.
    let new_articles = {
        let conn = db.get()?;
        conn.execute_batch("BEGIN")?;
        let result: Result<Vec<crate::models::RawArticle>> = (|| {
            let mut new = Vec::new();
            for a in &raw_articles {
                if db::insert_article(&conn, a)? {
                    if !a.translated_headline.is_empty() {
                        db::set_translated_headline(&conn, &a.id, &a.translated_headline)?;
                    }
                    new.push(a.clone());
                }
            }
            Ok(new)
        })();
        match result {
            Ok(new) => { conn.execute_batch("COMMIT")?; new }
            Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
        }
    };
    drop(raw_articles);

    let new_count = new_articles.len();
    log::info!("{} new articles inserted", new_count);

    if new_articles.is_empty() {
        return Ok(PipelineResult {
            articles_scraped: scraped_count,
            articles_new: 0,
            clusters_created: 0,
            failed_sources: Vec::new(),
        });
    }

    let conn = db.get()?;

    // 2. Build cluster data map from DB.
    // pub_map: cluster_id → (first_reported, last_updated, publisher_ids)
    let mut pub_map: HashMap<String, (String, String, Vec<String>)> =
        db::load_cluster_publishers(&conn)?
            .into_iter()
            .map(|(cid, _, first, last, pubs)| (cid, (first, last, pubs)))
            .collect();
    // article_data: cluster_id → Vec<(headline, translated, language, publisher_id, snippet, category)>
    let mut article_data: HashMap<String, Vec<(String, String, String, String, String, String)>> =
        db::load_all_cluster_headlines(&conn)?;

    let mut cluster_data: HashMap<String, clustering::ClusterData> = HashMap::new();
    for (cid, (_, last_updated, pubs)) in &pub_map {
        let articles = article_data.get(cid).map(|v| v.as_slice()).unwrap_or(&[]);
        let mut headlines = Vec::with_capacity(articles.len());
        let mut tokenized_headlines = Vec::with_capacity(articles.len());
        let mut token_set: HashSet<String> = HashSet::new();
        let cats: Vec<&str> = articles.iter().map(|(_, _, _, _, _, c)| c.as_str()).collect();

        for (h, t, lang, _, snippet, _) in articles {
            let en = if lang == "en" { h.clone() } else if !t.is_empty() { t.clone() } else { h.clone() };
            // No URL available for already-stored articles, so skip slug for existing clusters
            let text = if !snippet.is_empty() {
                format!("{} . {}", en, snippet.trim())
            } else {
                en
            };
            let tokens = clustering::tokenize_weighted(&text);
            for tok in &tokens {
                token_set.insert(tok.word.clone());
            }
            tokenized_headlines.push(tokens);
            headlines.push(text);
        }

        cluster_data.insert(cid.clone(), clustering::ClusterData {
            headlines,
            tokenized_headlines,
            token_set,
            last_updated: last_updated.clone(),
            category: Some(dominant_category(&cats)),
            publisher_ids: pubs.iter().cloned().collect(),
        });
    }

    log::info!("{} existing clusters loaded for matching", cluster_data.len());

    let mut idf = clustering::build_idf_table(&cluster_data);
    let mut new_vocab_since_refresh: usize = 0;
    const VOCAB_REFRESH_THRESHOLD: usize = 50;

    // 3. Greedy per-article cluster assignment.
    let mut clusters_created = 0;

    conn.execute_batch("BEGIN")?;
    let cluster_result: Result<()> = (|| {
        for article in &new_articles {
            let headline = if article.language == "en" {
                &article.original_headline
            } else if !article.translated_headline.is_empty() {
                &article.translated_headline
            } else {
                &article.original_headline
            };

            // Build cluster text: EN headline + snippet + URL slug (#4)
            let mut text = cluster_text(headline, &article.body_snippet, &article.original_url);

            // Append MT proper nouns alongside the English translation (#19)
            if article.language != "en" {
                let extras = mt_entity_words(&article.original_headline);
                if !extras.is_empty() {
                    text = format!("{} . {}", text, extras);
                }
            }

            let new_cluster_id = uuid::Uuid::new_v4().to_string();

            let assignment = clustering::assign_cluster(
                &text,
                &article.published_at,
                &article.category,
                &article.publisher_id,
                &cluster_data,
                &idf,
                &new_cluster_id,
            );

            db::set_cluster(&conn, &article.id, &assignment.cluster_id)?;

            if assignment.is_new {
                clusters_created += 1;
                log::info!("New cluster: {:?}", headline);

                let tokens = clustering::tokenize_weighted(&text);
                let token_set: HashSet<String> = tokens.iter().map(|t| t.word.clone()).collect();
                new_vocab_since_refresh += token_set.iter().filter(|w| !idf.contains_key(w.as_str())).count();

                let mut pub_set = HashSet::new();
                pub_set.insert(article.publisher_id.clone());

                cluster_data.insert(assignment.cluster_id.clone(), clustering::ClusterData {
                    headlines: vec![text.clone()],
                    tokenized_headlines: vec![tokens],
                    token_set,
                    last_updated: article.published_at.clone(),
                    category: Some(article.category.clone()),
                    publisher_ids: pub_set,
                });

                if new_vocab_since_refresh >= VOCAB_REFRESH_THRESHOLD {
                    idf = clustering::build_idf_table(&cluster_data);
                    new_vocab_since_refresh = 0;
                }

                pub_map.insert(assignment.cluster_id.clone(), (
                    article.published_at.clone(),
                    article.published_at.clone(),
                    vec![article.publisher_id.clone()],
                ));
                article_data.entry(assignment.cluster_id.clone()).or_default().push((
                    article.original_headline.clone(),
                    article.translated_headline.clone(),
                    article.language.clone(),
                    article.publisher_id.clone(),
                    article.body_snippet.clone(),
                    article.category.clone(),
                ));

                db::upsert_cluster(
                    &conn,
                    &assignment.cluster_id,
                    headline,
                    &article.published_at,
                    &article.published_at,
                    false,
                )?;
            } else {
                log::info!("Joined cluster: {} -> {}", headline, &assignment.cluster_id);

                if let Some(data) = cluster_data.get_mut(&assignment.cluster_id) {
                    let tokens = clustering::tokenize_weighted(&text);
                    for t in &tokens {
                        data.token_set.insert(t.word.clone());
                    }
                    data.tokenized_headlines.push(tokens);
                    data.headlines.push(text);
                    if article.published_at > data.last_updated {
                        data.last_updated = article.published_at.clone();
                    }
                    data.publisher_ids.insert(article.publisher_id.clone());
                    // Update dominant category if needed
                    let cats: Vec<&str> = article_data
                        .get(&assignment.cluster_id)
                        .map(|v| v.iter().map(|(_, _, _, _, _, c)| c.as_str()).collect())
                        .unwrap_or_default();
                    data.category = Some(dominant_category(&cats));
                }

                if let Some((_, last, pubs)) = pub_map.get_mut(&assignment.cluster_id) {
                    if article.published_at > *last {
                        *last = article.published_at.clone();
                    }
                    if !pubs.contains(&article.publisher_id) {
                        pubs.push(article.publisher_id.clone());
                    }
                }
                article_data.entry(assignment.cluster_id.clone()).or_default().push((
                    article.original_headline.clone(),
                    article.translated_headline.clone(),
                    article.language.clone(),
                    article.publisher_id.clone(),
                    article.body_snippet.clone(),
                    article.category.clone(),
                ));

                db::upsert_cluster(
                    &conn,
                    &assignment.cluster_id,
                    headline,
                    &article.published_at,
                    &article.published_at,
                    false,
                )?;
            }
        }
        Ok(())
    })();
    match cluster_result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    drop(new_articles);

    // 4. Second-pass cluster-to-cluster merge (#7)
    let merges = clustering::find_cluster_merges(&cluster_data, &idf);
    if !merges.is_empty() {
        log::info!("Second-pass merge: {} cluster pairs", merges.len());
        conn.execute_batch("BEGIN")?;
        let merge_result: Result<()> = (|| {
            for (from_id, to_id) in &merges {
                db::merge_cluster_articles(&conn, from_id, to_id)?;
                // Merge in-memory pub_map and article_data for the blindspot pass
                if let Some((from_first, from_last, from_pubs)) = pub_map.remove(from_id) {
                    if let Some((_, to_last, to_pubs)) = pub_map.get_mut(to_id) {
                        if from_last > *to_last { *to_last = from_last; }
                        for p in from_pubs { if !to_pubs.contains(&p) { to_pubs.push(p); } }
                    } else {
                        pub_map.insert(to_id.clone(), (from_first, from_last, from_pubs));
                    }
                }
                if let Some(from_articles) = article_data.remove(from_id) {
                    article_data.entry(to_id.clone()).or_default().extend(from_articles);
                }
            }
            Ok(())
        })();
        match merge_result {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(e) => { let _ = conn.execute_batch("ROLLBACK"); log::warn!("merge pass failed: {}", e); }
        }
    }

    drop(cluster_data);

    // 5. Blindspot analysis + best headline selection.
    let empty_vec = Vec::new();
    conn.execute_batch("BEGIN")?;
    let blindspot_result: Result<()> = (|| {
        for (cid, (first, last, pub_ids)) in &pub_map {
            let pub_refs: Vec<&str> = pub_ids.iter().map(|s| s.as_str()).collect();
            let is_blind = clustering::is_blindspot(&pub_refs);

            let articles = article_data.get(cid).unwrap_or(&empty_vec);
            let best_headline = clustering::pick_best_headline(articles);

            db::upsert_cluster(&conn, cid, &best_headline, first, last, is_blind)?;
        }
        Ok(())
    })();
    match blindspot_result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    Ok(PipelineResult {
        articles_scraped: scraped_count,
        articles_new: new_count,
        clusters_created,
        failed_sources: Vec::new(),
    })
}

/// Re-cluster every article already in the database without re-scraping.
pub fn recluster_all(db: &Pool<SqliteConnectionManager>) -> Result<PipelineResult> {
    let conn = db.get()?;

    db::wipe_clusters(&conn)?;
    log::info!("wiped all clusters for re-clustering");

    let articles = db::load_articles_for_recluster(&conn)?;
    log::info!("{} articles to re-cluster", articles.len());

    let mut cluster_data: HashMap<String, clustering::ClusterData> = HashMap::new();
    let mut pub_map: HashMap<String, (String, String, Vec<String>)> = HashMap::new();
    let mut article_data: HashMap<String, Vec<(String, String, String, String, String, String)>> = HashMap::new();
    let mut idf = clustering::build_idf_table(&cluster_data);
    let mut new_vocab_since_refresh: usize = 0;
    const VOCAB_REFRESH_THRESHOLD: usize = 50;
    let mut clusters_created = 0;

    conn.execute_batch("BEGIN")?;
    let recluster_result: Result<()> = (|| {
        for (article_id, original_headline, translated_headline, language, published_at, snippet, publisher_id, category, original_url) in &articles {
            let headline = if language == "en" {
                original_headline
            } else if !translated_headline.is_empty() {
                translated_headline
            } else {
                original_headline
            };

            let mut text = cluster_text(headline, snippet, original_url);
            if language != "en" {
                let extras = mt_entity_words(original_headline);
                if !extras.is_empty() {
                    text = format!("{} . {}", text, extras);
                }
            }

            let new_cluster_id = uuid::Uuid::new_v4().to_string();
            let assignment = clustering::assign_cluster(
                &text,
                published_at,
                category,
                publisher_id,
                &cluster_data,
                &idf,
                &new_cluster_id,
            );

            db::set_cluster(&conn, article_id, &assignment.cluster_id)?;

            if assignment.is_new {
                clusters_created += 1;
                let tokens = clustering::tokenize_weighted(&text);
                let token_set: HashSet<String> = tokens.iter().map(|t| t.word.clone()).collect();
                new_vocab_since_refresh += token_set.iter().filter(|w| !idf.contains_key(w.as_str())).count();

                let mut pub_set = HashSet::new();
                pub_set.insert(publisher_id.clone());

                cluster_data.insert(assignment.cluster_id.clone(), clustering::ClusterData {
                    headlines: vec![text.clone()],
                    tokenized_headlines: vec![tokens],
                    token_set,
                    last_updated: published_at.clone(),
                    category: Some(category.clone()),
                    publisher_ids: pub_set,
                });

                if new_vocab_since_refresh >= VOCAB_REFRESH_THRESHOLD {
                    idf = clustering::build_idf_table(&cluster_data);
                    new_vocab_since_refresh = 0;
                }

                pub_map.insert(assignment.cluster_id.clone(), (
                    published_at.clone(),
                    published_at.clone(),
                    vec![publisher_id.clone()],
                ));
                article_data.entry(assignment.cluster_id.clone()).or_default().push((
                    original_headline.clone(),
                    translated_headline.clone(),
                    language.clone(),
                    publisher_id.clone(),
                    snippet.clone(),
                    category.clone(),
                ));
                db::upsert_cluster(&conn, &assignment.cluster_id, headline, published_at, published_at, false)?;
            } else {
                if let Some(data) = cluster_data.get_mut(&assignment.cluster_id) {
                    let tokens = clustering::tokenize_weighted(&text);
                    for t in &tokens { data.token_set.insert(t.word.clone()); }
                    data.tokenized_headlines.push(tokens);
                    data.headlines.push(text);
                    if published_at > &data.last_updated { data.last_updated = published_at.clone(); }
                    data.publisher_ids.insert(publisher_id.clone());
                }
                if let Some((_, last, pubs)) = pub_map.get_mut(&assignment.cluster_id) {
                    if published_at > last { *last = published_at.clone(); }
                    if !pubs.contains(publisher_id) { pubs.push(publisher_id.clone()); }
                }
                article_data.entry(assignment.cluster_id.clone()).or_default().push((
                    original_headline.clone(),
                    translated_headline.clone(),
                    language.clone(),
                    publisher_id.clone(),
                    snippet.clone(),
                    category.clone(),
                ));
                db::upsert_cluster(&conn, &assignment.cluster_id, headline, published_at, published_at, false)?;
            }
        }
        Ok(())
    })();
    match recluster_result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    // Second-pass merge for recluster
    let merges = clustering::find_cluster_merges(&cluster_data, &idf);
    if !merges.is_empty() {
        log::info!("Recluster second-pass merge: {} pairs", merges.len());
        conn.execute_batch("BEGIN")?;
        let merge_result: Result<()> = (|| {
            for (from_id, to_id) in &merges {
                db::merge_cluster_articles(&conn, from_id, to_id)?;
                if let Some((from_first, from_last, from_pubs)) = pub_map.remove(from_id) {
                    if let Some((_, to_last, to_pubs)) = pub_map.get_mut(to_id) {
                        if from_last > *to_last { *to_last = from_last; }
                        for p in from_pubs { if !to_pubs.contains(&p) { to_pubs.push(p); } }
                    } else {
                        pub_map.insert(to_id.clone(), (from_first, from_last, from_pubs));
                    }
                }
                if let Some(from_articles) = article_data.remove(from_id) {
                    article_data.entry(to_id.clone()).or_default().extend(from_articles);
                }
            }
            Ok(())
        })();
        match merge_result {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(e) => { let _ = conn.execute_batch("ROLLBACK"); log::warn!("recluster merge pass failed: {}", e); }
        }
    }

    drop(cluster_data);

    let empty_vec = Vec::new();
    conn.execute_batch("BEGIN")?;
    let blindspot_result: Result<()> = (|| {
        for (cid, (first, last, pub_ids)) in &pub_map {
            let pub_refs: Vec<&str> = pub_ids.iter().map(|s| s.as_str()).collect();
            let is_blind = clustering::is_blindspot(&pub_refs);
            let articles = article_data.get(cid).unwrap_or(&empty_vec);
            let best_headline = clustering::pick_best_headline(articles);
            db::upsert_cluster(&conn, cid, &best_headline, first, last, is_blind)?;
        }
        Ok(())
    })();
    match blindspot_result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    log::info!("re-clustering complete: {} clusters", clusters_created);
    Ok(PipelineResult {
        articles_scraped: 0,
        articles_new: 0,
        clusters_created,
        failed_sources: Vec::new(),
    })
}

pub async fn run(db: &Pool<SqliteConnectionManager>) -> Result<PipelineResult> {
    let custom_pubs = {
        let conn = db.get()?;
        match db::prune_old_articles(&conn, 7) {
            Ok(n) => log::info!("pruned {} old articles on refresh", n),
            Err(e) => log::warn!("pruning failed: {}", e),
        }
        db::get_custom_publishers(&conn).unwrap_or_default()
    };

    let (mut raw_articles, failed_sources) = scraper::scrape_all(&custom_pubs).await;
    log::info!("scraped {} articles total", raw_articles.len());

    {
        let conn = db.get()?;
        let ids: Vec<&str> = raw_articles.iter().map(|a| a.id.as_str()).collect();
        if let Ok(existing) = db::get_existing_article_ids(&conn, &ids) {
            let skipped = existing.len();
            for a in &mut raw_articles {
                if existing.contains(&a.id) {
                    a.translated_headline = a.original_headline.clone();
                }
            }
            log::info!("skipping translation for {} already-stored articles", skipped);
        }
    }

    translate::translate_headlines(&mut raw_articles).await;

    for a in &mut raw_articles {
        if a.translated_headline.is_empty() && a.language == "en" {
            a.translated_headline = a.original_headline.clone();
        }
        let en_headline = if a.language == "en" {
            &a.original_headline
        } else if !a.translated_headline.is_empty() {
            &a.translated_headline
        } else {
            &a.original_headline
        };
        a.category = category::classify(&a.original_url, en_headline).to_string();
    }

    let mut result = process(db, raw_articles)?;
    result.failed_sources = failed_sources;
    Ok(result)
}
