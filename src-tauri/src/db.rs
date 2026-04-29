use anyhow::Result;
use rusqlite::{params, Connection};

use crate::models::{CustomPublisherDef, RawArticle};

// ── Database Setup & Migrations ──────────────────────────────────────────────

pub fn open(path: &std::path::Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS articles (
            id            TEXT PRIMARY KEY,
            publisher_id  TEXT NOT NULL,
            original_url  TEXT NOT NULL UNIQUE,
            headline      TEXT NOT NULL,
            translated_headline TEXT DEFAULT '',
            snippet       TEXT DEFAULT '',
            body_text     TEXT DEFAULT '',
            image_url     TEXT DEFAULT '',
            language      TEXT DEFAULT 'en',
            published_at  TEXT NOT NULL,
            cluster_id    TEXT,
            embedding     BLOB,
            category      TEXT DEFAULT 'general'
        );

        CREATE TABLE IF NOT EXISTS clusters (
            id               TEXT PRIMARY KEY,
            headline         TEXT NOT NULL,
            first_reported   TEXT NOT NULL,
            last_updated     TEXT NOT NULL,
            is_blindspot     INTEGER DEFAULT 0,
            ai_headline      TEXT DEFAULT '',
            ai_summary       TEXT DEFAULT ''
        );

        CREATE INDEX IF NOT EXISTS idx_articles_cluster   ON articles(cluster_id);
        CREATE INDEX IF NOT EXISTS idx_articles_published ON articles(published_at);
        CREATE INDEX IF NOT EXISTS idx_articles_publisher ON articles(publisher_id);
        CREATE INDEX IF NOT EXISTS idx_articles_language  ON articles(language);
        CREATE INDEX IF NOT EXISTS idx_articles_category  ON articles(category);

        CREATE TABLE IF NOT EXISTS custom_publishers (
            id             TEXT PRIMARY KEY,
            name           TEXT NOT NULL,
            rss_url        TEXT NOT NULL UNIQUE,
            scrape_method  TEXT NOT NULL DEFAULT 'rss',
            scrape_config  TEXT NOT NULL DEFAULT '',
            is_global      INTEGER NOT NULL DEFAULT 0
        );
        ",
    )?;

    // Read all column names in one PRAGMA call per table — replaces 6 separate prepare() probes.
    let col_names = |table: &str| -> rusqlite::Result<std::collections::HashSet<String>> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(names)
    };
    let articles_cols    = col_names("articles")?;
    let clusters_cols    = col_names("clusters")?;
    let custom_pubs_cols = col_names("custom_publishers")?;

    if !custom_pubs_cols.contains("scrape_method") {
        conn.execute_batch(
            "ALTER TABLE custom_publishers ADD COLUMN scrape_method TEXT NOT NULL DEFAULT 'rss';
             ALTER TABLE custom_publishers ADD COLUMN scrape_config TEXT NOT NULL DEFAULT '';",
        )?;
        log::info!("migrated: added scrape_method, scrape_config to custom_publishers");
    }
    if !articles_cols.contains("translated_headline") {
        conn.execute_batch("ALTER TABLE articles ADD COLUMN translated_headline TEXT DEFAULT ''")?;
        log::info!("migrated: added translated_headline column");
    }
    if !articles_cols.contains("body_text") {
        conn.execute_batch("ALTER TABLE articles ADD COLUMN body_text TEXT DEFAULT ''")?;
        log::info!("migrated: added body_text column");
    }
    if !articles_cols.contains("embedding") {
        conn.execute_batch("ALTER TABLE articles ADD COLUMN embedding BLOB")?;
        log::info!("migrated: added embedding column");
    }
    if !articles_cols.contains("category") {
        conn.execute_batch("ALTER TABLE articles ADD COLUMN category TEXT DEFAULT 'general'")?;
        log::info!("migrated: added category column");
    }
    if !clusters_cols.contains("ai_headline") {
        conn.execute_batch(
            "ALTER TABLE clusters ADD COLUMN ai_headline TEXT DEFAULT '';
             ALTER TABLE clusters ADD COLUMN ai_summary  TEXT DEFAULT '';",
        )?;
        log::info!("migrated: added ai_headline, ai_summary to clusters");
    }

    Ok(conn)
}

/// Per-connection PRAGMA setup used by the r2d2 pool's `with_init` callback.
/// Migrations (CREATE TABLE / ALTER TABLE) run once via `open()` at startup and
/// do not need to repeat for every pooled connection.
pub fn setup_pragmas(conn: &mut rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )
}

