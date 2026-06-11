use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
#[cfg(not(target_os = "ios"))]
use std::path::{Path, PathBuf};
#[cfg(not(target_os = "ios"))]
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Result};
#[cfg(not(target_os = "ios"))]
use anyhow::Context;
use chrono::{DateTime, Utc};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection};
use serde::Serialize;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

use crate::{clustering, db};

pub const ALGORITHM_VERSION: i64 = 1;
pub const EMBEDDING_MODEL: &str = "intfloat/multilingual-e5-small-q4";
pub const EMBEDDING_VERSION: i64 = 2;
const EMBEDDING_DIMENSIONS: usize = 384;
const PRIMARY_WINDOW_HOURS: f32 = 72.0;
const FOLLOW_UP_WINDOW_HOURS: f32 = 24.0 * 7.0;
// Calibrated against tests/fixtures/semantic_clustering.json. At 0.80 the
// labelled corpus has 100% precision and 83% recall; lower values admit
// recurring-person and same-institution false merges.
const AUTO_MATCH_THRESHOLD: f32 = 0.80;
const AMBIGUOUS_THRESHOLD: f32 = 0.70;
const MEDOID_THRESHOLD: f32 = 0.74;

#[derive(Clone, Debug)]
struct SemanticArticle {
    id: String,
    publisher_id: String,
    original_url: String,
    headline: String,
    translated_headline: String,
    snippet: String,
    language: String,
    published_at: String,
    category: String,
    old_cluster_id: Option<String>,
    embedding: Option<Vec<f32>>,
    embedding_model: String,
    embedding_version: i64,
    facts: EventFacts,
}

#[derive(Clone, Debug, Default)]
struct EventFacts {
    tokens: HashSet<String>,
    entities: HashSet<String>,
    locations: HashSet<String>,
    numbers: HashSet<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ScoreComponents {
    semantic: f32,
    lexical: f32,
    entity: f32,
    numeric: f32,
    category: f32,
    time: f32,
    same_publisher_penalty: f32,
}

#[derive(Clone, Debug)]
struct PairDecision {
    left: usize,
    right: usize,
    score: f32,
    components: ScoreComponents,
    veto: Option<&'static str>,
    eligible: bool,
}

#[derive(Clone, Debug)]
struct ClusterPlan {
    cluster_id: String,
    members: Vec<usize>,
}

pub trait EmbeddingEngine {
    fn model_name(&self) -> &'static str {
        EMBEDDING_MODEL
    }

    fn model_version(&self) -> i64 {
        EMBEDDING_VERSION
    }

    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

#[cfg(not(target_os = "ios"))]
struct DesktopEmbeddingEngine {
    model: fastembed::TextEmbedding,
}

#[cfg(not(target_os = "ios"))]
impl DesktopEmbeddingEngine {
    fn load() -> Result<Self> {
        use fastembed::{
            InitOptionsUserDefined, Pooling, TextEmbedding, TokenizerFiles,
            UserDefinedEmbeddingModel,
        };

        let model_dir = locate_model_dir().ok_or_else(|| {
            anyhow!(
                "bundled embedding model not found; set MERILL_EMBEDDING_MODEL_DIR for development"
            )
        })?;
        let read = |name: &str| -> Result<Vec<u8>> {
            std::fs::read(model_dir.join(name))
                .with_context(|| format!("reading embedding resource {name}"))
        };
        let tokenizer_files = TokenizerFiles {
            tokenizer_file: read("tokenizer.json")?,
            config_file: read("config.json")?,
            special_tokens_map_file: read("special_tokens_map.json")?,
            tokenizer_config_file: read("tokenizer_config.json")?,
        };
        let user_model =
            UserDefinedEmbeddingModel::new(read("model.onnx")?, tokenizer_files)
                .with_pooling(Pooling::Mean);
        let model = TextEmbedding::try_new_from_user_defined(
            user_model,
            InitOptionsUserDefined::new()
                .with_max_length(256)
                .with_intra_threads(2),
        )
        .context("initializing multilingual E5")?;
        Ok(Self { model })
    }
}

#[cfg(not(target_os = "ios"))]
impl EmbeddingEngine for DesktopEmbeddingEngine {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let passages: Vec<String> = texts
            .iter()
            .map(|text| format!("passage: {text}"))
            .collect();
        self.model
            .embed(passages, Some(32))
            .context("running multilingual E5")
    }
}

#[cfg(target_os = "ios")]
struct IosEmbeddingEngine;

#[cfg(target_os = "ios")]
impl EmbeddingEngine for IosEmbeddingEngine {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        crate::ios_ai::embed(texts).ok_or_else(|| anyhow!("iOS embedding bridge unavailable"))
    }
}

#[cfg(not(target_os = "ios"))]
fn locate_model_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("MERILL_EMBEDDING_MODEL_DIR") {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("embedding"),
    );
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("resources").join("embedding"));
            candidates.push(parent.join("../Resources/resources/embedding"));
            candidates.push(parent.join("../Resources/embedding"));
        }
    }
    candidates
        .into_iter()
        .find(|path| path.join("model.onnx").is_file())
}

