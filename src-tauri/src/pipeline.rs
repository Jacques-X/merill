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

/// Build the text used for TF-IDF clustering: "[headline] . [snippet]".
/// The snippet adds disambiguating context without losing the dense topic signal
/// of the headline. Falls back to headline-only when the snippet is empty.
fn cluster_text(headline: &str, snippet: &str) -> String {
    let s = snippet.trim();
    if s.is_empty() {
        headline.to_string()
    } else {
        format!("{} . {}", headline, s)
    }
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

    // 1. Store new articles (with translated headlines) in a single transaction.
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

    // 2. Build cluster data map (headlines + pre-tokenized forms + last_updated per cluster).
    // Use a single bulk query to avoid N+1 per-cluster fetches.
    // Keep pub_map and article_data alive so step 4 (blindspot) can reuse them after
    // augmenting with newly assigned articles — avoiding a second full-table DB scan.
    // pub_map: cluster_id → (first_reported, last_updated, publisher_ids)
    let mut pub_map: HashMap<String, (String, String, Vec<String>)> =
        db::load_cluster_publishers(&conn)?
            .into_iter()
            .map(|(cid, _, first, last, pubs)| (cid, (first, last, pubs)))
            .collect();
    let mut article_data: HashMap<String, Vec<(String, String, String, String, String)>> =
        db::load_all_cluster_headlines(&conn)?;

    let mut cluster_data: HashMap<String, clustering::ClusterData> = HashMap::new();
    for (cid, (_, last_updated, _)) in &pub_map {
        let articles = article_data.get(cid).map(|v| v.as_slice()).unwrap_or(&[]);
        let mut headlines: Vec<String> = Vec::with_capacity(articles.len());
        let mut tokenized_headlines: Vec<Vec<clustering::Token>> = Vec::with_capacity(articles.len());
        let mut token_set: HashSet<String> = HashSet::new();
        for (h, t, lang, _, snippet) in articles {
            let en = if lang == "en" { h.clone() } else if !t.is_empty() { t.clone() } else { h.clone() };
            let text = cluster_text(&en, snippet);
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
        });
    }

    log::info!("{} existing clusters loaded for matching", cluster_data.len());

    // Build IDF table from all existing cluster headlines.
    // Refreshed when enough new vocabulary has accumulated since the last rebuild —
    // a count-of-new-tokens threshold is more meaningful than a fixed cluster count.
    let mut idf = clustering::build_idf_table(&cluster_data);
    let mut new_vocab_since_refresh: usize = 0;
    const VOCAB_REFRESH_THRESHOLD: usize = 50;

    // 3. Assign each new article to a cluster using TF-IDF cosine similarity.
    // All writes are batched in a single transaction to avoid per-statement fsync overhead.
    let mut clusters_created = 0;

    conn.execute_batch("BEGIN")?;
    let cluster_result: Result<()> = (|| {
        for article in &new_articles {
            // Always use English headline for clustering consistency
            let headline = if article.language == "en" {
                &article.original_headline
            } else if !article.translated_headline.is_empty() {
                &article.translated_headline // MT->EN translation
            } else {
                &article.original_headline // MT original as last resort
            };

            // Hybrid text: headline + snippet gives the model dense topic + context.
            let text = cluster_text(headline, &article.body_snippet);

            let new_cluster_id = uuid::Uuid::new_v4().to_string();

            let assignment = clustering::assign_cluster(
                &text,
                &article.published_at,
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
                // Count tokens genuinely new to the IDF vocabulary and refresh if enough
                // have accumulated. Done after building token_set so the rebuild includes
                // this cluster's vocabulary.
                new_vocab_since_refresh += token_set.iter().filter(|w| !idf.contains_key(w.as_str())).count();
                cluster_data.insert(assignment.cluster_id.clone(), clustering::ClusterData {
                    headlines: vec![text.clone()],
                    tokenized_headlines: vec![tokens],
                    token_set,
                    last_updated: article.published_at.clone(),
                });
                if new_vocab_since_refresh >= VOCAB_REFRESH_THRESHOLD {
                    idf = clustering::build_idf_table(&cluster_data);
                    new_vocab_since_refresh = 0;
                }

                // Track for blindspot pass so step 4 needs no second DB load.
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

                // Keep the in-memory data up to date so subsequent articles in this batch
                // can also match against this new variant.
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
                }

                // Track for blindspot pass so step 4 needs no second DB load.
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
    drop(cluster_data);

    // 4. Blindspot analysis + best headline selection.
    // pub_map and article_data were built in step 2 and augmented in step 3 — no second DB load needed.
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
        failed_sources: Vec::new(), // populated by run()
    })
}