// ── Maintenance ──────────────────────────────────────────────────────────────

/// Delete articles older than `days` days and orphaned clusters with no articles.
pub fn prune_old_articles(conn: &Connection, days: u32) -> Result<usize> {
    let cutoff = format!("-{} days", days);
    let deleted = conn.execute(
        "DELETE FROM articles WHERE published_at < datetime('now', ?1)",
        rusqlite::params![cutoff],
    )?;
    // Remove clusters that now have no articles.
    conn.execute(
        "DELETE FROM clusters WHERE id NOT IN (
             SELECT DISTINCT cluster_id FROM articles WHERE cluster_id IS NOT NULL
         )",
        [],
    )?;
    Ok(deleted)
}

// ── Custom Publishers ────────────────────────────────────────────────────────

pub fn insert_custom_publisher(conn: &Connection, p: &CustomPublisherDef) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO custom_publishers (id, name, rss_url, scrape_method, scrape_config, is_global)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![p.id, p.name, p.rss_url, p.scrape_method, p.scrape_config, p.is_global as i32],
    )?;
    Ok(())
}

pub fn get_custom_publishers(conn: &Connection) -> Result<Vec<CustomPublisherDef>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, rss_url, scrape_method, scrape_config, is_global
         FROM custom_publishers ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CustomPublisherDef {
            id: row.get(0)?,
            name: row.get(1)?,
            rss_url: row.get(2)?,
            scrape_method: row.get::<_, String>(3).unwrap_or_else(|_| "rss".to_string()),
            scrape_config: row.get::<_, String>(4).unwrap_or_default(),
            is_global: row.get::<_, i32>(5)? != 0,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}

pub fn delete_custom_publisher(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM custom_publishers WHERE id = ?1", params![id])?;
    // Remove articles from this publisher so they don't show stale data
    conn.execute("DELETE FROM articles WHERE publisher_id = ?1", params![id])?;
    Ok(())
}

// ── CRUD Operations ──────────────────────────────────────────────────────────

/// Return the subset of `ids` that already exist in the articles table.
pub fn get_existing_article_ids(conn: &Connection, ids: &[&str]) -> Result<std::collections::HashSet<String>> {
    if ids.is_empty() {
        return Ok(std::collections::HashSet::new());
    }
    let placeholders: String = (1..=ids.len()).map(|i| format!("?{}", i)).collect::<Vec<_>>().join(",");
    let sql = format!("SELECT id FROM articles WHERE id IN ({})", placeholders);
    let mut stmt = conn.prepare(&sql)?;
    let existing = stmt
        .query_map(rusqlite::params_from_iter(ids.iter()), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(existing)
}

/// Insert an article if it doesn't already exist. Returns true if inserted.
/// Uses `prepare_cached` so the statement is compiled once per connection and reused
/// across calls — important when inserting many articles inside a single transaction.
pub fn insert_article(conn: &Connection, a: &RawArticle) -> Result<bool> {
    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO articles (id, publisher_id, original_url, headline, snippet, body_text, image_url, language, published_at, category)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    let changed = stmt.execute(params![
        a.id, a.publisher_id, a.original_url, a.original_headline,
        a.body_snippet, a.body_text, a.image_url, a.language, a.published_at, a.category,
    ])?;
    Ok(changed > 0)
}

/// Set the translated headline for an article.
pub fn set_translated_headline(conn: &Connection, article_id: &str, translated: &str) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "UPDATE articles SET translated_headline = ?1 WHERE id = ?2",
    )?;
    stmt.execute(params![translated, article_id])?;
    Ok(())
}

/// Assign an article to a cluster.
pub fn set_cluster(conn: &Connection, article_id: &str, cluster_id: &str) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "UPDATE articles SET cluster_id = ?1 WHERE id = ?2",
    )?;
    stmt.execute(params![cluster_id, article_id])?;
    Ok(())
}

/// Create or update a cluster.
pub fn upsert_cluster(
    conn: &Connection,
    id: &str,
    headline: &str,
    first_reported: &str,
    last_updated: &str,
    is_blindspot: bool,
) -> Result<()> {
    conn.execute(
        "INSERT INTO clusters (id, headline, first_reported, last_updated, is_blindspot)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
           headline     = excluded.headline,
           last_updated = MAX(excluded.last_updated, clusters.last_updated),
           is_blindspot = excluded.is_blindspot",
        params![id, headline, first_reported, last_updated, is_blindspot as i32],
    )?;
    Ok(())
}