/// Recompute semantic clusters. `Ok(None)` means the on-device model was not
/// available and the caller should retain the legacy lexical result.
pub fn recluster(
    pool: &Pool<SqliteConnectionManager>,
    regenerate_embeddings: bool,
) -> Result<Option<usize>> {
    let conn = pool.get()?;
    #[cfg(target_os = "ios")]
    {
        let mut engine = IosEmbeddingEngine;
        return match recluster_with_engine(&conn, &mut engine, regenerate_embeddings) {
            Ok(count) => Ok(Some(count)),
            Err(error) => {
                log::warn!("semantic clustering unavailable, using lexical fallback: {error:#}");
                Ok(None)
            }
        };
    }
    #[cfg(not(target_os = "ios"))]
    {
        static ENGINE: OnceLock<Result<Mutex<DesktopEmbeddingEngine>, String>> = OnceLock::new();
        let engine = ENGINE.get_or_init(|| {
            DesktopEmbeddingEngine::load()
                .map(Mutex::new)
                .map_err(|error| format!("{error:#}"))
        });
        let engine = match engine {
            Ok(engine) => engine,
            Err(error) => {
                log::warn!("semantic clustering unavailable, using lexical fallback: {error}");
                return Ok(None);
            }
        };
        let mut engine = engine
            .lock()
            .map_err(|_| anyhow!("embedding engine lock poisoned"))?;
        let count = recluster_with_engine(&conn, &mut *engine, regenerate_embeddings)?;
        Ok(Some(count))
    }
}

fn recluster_with_engine(
    conn: &Connection,
    engine: &mut dyn EmbeddingEngine,
    regenerate_embeddings: bool,
) -> Result<usize> {
    let mut articles = load_articles(conn)?;
    if articles.is_empty() {
        return Ok(0);
    }

    // Snapshot existing cluster IDs so we can report *newly created* clusters,
    // not the total count (which is what plans.len() gives).
    let existing_cluster_ids: std::collections::HashSet<String> = {
        let mut stmt = conn.prepare("SELECT id FROM clusters")?;
        let rows: rusqlite::Result<Vec<String>> =
            stmt.query_map([], |row| row.get::<_, String>(0))?.collect();
        rows?.into_iter().collect()
    };

    // Load user-initiated split constraints so recluster never re-merges an
    // article back into the cluster the user manually removed it from.
    // The constraint is stored as (article_id, old_cluster_id); we convert
    // it into a set of (article_index, article_index) pairs to block in
    // build_components.
    let raw_splits = db::load_cannot_link_pairs(conn)?;
    let article_id_to_index: HashMap<&str, usize> = articles
        .iter()
        .enumerate()
        .map(|(i, a)| (a.id.as_str(), i))
        .collect();
    // Build a set of (left, right) article-index pairs that must not be merged.
    // An article that was split from cluster C must not be placed back with any
    // article whose *current* old_cluster_id matches C.
    let cannot_link: HashSet<(usize, usize)> = raw_splits
        .iter()
        .flat_map(|(split_article_id, old_cluster_id)| {
            let Some(&split_idx) = article_id_to_index.get(split_article_id.as_str()) else {
                return vec![];
            };
            articles
                .iter()
                .enumerate()
                .filter(|(other_idx, other)| {
                    *other_idx != split_idx
                        && other
                            .old_cluster_id
                            .as_deref()
                            .is_some_and(|cid| cid == old_cluster_id)
                })
                .map(|(other_idx, _)| {
                    let (l, r) = if split_idx < other_idx {
                        (split_idx, other_idx)
                    } else {
                        (other_idx, split_idx)
                    };
                    (l, r)
                })
                .collect()
        })
        .collect();

    populate_embeddings(&mut articles, engine, regenerate_embeddings)?;
    let decisions = score_pairs(&articles);
    let components = build_components(&articles, &decisions, &cannot_link);
    let plans = assign_stable_cluster_ids(conn, &articles, components)?;
    persist_plan(conn, &articles, &plans, &decisions)?;

    let new_clusters = plans
        .iter()
        .filter(|p| !existing_cluster_ids.contains(&p.cluster_id))
        .count();
    Ok(new_clusters)
}