/// Re-cluster every article already in the database without re-scraping.
/// Wipes all existing cluster assignments and rebuilds them from scratch using
/// the current TF-IDF algorithm in chronological order.
pub fn recluster_all(db: &Pool<SqliteConnectionManager>) -> Result<PipelineResult> {
    let conn = db.get()?;

    db::wipe_clusters(&conn)?;
    log::info!("wiped all clusters for re-clustering");

    let articles = db::load_articles_for_recluster(&conn)?;
    log::info!("{} articles to re-cluster", articles.len());

    let mut cluster_data: HashMap<String, clustering::ClusterData> = HashMap::new();
    // Track per-cluster pub and headline data in memory for the blindspot pass.
    // recluster_all starts from a wiped state so both maps begin empty.
    // pub_map: cluster_id → (first_reported, last_updated, publisher_ids)
    let mut pub_map: HashMap<String, (String, String, Vec<String>)> = HashMap::new();
    let mut article_data: HashMap<String, Vec<(String, String, String, String, String)>> = HashMap::new();
    let mut idf = clustering::build_idf_table(&cluster_data);
    let mut new_vocab_since_refresh: usize = 0;
    const VOCAB_REFRESH_THRESHOLD: usize = 50;
    let mut clusters_created = 0;

    conn.execute_batch("BEGIN")?;
    let recluster_result: Result<()> = (|| {
        for (article_id, original_headline, translated_headline, language, published_at, snippet, publisher_id) in &articles {
            let headline = if language == "en" {
                original_headline
            } else if !translated_headline.is_empty() {
                translated_headline
            } else {
                original_headline
            };

            let text = cluster_text(headline, snippet);

            let new_cluster_id = uuid::Uuid::new_v4().to_string();
            let assignment = clustering::assign_cluster(
                &text,
                published_at,
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
                cluster_data.insert(assignment.cluster_id.clone(), clustering::ClusterData {
                    headlines: vec![text.clone()],
                    tokenized_headlines: vec![tokens],
                    token_set,
                    last_updated: published_at.clone(),
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
                ));
                db::upsert_cluster(&conn, &assignment.cluster_id, headline, published_at, published_at, false)?;
            } else {
                if let Some(data) = cluster_data.get_mut(&assignment.cluster_id) {
                    let tokens = clustering::tokenize_weighted(&text);
                    for t in &tokens {
                        data.token_set.insert(t.word.clone());
                    }
                    data.tokenized_headlines.push(tokens);
                    data.headlines.push(text);
                    if published_at > &data.last_updated {
                        data.last_updated = published_at.clone();
                    }
                }
                if let Some((_, last, pubs)) = pub_map.get_mut(&assignment.cluster_id) {
                    if published_at > last {
                        *last = published_at.clone();
                    }
                    if !pubs.contains(publisher_id) {
                        pubs.push(publisher_id.clone());
                    }
                }
                article_data.entry(assignment.cluster_id.clone()).or_default().push((
                    original_headline.clone(),
                    translated_headline.clone(),
                    language.clone(),
                    publisher_id.clone(),
                    snippet.clone(),
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

    // Blindspot analysis + best headline for all clusters.
    // pub_map and article_data were built in the clustering loop — no second DB load needed.
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

    // Mark already-stored articles so translate_headlines skips them
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
        // Only fall back to original for EN articles — MT articles with no translation
        // keep translated_headline empty so the UI can show the Maltese original correctly.
        if a.translated_headline.is_empty() && a.language == "en" {
            a.translated_headline = a.original_headline.clone();
        }
        // Classify using URL + English headline
        let en_headline = if a.language == "en" {
            &a.original_headline
        } else if !a.translated_headline.is_empty() {
            &a.translated_headline // MT->EN translation
        } else {
            &a.original_headline // MT original as fallback for classification
        };
        a.category = category::classify(&a.original_url, en_headline).to_string();
    }

    let mut result = process(db, raw_articles)?;
    result.failed_sources = failed_sources;
    Ok(result)
}