// ── Queries ──────────────────────────────────────────────────────────────────

/// Lightweight row for frontend display — no embeddings, no body_text.
pub struct ArticleRowLight {
    pub id: String,
    pub publisher_id: String,
    pub original_url: String,
    pub headline: String,
    pub translated_headline: String,
    pub snippet: String,
    pub image_url: String,
    pub language: String,
    pub published_at: String,
    pub cluster_id: String,
    pub category: String,
}

/// Cluster row returned to the frontend — includes ai-generated fields.
pub struct ClusterRowLight {
    pub id: String,
    pub headline: String,
    pub first_reported: String,
    pub last_updated: String,
    pub is_blindspot: bool,
    pub ai_headline: String,
    pub ai_summary: String,
    pub articles: Vec<ArticleRowLight>,
}

/// Load clusters for frontend display using a single JOIN query (no N+1).
pub fn load_clusters_light(
    conn: &Connection,
    blindspots_only: bool,
) -> Result<Vec<ClusterRowLight>> {
    let where_clause = if blindspots_only { "WHERE c.is_blindspot = 1" } else { "" };
    let sql = format!(
        "SELECT c.id, c.headline, c.first_reported, c.last_updated, c.is_blindspot,
                c.ai_headline, c.ai_summary,
                a.id, a.publisher_id, a.original_url, a.headline, a.translated_headline,
                a.snippet, a.image_url, a.language, a.published_at, a.cluster_id, a.category
         FROM clusters c
         INNER JOIN articles a ON a.cluster_id = c.id
         {}
         ORDER BY c.last_updated DESC, a.published_at ASC",
        where_clause
    );

    let mut stmt = conn.prepare(&sql)?;

    let mut cluster_order: Vec<String> = Vec::new();
    let mut cluster_map: std::collections::HashMap<
        String,
        (String, String, String, bool, String, String, Vec<ArticleRowLight>),
        //  headline  first   last    blind  ai_hl   ai_sum
    > = std::collections::HashMap::new();

    let rows = stmt.query_map([], |row| {
        let cluster_id: String    = row.get(0)?;
        let cluster_headline      = row.get::<_, String>(1)?;
        let first_reported        = row.get::<_, String>(2)?;
        let last_updated          = row.get::<_, String>(3)?;
        let is_blindspot: bool    = row.get::<_, i32>(4)? != 0;
        let ai_headline           = row.get::<_, String>(5).unwrap_or_default();
        let ai_summary            = row.get::<_, String>(6).unwrap_or_default();
        let article = ArticleRowLight {
            id:                  row.get(7)?,
            publisher_id:        row.get(8)?,
            original_url:        row.get(9)?,
            headline:            row.get(10)?,
            translated_headline: row.get::<_, String>(11).unwrap_or_default(),
            snippet:             row.get::<_, String>(12).unwrap_or_default(),
            image_url:           row.get(13)?,
            language:            row.get(14)?,
            published_at:        row.get(15)?,
            cluster_id:          row.get(16)?,
            category:            row.get::<_, String>(17).unwrap_or_else(|_| "general".to_string()),
        };
        Ok((cluster_id, cluster_headline, first_reported, last_updated,
            is_blindspot, ai_headline, ai_summary, article))
    })?;

    for row in rows {
        let (cid, headline, first, last, blindspot, ai_hl, ai_sum, article) = row?;
        if !cluster_map.contains_key(&cid) {
            cluster_order.push(cid.clone());
            cluster_map.insert(cid.clone(), (headline, first, last, blindspot, ai_hl, ai_sum, Vec::new()));
        }
        if let Some(entry) = cluster_map.get_mut(&cid) {
            entry.6.push(article);
        }
    }

    let result = cluster_order
        .into_iter()
        .filter_map(|cid| {
            cluster_map.remove(&cid).map(|(headline, first, last, blindspot, ai_headline, ai_summary, articles)| {
                ClusterRowLight { id: cid, headline, first_reported: first, last_updated: last,
                    is_blindspot: blindspot, ai_headline, ai_summary, articles }
            })
        })
        .collect();

    Ok(result)
}