fn load_articles(conn: &Connection) -> Result<Vec<SemanticArticle>> {
    let mut stmt = conn.prepare(
        "SELECT id, publisher_id, original_url, headline, translated_headline, snippet,
                language, published_at, category, cluster_id, embedding,
                embedding_model, embedding_version
         FROM articles
         ORDER BY published_at ASC, id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let headline: String = row.get(3)?;
        let translated_headline: String = row.get::<_, String>(4).unwrap_or_default();
        Ok(SemanticArticle {
            id: row.get(0)?,
            publisher_id: row.get(1)?,
            original_url: row.get(2)?,
            facts: extract_facts(&headline, &translated_headline),
            headline,
            translated_headline,
            snippet: row.get::<_, String>(5).unwrap_or_default(),
            language: row.get::<_, String>(6).unwrap_or_else(|_| "en".to_string()),
            published_at: row.get(7)?,
            category: row.get::<_, String>(8).unwrap_or_else(|_| "general".to_string()),
            old_cluster_id: row.get(9)?,
            embedding: row
                .get::<_, Option<Vec<u8>>>(10)?
                .and_then(|bytes| decode_embedding(&bytes)),
            embedding_model: row.get::<_, String>(11).unwrap_or_default(),
            embedding_version: row.get::<_, i64>(12).unwrap_or_default(),
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn populate_embeddings(
    articles: &mut [SemanticArticle],
    engine: &mut dyn EmbeddingEngine,
    regenerate: bool,
) -> Result<()> {
    let missing: Vec<usize> = articles
        .iter()
        .enumerate()
        .filter(|(_, article)| {
            regenerate
                || article.embedding.as_ref().is_none_or(|v| v.len() != EMBEDDING_DIMENSIONS)
                || article.embedding_model != engine.model_name()
                || article.embedding_version != engine.model_version()
        })
        .map(|(index, _)| index)
        .collect();
    if missing.is_empty() {
        return Ok(());
    }

    let texts: Vec<String> = missing
        .iter()
        .map(|&index| canonical_text(&articles[index]))
        .collect();
    let vectors = engine.embed(&texts)?;
    if vectors.len() != missing.len()
        || vectors.iter().any(|vector| vector.len() != EMBEDDING_DIMENSIONS)
    {
        return Err(anyhow!("embedding engine returned an unexpected shape"));
    }

    for (&index, mut vector) in missing.iter().zip(vectors) {
        normalize_vector(&mut vector);
        articles[index].embedding = Some(vector);
        articles[index].embedding_model = engine.model_name().to_string();
        articles[index].embedding_version = engine.model_version();
    }
    Ok(())
}

fn canonical_text(article: &SemanticArticle) -> String {
    let mut parts = vec![clean_text(&article.headline)];
    if !article.translated_headline.is_empty()
        && fold_text(&article.translated_headline) != fold_text(&article.headline)
    {
        parts.push(clean_text(&article.translated_headline));
    }
    let slug = url_slug(&article.original_url);
    if !slug.is_empty() {
        parts.push(slug);
    }
    let snippet = clean_text(&article.snippet);
    if !snippet.is_empty() {
        parts.push(snippet.chars().take(450).collect());
    }
    parts.join(" . ")
}

fn clean_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn url_slug(url: &str) -> String {
    url.split(['?', '#'])
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .split(['-', '_'])
        .filter(|part| part.len() > 2)
        .collect::<Vec<_>>()
        .join(" ")
}

fn score_pairs(articles: &[SemanticArticle]) -> Vec<PairDecision> {
    let mut decisions = Vec::new();
    for left in 0..articles.len() {
        for right in (left + 1)..articles.len() {
            let hours = hours_between(
                &articles[left].published_at,
                &articles[right].published_at,
            );
            if hours > FOLLOW_UP_WINDOW_HOURS {
                continue;
            }
            decisions.push(score_pair(left, right, articles, hours));
        }
    }
    decisions.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| articles[a.left].id.cmp(&articles[b.left].id))
            .then_with(|| articles[a.right].id.cmp(&articles[b.right].id))
    });
    decisions
}

fn score_pair(
    left: usize,
    right: usize,
    articles: &[SemanticArticle],
    hours: f32,
) -> PairDecision {
    let a = &articles[left];
    let b = &articles[right];
    let semantic = cosine(
        a.embedding.as_deref().unwrap_or_default(),
        b.embedding.as_deref().unwrap_or_default(),
    );
    let lexical = dice(&a.facts.tokens, &b.facts.tokens);
    let entity = overlap_ratio(&a.facts.entities, &b.facts.entities);
    let numeric = agreement(&a.facts.numbers, &b.facts.numbers);
    let category = if a.category == b.category && a.category != "general" {
        1.0
    } else if a.category == "general" || b.category == "general" {
        0.55
    } else {
        // Partial credit instead of 0 — cross-category stories do sometimes
        // genuinely belong together (crime/politics, business/local, etc.).
        0.30
    };
    let time = (-hours / 48.0).exp();
    let same_publisher_penalty = if a.publisher_id == b.publisher_id {
        0.04
    } else {
        0.0
    };
    let veto = contradiction(&a.facts, &b.facts, semantic);

    // Cross-language pairs: lexical score is structurally near-zero because the
    // token sets are from different languages. Redistribute the lexical weight
    // onto semantic + entity so these pairs are judged on meaning, not surface form.
    let cross_language = a.language != b.language;
    let score = if cross_language {
        0.75 * semantic + 0.15 * entity + 0.05 * category + 0.05 * time - same_publisher_penalty
    } else {
        0.65 * semantic
            + 0.15 * lexical
            + 0.10 * entity
            + 0.05 * category
            + 0.05 * time
            - same_publisher_penalty
    };

    // Primary follow-up window: articles within 72h are always eligible for
    // scoring. Beyond that, require strong semantic + supporting evidence.
    // Slow-burn stories (crime sagas, political inquiries) get a slightly relaxed
    // gate: they can satisfy the window with high semantic alone if a shared
    // entity anchor exists.
    let is_slow_burn = a.category == "crime"
        || a.category == "politics"
        || b.category == "crime"
        || b.category == "politics";
    let follow_up_evidence = hours <= PRIMARY_WINDOW_HOURS
        || (semantic >= 0.84 && (entity > 0.0 || lexical >= 0.25))
        || (is_slow_burn && semantic >= 0.87 && entity > 0.0);

    // Two-tier acceptance:
    //   • Primary gate: score ≥ AUTO_MATCH_THRESHOLD (0.80) — standard precision gate.
    //   • Ambiguous band (0.70–0.80): accept if a shared rare entity anchor exists
    //     and there is no veto. This recovers near-misses without lowering the
    //     global precision gate.
    let in_ambiguous_band =
        score >= AMBIGUOUS_THRESHOLD && score < AUTO_MATCH_THRESHOLD;
    // "Rare anchor" proxy: meaningful entity overlap (not just a common name).
    // overlap_ratio ≥ 0.25 means at least 25 % of the smaller entity set is shared.
    let has_entity_anchor = entity >= 0.25;
    let ambiguous_accept = in_ambiguous_band && has_entity_anchor && veto.is_none() && follow_up_evidence;

    PairDecision {
        left,
        right,
        score,
        components: ScoreComponents {
            semantic,
            lexical,
            entity,
            numeric,
            category,
            time,
            same_publisher_penalty,
        },
        veto,
        eligible: (veto.is_none() && follow_up_evidence && score >= AUTO_MATCH_THRESHOLD)
            || ambiguous_accept,
    }
}

fn contradiction(a: &EventFacts, b: &EventFacts, semantic: f32) -> Option<&'static str> {
    // Conflicting numbers only veto below 0.85 — above that, differing figures
    // almost always indicate an update to the same developing story
    // (e.g. "2 dead" → "death toll rises to 3").
    if !a.numbers.is_empty()
        && !b.numbers.is_empty()
        && a.numbers.is_disjoint(&b.numbers)
        && semantic < 0.85
    {
        return Some("conflicting-numbers");
    }
    // Conflicting locations veto below 0.85 — national angle vs. town-specific
    // coverage of the same event should be allowed through at high semantic similarity.
    if !a.locations.is_empty()
        && !b.locations.is_empty()
        && a.locations.is_disjoint(&b.locations)
        && semantic < 0.85
    {
        return Some("conflicting-locations");
    }
    None
}

