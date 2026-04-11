use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::category;
use crate::clustering;
use crate::db;
use crate::models::RawArticle;
use crate::scraper;
use crate::translate;

pub struct PipelineResult {
    pub articles_scraped: usize,
    pub articles_new: usize,
    pub clusters_created: usize,
    pub failed_sources: Vec<String>,
}

/// Store → cluster by keywords → blindspot.
fn process(
    db: &Mutex<Connection>,
    raw_articles: Vec<RawArticle>,
) -> Result<PipelineResult> {
    let scraped_count = raw_articles.len();

    // 1. Store new articles (with translated headlines) in a single transaction.
    let new_articles = {
        let conn = db.lock().unwrap();
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

    let conn = db.lock().unwrap();

    // 2. Build cluster data map (headlines + pre-tokenized forms + last_updated per cluster).
    // Use a single bulk query to avoid N+1 per-cluster fetches.
    let mut cluster_data: HashMap<String, clustering::ClusterData> = HashMap::new();
    {
        let existing = db::load_cluster_publishers(&conn)?;
        let all_headlines = db::load_all_cluster_headlines(&conn)?;
        for (cid, _, _, last_updated, _) in &existing {
            let articles = all_headlines.get(cid).map(|v| v.as_slice()).unwrap_or(&[]);
            let mut headlines: Vec<String> = Vec::with_capacity(articles.len());
            let mut tokenized_headlines: Vec<Vec<clustering::Token>> = Vec::with_capacity(articles.len());
            for (h, t, lang, _) in articles {
                let en = if lang == "en" { h.clone() } else if !t.is_empty() { t.clone() } else { h.clone() };
                tokenized_headlines.push(clustering::tokenize_weighted(&en));
                headlines.push(en);
            }
            cluster_data.insert(cid.clone(), clustering::ClusterData {
                headlines,
                tokenized_headlines,
                last_updated: last_updated.clone(),
            });
        }
    }

    log::info!("{} existing clusters loaded for matching", cluster_data.len());

    // Build IDF table from all existing cluster headlines.
    // Refreshed every IDF_REFRESH_EVERY new clusters so later articles in large batches
    // benefit from vocabulary that emerged earlier in the same batch.
    let mut idf = clustering::build_idf_table(&cluster_data);
    let mut clusters_since_idf_refresh: usize = 0;
    const IDF_REFRESH_EVERY: usize = 25;

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

            let new_cluster_id = uuid::Uuid::new_v4().to_string();

            let assignment = clustering::assign_cluster(
                headline,
                &article.published_at,
                &cluster_data,
                &idf,
                &new_cluster_id,
            );

            db::set_cluster(&conn, &article.id, &assignment.cluster_id)?;

            if assignment.is_new {
                clusters_created += 1;
                clusters_since_idf_refresh += 1;
                if clusters_since_idf_refresh >= IDF_REFRESH_EVERY {
                    idf = clustering::build_idf_table(&cluster_data);
                    clusters_since_idf_refresh = 0;
                }
                log::info!("New cluster: {:?}", headline);

                cluster_data.insert(assignment.cluster_id.clone(), clustering::ClusterData {
                    headlines: vec![headline.to_string()],
                    tokenized_headlines: vec![clustering::tokenize_weighted(headline)],
                    last_updated: article.published_at.clone(),
                });

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
                // can also match against this new headline variant.
                if let Some(data) = cluster_data.get_mut(&assignment.cluster_id) {
                    data.tokenized_headlines.push(clustering::tokenize_weighted(headline));
                    data.headlines.push(headline.to_string());
                    if article.published_at > data.last_updated {
                        data.last_updated = article.published_at.clone();
                    }
                }

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
    // Load all cluster headlines in one query to avoid N+1 per-cluster fetches.
    let cluster_pubs = db::load_cluster_publishers(&conn)?;
    let all_cluster_headlines = db::load_all_cluster_headlines(&conn)?;
    let empty_vec = Vec::new();
    conn.execute_batch("BEGIN")?;
    let blindspot_result: Result<()> = (|| {
        for (cid, _headline, first, last, pub_ids) in &cluster_pubs {
            let pub_refs: Vec<&str> = pub_ids.iter().map(|s| s.as_str()).collect();
            let is_blind = clustering::is_blindspot(&pub_refs);

            let article_data = all_cluster_headlines.get(cid).unwrap_or(&empty_vec);
            let best_headline = clustering::pick_best_headline(article_data);

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
pub fn recluster_all(db: &Mutex<Connection>) -> Result<PipelineResult> {
    let conn = db.lock().unwrap();

    db::wipe_clusters(&conn)?;
    log::info!("wiped all clusters for re-clustering");

    let articles = db::load_articles_for_recluster(&conn)?;
    log::info!("{} articles to re-cluster", articles.len());

    let mut cluster_data: HashMap<String, clustering::ClusterData> = HashMap::new();
    let mut idf = clustering::build_idf_table(&cluster_data);
    let mut clusters_since_idf_refresh: usize = 0;
    const IDF_REFRESH_EVERY: usize = 25;
    let mut clusters_created = 0;

    conn.execute_batch("BEGIN")?;
    let recluster_result: Result<()> = (|| {
        for (article_id, original_headline, translated_headline, language, published_at) in &articles {
            let headline = if language == "en" {
                original_headline
            } else if !translated_headline.is_empty() {
                translated_headline
            } else {
                original_headline
            };

            let new_cluster_id = uuid::Uuid::new_v4().to_string();
            let assignment = clustering::assign_cluster(
                headline,
                published_at,
                &cluster_data,
                &idf,
                &new_cluster_id,
            );

            db::set_cluster(&conn, article_id, &assignment.cluster_id)?;

            if assignment.is_new {
                clusters_created += 1;
                clusters_since_idf_refresh += 1;
                if clusters_since_idf_refresh >= IDF_REFRESH_EVERY {
                    idf = clustering::build_idf_table(&cluster_data);
                    clusters_since_idf_refresh = 0;
                }
                cluster_data.insert(assignment.cluster_id.clone(), clustering::ClusterData {
                    headlines: vec![headline.to_string()],
                    tokenized_headlines: vec![clustering::tokenize_weighted(headline)],
                    last_updated: published_at.clone(),
                });
                db::upsert_cluster(&conn, &assignment.cluster_id, headline, published_at, published_at, false)?;
            } else {
                if let Some(data) = cluster_data.get_mut(&assignment.cluster_id) {
                    data.tokenized_headlines.push(clustering::tokenize_weighted(headline));
                    data.headlines.push(headline.to_string());
                    if published_at > &data.last_updated {
                        data.last_updated = published_at.clone();
                    }
                }
                db::upsert_cluster(&conn, &assignment.cluster_id, headline, published_at, published_at, false)?;
            }
        }
        Ok(())
    })();
    match recluster_result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(e) => { let _ = conn.execute_batch("ROLLBACK"); return Err(e); }
    }

    // Blindspot analysis + best headline for all clusters (single bulk load, no N+1).
    let cluster_pubs = db::load_cluster_publishers(&conn)?;
    let all_cluster_headlines = db::load_all_cluster_headlines(&conn)?;
    let empty_vec = Vec::new();
    conn.execute_batch("BEGIN")?;
    let blindspot_result: Result<()> = (|| {
        for (cid, _headline, first, last, pub_ids) in &cluster_pubs {
            let pub_refs: Vec<&str> = pub_ids.iter().map(|s| s.as_str()).collect();
            let is_blind = clustering::is_blindspot(&pub_refs);
            let article_data = all_cluster_headlines.get(cid).unwrap_or(&empty_vec);
            let best_headline = clustering::pick_best_headline(article_data);
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

pub async fn run(db: &Mutex<Connection>) -> Result<PipelineResult> {
    let custom_pubs = {
        let conn = db.lock().unwrap();
        db::get_custom_publishers(&conn).unwrap_or_default()
    };

    let (mut raw_articles, failed_sources) = scraper::scrape_all(&custom_pubs).await;
    log::info!("scraped {} articles total", raw_articles.len());

    // Mark already-stored articles so translate_headlines skips them
    {
        let conn = db.lock().unwrap();
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