/// Load cluster metadata with publisher lists for blindspot analysis — single query using GROUP_CONCAT.
pub fn load_cluster_publishers(
    conn: &Connection,
) -> Result<Vec<(String, String, String, String, Vec<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.headline, c.first_reported,
                MAX(a.published_at) as last_updated,
                GROUP_CONCAT(DISTINCT a.publisher_id) as pub_ids
         FROM clusters c
         JOIN articles a ON a.cluster_id = c.id
         GROUP BY c.id",
    )?;

    let rows = stmt.query_map([], |row| {
        let cid: String = row.get(0)?;
        let headline: String = row.get(1)?;
        let first: String = row.get(2)?;
        let last: String = row.get(3)?;
        let pub_concat: String = row.get::<_, String>(4).unwrap_or_default();
        Ok((cid, headline, first, last, pub_concat))
    })?;

    let result = rows
        .filter_map(|r| r.ok())
        .map(|(cid, headline, first, last, pub_concat)| {
            let pub_ids: Vec<String> = if pub_concat.is_empty() {
                Vec::new()
            } else {
                pub_concat.split(',').map(|s| s.to_string()).collect()
            };
            (cid, headline, first, last, pub_ids)
        })
        .filter(|(_, _, _, _, pub_ids)| !pub_ids.is_empty())
        .collect();

    Ok(result)
}


/// Move a single article to a new cluster (user-initiated split).
/// Creates the new cluster row and deletes any old cluster that becomes empty.
pub fn split_article_to_cluster(
    conn: &Connection,
    article_id: &str,
    new_cluster_id: &str,
    headline: &str,
    published_at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE articles SET cluster_id = ?1 WHERE id = ?2",
        params![new_cluster_id, article_id],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO clusters (id, headline, first_reported, last_updated, is_blindspot)
         VALUES (?1, ?2, ?3, ?4, 0)",
        params![new_cluster_id, headline, published_at, published_at],
    )?;
    // Prune any cluster that now has no articles.
    conn.execute(
        "DELETE FROM clusters WHERE id NOT IN (
             SELECT DISTINCT cluster_id FROM articles WHERE cluster_id IS NOT NULL
         )",
        [],
    )?;
    Ok(())
}

/// Wipe all cluster assignments so the feed can be fully re-clustered.
pub fn wipe_clusters(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "UPDATE articles SET cluster_id = NULL;
         DELETE FROM clusters;",
    )?;
    Ok(())
}

/// Delete all articles and clusters, leaving the schema intact.
pub fn wipe_all_data(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM articles;
         DELETE FROM clusters;",
    )?;
    Ok(())
}

/// Load every article in chronological order for re-clustering.
/// Returns (id, original_headline, translated_headline, language, published_at, snippet, publisher_id).
pub fn load_articles_for_recluster(
    conn: &Connection,
) -> Result<Vec<(String, String, String, String, String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, headline, translated_headline, language, published_at, snippet, publisher_id
         FROM articles
         ORDER BY published_at ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2).unwrap_or_default(),
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5).unwrap_or_default(),
                row.get::<_, String>(6).unwrap_or_default(),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get (headline, translated_headline, language, publisher_id) for all articles in a cluster.
pub fn get_cluster_headlines(
    conn: &Connection,
    cluster_id: &str,
) -> Result<Vec<(String, String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT headline, translated_headline, language, publisher_id
         FROM articles WHERE cluster_id = ?1"
    )?;
    let rows = stmt
        .query_map(params![cluster_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1).unwrap_or_default(),
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Load headlines + snippets for ALL clusters in a single query, grouped by cluster_id.
/// Returns (headline, translated_headline, language, publisher_id, snippet).
/// Avoids N+1 queries during blindspot analysis and cluster-data building.
pub fn load_all_cluster_headlines(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, Vec<(String, String, String, String, String)>>> {
    let mut stmt = conn.prepare(
        "SELECT cluster_id, headline, translated_headline, language, publisher_id, snippet
         FROM articles WHERE cluster_id IS NOT NULL",
    )?;
    let mut result: std::collections::HashMap<String, Vec<(String, String, String, String, String)>> =
        std::collections::HashMap::new();
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2).unwrap_or_default(),
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5).unwrap_or_default(),
        ))
    })?;
    for row in rows {
        let (cluster_id, headline, translated, language, publisher_id, snippet) = row?;
        result
            .entry(cluster_id)
            .or_default()
            .push((headline, translated, language, publisher_id, snippet));
    }
    Ok(result)
}