/// Path-halving union-find for O(α(n)) cluster lookups.
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            // Path halving: point every other node directly to grandparent.
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, x: usize, y: usize) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx != ry {
            self.parent[rx] = ry;
        }
    }
}

fn build_components(
    articles: &[SemanticArticle],
    decisions: &[PairDecision],
    cannot_link: &HashSet<(usize, usize)>,
) -> Vec<Vec<usize>> {
    let n = articles.len();
    let mut uf = UnionFind::new(n);

    let pair_lookup: HashMap<(usize, usize), &PairDecision> = decisions
        .iter()
        .map(|decision| ((decision.left, decision.right), decision))
        .collect();

    // Maintain root → members map so coherence checks don't need to scan all
    // articles; updated on every successful union.
    let mut members_map: HashMap<usize, Vec<usize>> =
        (0..n).map(|i| (i, vec![i])).collect();

    for decision in decisions.iter().filter(|d| d.eligible) {
        // Respect user-split cannot-link constraints: never merge two articles
        // that the user deliberately separated.
        let key = (decision.left.min(decision.right), decision.left.max(decision.right));
        if cannot_link.contains(&key) {
            continue;
        }

        let left_root = uf.find(decision.left);
        let right_root = uf.find(decision.right);
        if left_root == right_root {
            continue;
        }

        // Build proposed merged member list for the coherence check.
        let left_members = members_map.get(&left_root).cloned().unwrap_or_default();
        let right_members = members_map.get(&right_root).cloned().unwrap_or_default();

        // Also check that merging the two components wouldn't introduce any
        // cannot-link pair transitively.
        let has_forbidden = left_members.iter().any(|&l| {
            right_members.iter().any(|&r| {
                let pair = (l.min(r), l.max(r));
                cannot_link.contains(&pair)
            })
        });
        if has_forbidden {
            continue;
        }

        let mut merged = left_members;
        merged.extend_from_slice(&right_members);
        merged.sort_unstable();

        if cluster_is_coherent(&merged, &pair_lookup, articles) {
            uf.union(left_root, right_root);
            let new_root = uf.find(left_root);
            members_map.remove(&left_root);
            members_map.remove(&right_root);
            members_map.insert(new_root, merged);
        }
    }

    let mut clusters: Vec<Vec<usize>> = members_map.into_values().collect();
    clusters.sort_by(|a, b| component_key(a, articles).cmp(&component_key(b, articles)));
    clusters
}

fn cluster_is_coherent(
    members: &[usize],
    pairs: &HashMap<(usize, usize), &PairDecision>,
    articles: &[SemanticArticle],
) -> bool {
    // Do NOT require every internal pair to exist in the lookup table.
    // Articles that are >FOLLOW_UP_WINDOW_HOURS apart were never scored, so their
    // pair is simply absent — that is not a contradiction.  Long-running stories
    // (court sagas, ongoing inquiries) would otherwise never consolidate.
    //
    // Similarly, one vetoed internal pair (e.g. two updates with different death
    // tolls) should not block a cluster union whose medoid connections are clean.
    // Only the medoid connections are enforced below.

    let Some(medoid) = members.iter().copied().max_by(|&a, &b| {
        average_score(a, members, pairs)
            .partial_cmp(&average_score(b, members, pairs))
            .unwrap_or(Ordering::Equal)
            .then_with(|| articles[b].id.cmp(&articles[a].id))
    }) else {
        return false;
    };

    // Every non-medoid member must either:
    //   • have a scored, non-vetoed connection to the medoid above MEDOID_THRESHOLD, OR
    //   • have no scored connection at all (articles too far apart — treat as neutral).
    members.iter().all(|&member| {
        if member == medoid {
            return true;
        }
        match pairs.get(&ordered_pair(member, medoid)) {
            Some(pair) => pair.score >= MEDOID_THRESHOLD && pair.veto.is_none(),
            // Missing pair = articles outside the scoring window; allow them through.
            None => true,
        }
    })
}

fn average_score(
    member: usize,
    members: &[usize],
    pairs: &HashMap<(usize, usize), &PairDecision>,
) -> f32 {
    let scores: Vec<f32> = members
        .iter()
        .copied()
        .filter(|&other| other != member)
        .filter_map(|other| pairs.get(&ordered_pair(member, other)).map(|pair| pair.score))
        .collect();
    if scores.is_empty() {
        1.0
    } else {
        scores.iter().sum::<f32>() / scores.len() as f32
    }
}

