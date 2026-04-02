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

    // 2. Build headline map from existing clusters — all article headlines per cluster.
    let mut cluster_headlines: HashMap<String, Vec<String>> = HashMap::new();
    {
        let existing = db::load_cluster_publishers(&conn)?;
        for (cid, _, _, _, _) in &existing {
            if let Ok(articles) = db::get_cluster_headlines(&conn, cid) {
                let headlines: Vec<String> = articles.into_iter().map(|(h, t, lang, _)| {
                    if lang == "en" { h } else if !t.is_empty() { t } else { h }
                }).collect();
                cluster_headlines.insert(cid.clone(), headlines);
            }
        }
    }

    log::info!("{} existing clusters loaded for matching", cluster_headlines.len());

    // 3. Assign each new article to a cluster by headline keyword overlap.
    let mut clusters_created = 0;

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
            &cluster_headlines,
            &new_cluster_id,
        );

        db::set_cluster(&conn, &article.id, &assignment.cluster_id)?;

        if assignment.is_new {
            clusters_created += 1;
            log::info!("New cluster: {:?}", headline);

            cluster_headlines.insert(assignment.cluster_id.clone(), vec![headline.to_string()]);

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

            // Keep the in-memory vec up to date so subsequent articles in this batch
            // can also match against this new headline variant.
            if let Some(v) = cluster_headlines.get_mut(&assignment.cluster_id) {
                v.push(headline.to_string());
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

    drop(new_articles);
    drop(cluster_headlines);

    // 4. Blindspot analysis + best headline selection.
    let cluster_pubs = db::load_cluster_publishers(&conn)?;
    for (cid, _headline, first, last, pub_ids) in &cluster_pubs {
        let pub_refs: Vec<&str> = pub_ids.iter().map(|s| s.as_str()).collect();
        let is_blind = clustering::is_blindspot(&pub_refs);

        // Pick the most neutral/representative headline for the cluster
        let article_data = db::get_cluster_headlines(&conn, cid)?;
        let best_headline = clustering::pick_best_headline(&article_data);

        db::upsert_cluster(&conn, cid, &best_headline, first, last, is_blind)?;
    }

    Ok(PipelineResult {
        articles_scraped: scraped_count,
        articles_new: new_count,
        clusters_created,
        failed_sources: Vec::new(), // populated by run()
    })
}

pub async fn run(db: &Mutex<Connection>) -> Result<PipelineResult> {
    {
        let conn = db.lock().unwrap();
        match db::prune_old_articles(&conn, 48) {
            Ok((a, c)) => log::info!("pruned {} old articles, {} orphaned clusters", a, c),
            Err(e) => log::warn!("prune failed: {}", e),
        }
    }

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