fn assign_stable_cluster_ids(
    conn: &Connection,
    articles: &[SemanticArticle],
    components: Vec<Vec<usize>>,
) -> Result<Vec<ClusterPlan>> {
    let old_dates: HashMap<String, String> = {
        let mut stmt = conn.prepare("SELECT id, first_reported FROM clusters")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut used = HashSet::new();
    let plans = components
        .into_iter()
        .map(|members| {
            let mut candidates: Vec<(String, String, String)> = members
                .iter()
                .filter_map(|&index| {
                    articles[index].old_cluster_id.as_ref().map(|id| {
                        (
                            id.clone(),
                            articles[index].published_at.clone(),
                            articles[index].id.clone(),
                        )
                    })
                })
                .filter(|(id, _, _)| !used.contains(id))
                .collect();
            candidates.sort_by(|a, b| {
                a.1.cmp(&b.1)
                    .then_with(|| a.2.cmp(&b.2))
                    .then_with(|| old_dates.get(&a.0).cmp(&old_dates.get(&b.0)))
                    .then_with(|| a.0.cmp(&b.0))
            });
            candidates.dedup_by(|a, b| a.0 == b.0);
            let cluster_id = candidates
                .into_iter()
                .map(|(id, _, _)| id)
                .next()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            used.insert(cluster_id.clone());
            ClusterPlan {
                cluster_id,
                members,
            }
        })
        .collect();
    Ok(plans)
}

fn persist_plan(
    conn: &Connection,
    articles: &[SemanticArticle],
    plans: &[ClusterPlan],
    decisions: &[PairDecision],
) -> Result<()> {
    let final_cluster_by_article: HashMap<usize, &str> = plans
        .iter()
        .flat_map(|plan| {
            plan.members
                .iter()
                .map(move |&index| (index, plan.cluster_id.as_str()))
        })
        .collect();
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result: Result<()> = (|| {
        let mut update_embedding = conn.prepare_cached(
            "UPDATE articles
             SET embedding = ?1, embedding_model = ?2, embedding_version = ?3
             WHERE id = ?4",
        )?;
        for article in articles {
            let embedding = article
                .embedding
                .as_deref()
                .ok_or_else(|| anyhow!("article {} has no embedding", article.id))?;
            update_embedding.execute(params![
                encode_embedding(embedding),
                article.embedding_model,
                article.embedding_version,
                article.id
            ])?;
        }

        let mut set_cluster =
            conn.prepare_cached("UPDATE articles SET cluster_id = ?1 WHERE id = ?2")?;
        for plan in plans {
            for &index in &plan.members {
                set_cluster.execute(params![plan.cluster_id, articles[index].id])?;
            }
        }

        for plan in plans {
            let first = plan
                .members
                .iter()
                .map(|&index| articles[index].published_at.as_str())
                .min()
                .unwrap_or_default();
            let last = plan
                .members
                .iter()
                .map(|&index| articles[index].published_at.as_str())
                .max()
                .unwrap_or_default();
            let publishers: Vec<&str> = plan
                .members
                .iter()
                .map(|&index| articles[index].publisher_id.as_str())
                .collect();
            let display_articles: Vec<(String, String, String, String, String, String)> = plan
                .members
                .iter()
                .map(|&index| {
                    let article = &articles[index];
                    (
                        article.headline.clone(),
                        article.translated_headline.clone(),
                        article.language.clone(),
                        article.publisher_id.clone(),
                        article.snippet.clone(),
                        article.category.clone(),
                    )
                })
                .collect();
            db::upsert_cluster(
                conn,
                &plan.cluster_id,
                &clustering::pick_best_headline(&display_articles),
                first,
                last,
                clustering::is_blindspot(&publishers),
            )?;
        }
        conn.execute(
            "DELETE FROM clusters
             WHERE id NOT IN (
                 SELECT DISTINCT cluster_id FROM articles WHERE cluster_id IS NOT NULL
             )",
            [],
        )?;

        conn.execute(
            "DELETE FROM clustering_diagnostics WHERE algorithm_version = ?1",
            params![ALGORITHM_VERSION],
        )?;
        let mut insert_diagnostic = conn.prepare_cached(
            "INSERT INTO clustering_diagnostics
             (article_id_a, article_id_b, algorithm_version, score, decision, reason, components_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        // Only persist near-threshold pairs (score ≥ 0.5 or eligible); pairs well
        // below threshold add no diagnostic value and bloat the table significantly.
        for decision in decisions.iter().filter(|d| d.eligible || d.score >= 0.50) {
            let accepted = decision.eligible
                && final_cluster_by_article.get(&decision.left)
                    == final_cluster_by_article.get(&decision.right);
            let state = if accepted {
                "matched"
            } else if decision.veto.is_none() && decision.score >= AMBIGUOUS_THRESHOLD {
                "ambiguous"
            } else {
                "rejected"
            };
            let reason = decision.veto.unwrap_or(if decision.score < AMBIGUOUS_THRESHOLD {
                "below-threshold"
            } else if decision.score < AUTO_MATCH_THRESHOLD {
                "precision-gate"
            } else if decision.eligible {
                "cluster-coherence"
            } else {
                "follow-up-evidence"
            });
            insert_diagnostic.execute(params![
                articles[decision.left].id,
                articles[decision.right].id,
                ALGORITHM_VERSION,
                decision.score,
                state,
                reason,
                serde_json::to_string(&decision.components)?
            ])?;
            if state == "ambiguous" {
                log::info!(
                    "ambiguous cluster candidate score={:.3} left={:?} right={:?}",
                    decision.score,
                    articles[decision.left].headline,
                    articles[decision.right].headline
                );
            }
        }
        Ok(())
    })();
    finish_transaction(conn, result)
}

fn finish_transaction(conn: &Connection, result: Result<()>) -> Result<()> {
    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn extract_facts(headline: &str, translated: &str) -> EventFacts {
    let combined = format!("{headline} {translated}");
    let folded = fold_text(&combined);
    let tokens: HashSet<String> = folded
        .split_whitespace()
        .filter_map(normalize_token)
        .collect();
    let numbers = folded
        .split_whitespace()
        .filter_map(|token| {
            let digits: String = token.chars().filter(char::is_ascii_digit).collect();
            (!digits.is_empty()).then_some(digits)
        })
        .collect();
    let locations = known_locations()
        .iter()
        .filter(|location| folded.contains(**location))
        .map(|location| (*location).to_string())
        .collect();
    EventFacts {
        entities: extract_entities(&combined),
        tokens,
        locations,
        numbers,
    }
}

fn fold_text(text: &str) -> String {
    let decomposed: String = text
        .nfkd()
        .filter(|ch| !is_combining_mark(*ch))
        .flat_map(char::to_lowercase)
        .map(|ch| if ch.is_alphanumeric() { ch } else { ' ' })
        .collect();
    let aliases = [
        ("north american", "north_america"),
        ("new york city", "new_york"),
        ("new york", "new_york"),
        ("tribeca film festival", "tribeca"),
        ("film festival", "festival"),
        ("prime minister", "prime_minister"),
    ];
    aliases
        .iter()
        .fold(decomposed, |value, (from, to)| value.replace(from, to))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_token(token: &str) -> Option<String> {
    const STOP: &[&str] = &[
        "about", "after", "at", "before", "from", "has", "have", "in", "its", "malta",
        "maltese", "of", "on", "the", "to", "with", "and", "for", "a", "an", "is",
        "are", "this", "that", "new", "says", "said", "qed", "fil", "tal", "mill",
    ];
    if token.len() < 3 || STOP.contains(&token) {
        return None;
    }
    let stemmed = token
        .strip_suffix("ing")
        .filter(|stem| stem.len() > 4)
        .or_else(|| token.strip_suffix("ed").filter(|stem| stem.len() > 4))
        .or_else(|| token.strip_suffix('s').filter(|stem| stem.len() > 4))
        .unwrap_or(token);
    Some(stemmed.to_string())
}

fn extract_entities(text: &str) -> HashSet<String> {
    const GENERIC: &[&str] = &[
        "Maltese", "Malta", "Film", "Festival", "North", "American", "New", "The",
    ];
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|ch: char| !ch.is_alphanumeric())
                .to_string()
        })
        .collect();
    let mut entities = HashSet::new();
    for (index, word) in words.iter().enumerate() {
        if word.len() < 3
            || GENERIC.contains(&word.as_str())
            || !word.chars().next().is_some_and(char::is_uppercase)
        {
            continue;
        }
        entities.insert(fold_text(word));
        if let Some(next) = words.get(index + 1) {
            if next.chars().next().is_some_and(char::is_uppercase)
                && !GENERIC.contains(&next.as_str())
            {
                entities.insert(format!("{}_{}", fold_text(word), fold_text(next)));
            }
        }
    }
    entities
}

fn known_locations() -> &'static [&'static str] {
    &[
        "birzebbuga",
        "bormla",
        "bugibba",
        "gozo",
        "gżira",
        "gzira",
        "hamrun",
        "marsa",
        "marsaskala",
        "mellieha",
        "mosta",
        "new_york",
        "paola",
        "qormi",
        "sliema",
        "st julians",
        "valletta",
        "zejtun",
    ]
}

fn dice(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    2.0 * a.intersection(b).count() as f32 / (a.len() + b.len()) as f32
}

fn overlap_ratio(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    a.intersection(b).count() as f32 / a.len().min(b.len()) as f32
}

fn agreement(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        0.5
    } else if a.is_disjoint(b) {
        0.0
    } else {
        1.0
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b).map(|(left, right)| left * right).sum()
}

fn normalize_vector(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in vector {
            *value /= norm;
        }
    }
}

fn encode_embedding(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn decode_embedding(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() != EMBEDDING_DIMENSIONS * std::mem::size_of::<f32>() {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().expect("four-byte chunk")))
            .collect(),
    )
}

fn hours_between(left: &str, right: &str) -> f32 {
    let parse = |value: &str| {
        DateTime::parse_from_rfc3339(value)
            .map(|date| date.with_timezone(&Utc))
            .or_else(|_| {
                chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
                    .map(|date| date.and_utc())
            })
    };
    match (parse(left), parse(right)) {
        (Ok(left), Ok(right)) => (left - right).num_minutes().unsigned_abs() as f32 / 60.0,
        _ => FOLLOW_UP_WINDOW_HOURS + 1.0,
    }
}

fn ordered_pair(left: usize, right: usize) -> (usize, usize) {
    if left < right {
        (left, right)
    } else {
        (right, left)
    }
}

fn component_key(members: &[usize], articles: &[SemanticArticle]) -> (String, String) {
    members
        .iter()
        .map(|&index| {
            (
                articles[index].published_at.clone(),
                articles[index].id.clone(),
            )
        })
        .min()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RawArticle;
    use serde::Deserialize;
    use std::path::Path;

    struct KeywordEngine;

    impl EmbeddingEngine for KeywordEngine {
        fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|text| {
                    let folded = fold_text(text);
                    let mut vector = vec![0.0; EMBEDDING_DIMENSIONS];
                    let concepts = [
                        ("zejtune", 0),
                        ("tribeca", 1),
                        ("premier", 2),
                        ("crane", 3),
                        ("birzebbuga", 4),
                        ("cocaine", 5),
                    ];
                    for (concept, index) in concepts {
                        if folded.contains(concept) {
                            vector[index] = 1.0;
                        }
                    }
                    normalize_vector(&mut vector);
                    vector
                })
                .collect())
        }
    }

    fn article(id: &str, headline: &str, time: &str) -> SemanticArticle {
        SemanticArticle {
            id: id.to_string(),
            publisher_id: format!("publisher-{id}"),
            original_url: format!("https://example.com/{id}"),
            headline: headline.to_string(),
            translated_headline: String::new(),
            snippet: String::new(),
            language: "en".to_string(),
            published_at: time.to_string(),
            category: "entertainment".to_string(),
            old_cluster_id: None,
            embedding: None,
            embedding_model: String::new(),
            embedding_version: 0,
            facts: extract_facts(headline, ""),
        }
    }

    fn raw_article(id: &str, headline: &str) -> RawArticle {
        RawArticle {
            id: id.to_string(),
            publisher_id: format!("publisher-{id}"),
            original_url: format!("https://example.com/{id}"),
            original_headline: headline.to_string(),
            translated_headline: headline.to_string(),
            body_snippet: String::new(),
            body_text: String::new(),
            image_url: String::new(),
            language: "en".to_string(),
            published_at: "2026-06-08T12:00:00+00:00".to_string(),
            category: "entertainment".to_string(),
        }
    }

    #[test]
    fn zejtune_tribeca_regression_clusters() {
        let mut articles = vec![
            article(
                "a",
                "Maltese film Żejtune has its North American premiere",
                "2026-06-08T16:00:00+00:00",
            ),
            article(
                "b",
                "Maltese Film Żejtune Premieres At Tribeca Film Festival In New York",
                "2026-06-08T16:14:00+00:00",
            ),
        ];
        let mut engine = KeywordEngine;
        let texts: Vec<_> = articles.iter().map(canonical_text).collect();
        let vectors = engine.embed(&texts).unwrap();
        for (article, vector) in articles.iter_mut().zip(vectors) {
            article.embedding = Some(vector);
        }
        let decisions = score_pairs(&articles);
        assert!(decisions[0].eligible, "{decisions:#?}");
        assert_eq!(build_components(&articles, &decisions, &HashSet::new()).len(), 1);
    }

    #[test]
    fn clustering_is_independent_of_input_order() {
        let source = vec![
            article(
                "a",
                "Tower crane drops concrete slab onto car in St Julians",
                "2026-06-08T10:00:00+00:00",
            ),
            article(
                "b",
                "Concrete slab falls from crane in St Julians",
                "2026-06-08T11:00:00+00:00",
            ),
            article(
                "c",
                "Man jailed after cocaine trafficking investigation",
                "2026-06-08T12:00:00+00:00",
            ),
        ];
        let cluster_ids = |mut articles: Vec<SemanticArticle>| {
            let mut engine = KeywordEngine;
            let vectors = engine
                .embed(&articles.iter().map(canonical_text).collect::<Vec<_>>())
                .unwrap();
            for (article, vector) in articles.iter_mut().zip(vectors) {
                article.embedding = Some(vector);
            }
            let decisions = score_pairs(&articles);
            let mut groups: Vec<Vec<String>> = build_components(&articles, &decisions, &HashSet::new())
                .into_iter()
                .map(|members| {
                    let mut ids: Vec<_> = members
                        .into_iter()
                        .map(|index| articles[index].id.clone())
                        .collect();
                    ids.sort();
                    ids
                })
                .collect();
            groups.sort();
            groups
        };
        let forward = cluster_ids(source.clone());
        let mut reversed = source;
        reversed.reverse();
        assert_eq!(forward, cluster_ids(reversed));
    }

    #[test]
    fn embedding_round_trip_rejects_wrong_versions() {
        let vector = vec![0.25; EMBEDDING_DIMENSIONS];
        assert_eq!(decode_embedding(&encode_embedding(&vector)), Some(vector));
        assert!(decode_embedding(&[0; 8]).is_none());
    }

    #[cfg(not(target_os = "ios"))]
    #[test]
    fn bundled_e5_model_matches_zejtune_regression() {
        let mut articles = vec![
            article(
                "a",
                "Maltese film Żejtune has its North American premiere",
                "2026-06-08T16:00:00+00:00",
            ),
            article(
                "b",
                "Maltese Film Żejtune Premieres At Tribeca Film Festival In New York",
                "2026-06-08T16:14:00+00:00",
            ),
        ];
        let mut engine = DesktopEmbeddingEngine::load().unwrap();
        let vectors = engine
            .embed(&articles.iter().map(canonical_text).collect::<Vec<_>>())
            .unwrap();
        for (article, mut vector) in articles.iter_mut().zip(vectors) {
            normalize_vector(&mut vector);
            article.embedding = Some(vector);
        }
        let decision = score_pair(0, 1, &articles, 14.0 / 60.0);
        assert!(decision.eligible, "{decision:#?}");
    }

    #[derive(Deserialize)]
    struct CorpusPair {
        label: bool,
        left: String,
        right: String,
        category: String,
    }

    #[cfg(not(target_os = "ios"))]
    #[test]
    fn labelled_corpus_meets_precision_gate() {
        let corpus: Vec<CorpusPair> = serde_json::from_str(include_str!(
            "../tests/fixtures/semantic_clustering.json"
        ))
        .unwrap();
        let mut articles = Vec::with_capacity(corpus.len() * 2);
        for (index, pair) in corpus.iter().enumerate() {
            let mut left = article(
                &format!("{index}-left"),
                &pair.left,
                "2026-06-08T12:00:00+00:00",
            );
            let mut right = article(
                &format!("{index}-right"),
                &pair.right,
                "2026-06-08T13:00:00+00:00",
            );
            left.category = pair.category.clone();
            right.category = pair.category.clone();
            articles.push(left);
            articles.push(right);
        }
        let mut engine = DesktopEmbeddingEngine::load().unwrap();
        let vectors = engine
            .embed(&articles.iter().map(canonical_text).collect::<Vec<_>>())
            .unwrap();
        for (article, mut vector) in articles.iter_mut().zip(vectors) {
            normalize_vector(&mut vector);
            article.embedding = Some(vector);
        }

        let mut true_positive = 0usize;
        let mut false_positive = 0usize;
        let positives = corpus.iter().filter(|pair| pair.label).count();
        for (index, pair) in corpus.iter().enumerate() {
            let decision = score_pair(index * 2, index * 2 + 1, &articles, 1.0);
            if decision.eligible && pair.label {
                true_positive += 1;
            } else if decision.eligible {
                false_positive += 1;
            }
            eprintln!(
                "label={} score={:.3} semantic={:.3} lexical={:.3} {:?}",
                pair.label,
                decision.score,
                decision.components.semantic,
                decision.components.lexical,
                decision.veto
            );
        }
        let predicted = true_positive + false_positive;
        let precision = if predicted == 0 {
            1.0
        } else {
            true_positive as f32 / predicted as f32
        };
        let recall = true_positive as f32 / positives as f32;
        assert!(precision >= 0.99, "precision={precision:.3}");
        assert!(recall >= 0.66, "recall={recall:.3}");
    }

    #[test]
    fn reclustering_is_idempotent_and_preserves_oldest_cluster_id() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        let first = raw_article(
            "a",
            "Maltese film Żejtune has its North American premiere",
        );
        let second = raw_article(
            "b",
            "Maltese Film Żejtune Premieres At Tribeca Film Festival In New York",
        );
        db::insert_article(&conn, &first).unwrap();
        db::insert_article(&conn, &second).unwrap();
        db::upsert_cluster(
            &conn,
            "oldest",
            &first.original_headline,
            &first.published_at,
            &first.published_at,
            false,
        )
        .unwrap();
        db::set_cluster(&conn, "a", "oldest").unwrap();
        db::upsert_cluster(
            &conn,
            "newer",
            &second.original_headline,
            &second.published_at,
            &second.published_at,
            false,
        )
        .unwrap();
        db::set_cluster(&conn, "b", "newer").unwrap();

        assert_eq!(
            recluster_with_engine(&conn, &mut KeywordEngine, false).unwrap(),
            1
        );
        let ids_after_first: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT cluster_id FROM articles ORDER BY id")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        assert_eq!(ids_after_first, vec!["oldest", "oldest"]);

        assert_eq!(
            recluster_with_engine(&conn, &mut KeywordEngine, false).unwrap(),
            1
        );
        let ids_after_second: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT cluster_id FROM articles ORDER BY id")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        assert_eq!(ids_after_first, ids_after_second);
    }

    struct FailingEngine;

    impl EmbeddingEngine for FailingEngine {
        fn embed(&mut self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Err(anyhow!("inference failed"))
        }
    }

    #[test]
    fn inference_failure_leaves_existing_clusters_untouched() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        let item = raw_article("a", "Existing story");
        db::insert_article(&conn, &item).unwrap();
        db::upsert_cluster(
            &conn,
            "existing",
            &item.original_headline,
            &item.published_at,
            &item.published_at,
            false,
        )
        .unwrap();
        db::set_cluster(&conn, "a", "existing").unwrap();
        assert!(recluster_with_engine(&conn, &mut FailingEngine, true).is_err());
        let cluster: String = conn
            .query_row(
                "SELECT cluster_id FROM articles WHERE id = 'a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cluster, "existing");
    }

    #[test]
    fn persistence_failure_rolls_back_embeddings_and_assignments() {
        let conn = db::open(Path::new(":memory:")).unwrap();
        let item = raw_article("a", "Maltese film Żejtune premieres at Tribeca");
        db::insert_article(&conn, &item).unwrap();
        db::upsert_cluster(
            &conn,
            "existing",
            &item.original_headline,
            &item.published_at,
            &item.published_at,
            false,
        )
        .unwrap();
        db::set_cluster(&conn, "a", "existing").unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_diagnostics
             BEFORE INSERT ON clustering_diagnostics
             BEGIN
               SELECT RAISE(ABORT, 'forced diagnostic failure');
             END;",
        )
        .unwrap();
        let second = raw_article(
            "b",
            "Żejtune has its North American premiere in New York",
        );
        db::insert_article(&conn, &second).unwrap();

        assert!(recluster_with_engine(&conn, &mut KeywordEngine, true).is_err());
        let state: (Option<Vec<u8>>, Option<String>) = conn
            .query_row(
                "SELECT embedding, cluster_id FROM articles WHERE id = 'a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(state.0.is_none());
        assert_eq!(state.1.as_deref(), Some("existing"));
    }
}
