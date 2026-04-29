use std::collections::{HashMap, HashSet};

use crate::models::BiasCategory;
use crate::publishers::PUBLISHERS;

pub struct ClusterAssignment {
    pub cluster_id: String,
    pub is_new: bool,
}

pub struct ClusterData {
    pub headlines: Vec<String>,
    /// Pre-tokenized forms of `headlines` — kept in sync so `assign_cluster`
    /// never re-tokenizes the same headline twice.
    pub tokenized_headlines: Vec<Vec<Token>>,
    /// Union of all token words across every headline in this cluster.
    /// Used as a fast-reject filter: a new article must share at least
    /// MIN_SHARED_TOKENS distinct tokens with this set before cosine similarity
    /// is computed. Also used by `build_idf_table` to avoid re-tokenizing.
    pub token_set: HashSet<String>,
    pub last_updated: String, // ISO 8601
}

/// Words too generic to be useful for matching news stories.
const STOP_WORDS: &[&str] = &[
    // Malta geography — appears in virtually every headline, adds no discrimination
    "malta", "maltese", "gozo", "gozitan", "valletta",
    // Reporting verbs — present in almost every news headline
    "says", "said", "claims", "claim", "report", "reports", "reported",
    "confirms", "confirmed", "deny", "denies", "announces", "announced",
    "statement", "breaking", "exclusive", "updated", "watch", "news",
    // Temporal markers
    "today", "yesterday", "tomorrow",
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
    "week", "weeks", "month", "months", "year", "years", "day", "days",
    "annual", "weekly", "monthly",
    // Position / ordinal words
    "first", "second", "third", "last", "next", "new",
    // Generic quantifiers and modifiers
    "more", "most", "much", "many", "some", "all", "also", "even", "just",
    "only", "very", "still", "back", "high", "good", "great", "long",
    "open", "same", "each", "every", "other", "such", "over", "under",
    "upon", "part", "need", "want",
    // Generic verbs that don't identify a story
    "come", "does", "down", "gets", "give", "goes", "going", "gone",
    "hold", "held", "know", "left", "like", "look", "made", "make",
    "take", "tell", "told", "took", "turn", "used", "went", "well",
    "call", "called", "show", "here", "people",
    // Function words
    "about", "after", "before", "being", "been", "from", "have", "into",
    "that", "this", "they", "their", "them", "then", "than", "these",
    "those", "were", "what", "when", "where", "which", "while", "with",
    "will", "would", "could", "should",
    // News-domain institutional terms — too common across unrelated stories
    // to discriminate between them; removing these forces clustering to rely
    // on specific names, numbers, and places instead.
    "police", "court", "judge", "magistrate", "tribunal",
    "arrest", "arrested", "charged", "convicted", "sentenced", "jailed",
    "victim", "suspect", "accused", "guilty", "crime", "criminal",
    "government", "minister", "ministry", "parliament", "opposition",
    "council", "authority", "commission", "board", "committee",
    "public", "national", "local", "european", "international",
    "million", "billion", "percent", "according",
    "company", "business", "sector",
    "school", "university", "hospital",
    "development", "project", "plan", "proposed", "scheme",
    "decision", "proposal", "issue", "issues",
    "man", "woman", "family", "children", "residents", "person",
    "found", "given", "asked", "major", "official", "officials",
];

/// Similarity thresholds.
const COSINE_BASE: f32 = 0.35;
/// Lowered threshold when a high-specificity proper noun or number is shared.
const COSINE_ANCHOR: f32 = 0.26;
/// Stale-cluster penalty: multiply base threshold by this when gap > STALE_HOURS.
const COSINE_STALE_MULT: f32 = 1.5;
const STALE_HOURS: f64 = 72.0;
/// Minimum IDF a token needs to qualify as an "anchor" (prevents generic proper nouns
/// from triggering the lower threshold).
const ANCHOR_MIN_IDF: f32 = 3.0;
/// Minimum distinct tokens an article must share with a cluster before
/// cosine similarity is even computed — prevents false matches from a single
/// generic shared word.
const MIN_SHARED_TOKENS: usize = 2;

#[derive(Clone)]
pub struct Token {
    pub word: String,
    pub weight: u32,
}

/// Normalise numeric strings before tokenisation:
///   thousands separators: "4,000" → "4000"
///   k/m suffixes:         "4k" → "4000", "€22m" → "€22000000"
fn normalize_headline(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c.is_ascii_digit() {
            while i < n && (chars[i].is_ascii_digit() || chars[i] == ',') {
                if chars[i] != ',' {
                    out.push(chars[i]);
                }
                i += 1;
            }
            if i < n {
                let suffix = chars[i];
                let not_followed = i + 1 >= n || !chars[i + 1].is_alphanumeric();
                if not_followed {
                    if suffix == 'k' || suffix == 'K' {
                        out.push_str("000");
                        i += 1;
                        continue;
                    } else if suffix == 'm' || suffix == 'M' {
                        out.push_str("000000");
                        i += 1;
                        continue;
                    }
                }
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Extract weighted tokens from a headline.
///
/// Weights:
///   3 — pure number / currency figure
///   2 — proper noun (capitalised **not** at position 0, to avoid the
///        sentence-initial capital false-positive), or an all-caps acronym (EU, PN)
///   1 — ordinary word
pub fn tokenize_weighted(original: &str) -> Vec<Token> {
    let normalized = normalize_headline(original);
    let stops: HashSet<&str> = STOP_WORDS.iter().copied().collect();
    let lower = normalized.to_lowercase();

    // Zip the two split iterators directly — avoids collecting both into Vec<&str>.
    let mut result = Vec::new();
    for (i, (orig_w, lw)) in normalized
        .split(|c: char| !c.is_alphanumeric())
        .zip(lower.split(|c: char| !c.is_alphanumeric()))
        .enumerate()
    {
        if lw.len() <= 2 || stops.contains(lw) {
            continue;
        }

        let weight = if lw.chars().all(|c| c.is_ascii_digit()) {
            3 // number
        } else if orig_w.len() >= 2 && orig_w.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()) {
            // All-caps acronym (EU, PN, PL, AFP) — proper noun even at position 0
            2
        } else if i > 0 && orig_w.chars().next().map_or(false, |c| c.is_uppercase()) {
            // Title-case proper noun, but only when NOT the first token
            // (first word is always capitalised in English headlines)
            2
        } else {
            1
        };

        result.push(Token { word: lw.to_string(), weight });
    }

    add_bigrams(result)
}

/// Append compound tokens for adjacent proper nouns / numbers.
///
/// Example: ["robert"(2), "abela"(2)] → adds "robert_abela"(3)
/// The original tokens are kept so partial matches ("Abela" alone) still work.
fn add_bigrams(mut tokens: Vec<Token>) -> Vec<Token> {
    let n = tokens.len();
    let mut bigrams = Vec::new();
    for i in 0..n.saturating_sub(1) {
        if tokens[i].weight >= 2 && tokens[i + 1].weight >= 2 {
            bigrams.push(Token {
                word: format!("{}_{}", tokens[i].word, tokens[i + 1].word),
                weight: 3,
            });
        }
    }
    tokens.extend(bigrams);
    tokens
}

// ── IDF table ───────────────────────────────────────────────────────────────

/// Build a smoothed IDF table from all cluster headlines.
///
///   IDF(w) = ln((N + 1) / (df(w) + 1)) + 1
///
/// where N = number of clusters and df(w) = clusters that contain word w.
pub fn build_idf_table(cluster_data: &HashMap<String, ClusterData>) -> HashMap<String, f32> {
    let n = cluster_data.len() as f32;
    if n == 0.0 {
        return HashMap::new();
    }

    let mut df: HashMap<String, u32> = HashMap::new();

    for data in cluster_data.values() {
        // token_set is already the per-cluster unique word set — no re-tokenization needed.
        for word in &data.token_set {
            *df.entry(word.clone()).or_insert(0) += 1;
        }
    }

    df.into_iter()
        .map(|(word, count)| {
            let idf = ((n + 1.0) / (count as f32 + 1.0)).ln() + 1.0;
            (word, idf)
        })
        .collect()
}

// ── TF-IDF cosine similarity ────────────────────────────────────────────────

/// Compute TF-IDF cosine similarity between two token lists.
///
/// Score = dot(a, b) / (|a| * |b|)
/// where each token dimension = token_weight * IDF.
///
/// Also returns `has_anchor`: true when a shared token has weight ≥ 2 AND
/// IDF ≥ ANCHOR_MIN_IDF (indicating a rare enough proper noun or number).
fn tfidf_cosine(
    a: &[Token],
    b: &[Token],
    idf: &HashMap<String, f32>,
) -> (f32, bool) {
    if a.is_empty() || b.is_empty() {
        return (0.0, false);
    }

    let default_idf = 1.0_f32;

    // Build a-side: store (weight, tfidf_score) for anchor detection during the b-pass.
    let a_scores: HashMap<&str, (u32, f32)> = a
        .iter()
        .map(|t| {
            let idf_val = idf.get(t.word.as_str()).copied().unwrap_or(default_idf);
            (t.word.as_str(), (t.weight, t.weight as f32 * idf_val))
        })
        .collect();

    let mag_a_sq: f32 = a_scores.values().map(|(_, v)| v * v).sum();
    if mag_a_sq == 0.0 {
        return (0.0, false);
    }

    // Single pass over b: compute dot product, |b|², and anchor detection simultaneously.
    // Eliminates the b_scores HashMap entirely.
    let mut dot = 0.0_f32;
    let mut mag_b_sq = 0.0_f32;
    let mut has_anchor = false;

    for t in b {
        let idf_val = idf.get(t.word.as_str()).copied().unwrap_or(default_idf);
        let wb = t.weight as f32 * idf_val;
        mag_b_sq += wb * wb;
        if let Some(&(a_weight, wa)) = a_scores.get(t.word.as_str()) {
            dot += wa * wb;
            if !has_anchor
                && a_weight >= 2
                && t.word.len() >= 3
                && idf.get(t.word.as_str()).copied().unwrap_or(0.0) >= ANCHOR_MIN_IDF
            {
                has_anchor = true;
            }
        }
    }

    if dot == 0.0 || mag_b_sq == 0.0 {
        return (0.0, false);
    }

    (dot / (mag_a_sq.sqrt() * mag_b_sq.sqrt()), has_anchor)
}

// ── Time helper ─────────────────────────────────────────────────────────────

/// Rough hours between two ISO 8601 timestamps ("YYYY-MM-DDTHH:…").
/// Precision is ~1 hour, good enough for the 72-hour staleness check.
fn hours_between(a: &str, b: &str) -> f64 {
    fn to_hours(s: &str) -> Option<f64> {
        let bytes = s.as_bytes();
        if bytes.len() < 13 {
            return None;
        }
        let year: f64  = std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?;
        let month: f64 = std::str::from_utf8(&bytes[5..7]).ok()?.parse().ok()?;
        let day: f64   = std::str::from_utf8(&bytes[8..10]).ok()?.parse().ok()?;
        let hour: f64  = std::str::from_utf8(&bytes[11..13]).ok()?.parse().ok()?;
        // Approximate: months treated as 30.44 days, years as 365.25 days
        Some(year * 8766.0 + month * 730.5 + day * 24.0 + hour)
    }
    let ha = to_hours(a).unwrap_or(0.0);
    let hb = to_hours(b).unwrap_or(0.0);
    (ha - hb).abs()
}

// ── Cluster assignment ───────────────────────────────────────────────────────

/// Assign a new article to the best matching existing cluster, or create a new one.
///
/// Uses TF-IDF cosine similarity with:
///   - lower threshold when a rare proper-noun / number anchor is shared
///   - higher threshold when article is > 72 h newer than the cluster
pub fn assign_cluster(
    article_headline: &str,
    article_published_at: &str,
    cluster_data: &HashMap<String, ClusterData>,
    idf: &HashMap<String, f32>,
    new_cluster_id: &str,
) -> ClusterAssignment {
    let article_tokens = tokenize_weighted(article_headline);

    if article_tokens.is_empty() {
        return ClusterAssignment { cluster_id: new_cluster_id.to_string(), is_new: true };
    }

    let mut best_id = "";
    let mut best_score = 0.0_f32;

    // Collect the article's distinct token words once for the fast-reject below.
    let article_word_set: HashSet<&str> = article_tokens.iter().map(|t| t.word.as_str()).collect();

    for (cid, data) in cluster_data {
        // Fast reject: require at least MIN_SHARED_TOKENS distinct tokens in common.
        // A single shared generic word (even after stop-word filtering) is not
        // enough evidence that two headlines are about the same story.
        let shared_count = article_word_set.iter()
            .filter(|w| data.token_set.contains(**w))
            .count();
        if shared_count < MIN_SHARED_TOKENS {
            continue;
        }

        let is_stale = hours_between(article_published_at, &data.last_updated) > STALE_HOURS;

        for cluster_tokens in &data.tokenized_headlines {
            let (score, has_anchor) = tfidf_cosine(&article_tokens, cluster_tokens, idf);

            let threshold = if is_stale {
                COSINE_BASE * COSINE_STALE_MULT
            } else if has_anchor {
                COSINE_ANCHOR
            } else {
                COSINE_BASE
            };

            if score >= threshold && score > best_score {
                best_score = score;
                best_id = cid;
            }
        }
    }

    if !best_id.is_empty() {
        ClusterAssignment { cluster_id: best_id.to_string(), is_new: false }
    } else {
        ClusterAssignment { cluster_id: new_cluster_id.to_string(), is_new: true }
    }
}

// ── Blindspot detection ─────────────────────────────────────────────────────

const INDEPENDENT: &[BiasCategory] = &[
    BiasCategory::CommercialIndependent,
    BiasCategory::InvestigativeIndependent,
];

pub fn is_blindspot(publisher_ids: &[&str]) -> bool {
    if publisher_ids.is_empty() {
        return false;
    }
    let categories: HashSet<BiasCategory> = publisher_ids
        .iter()
        .filter_map(|id| PUBLISHERS.get(id).map(|p| p.bias_category))
        .collect();
    if categories.is_empty() {
        return false;
    }
    !categories.iter().any(|c| INDEPENDENT.contains(c))
}

// ── Best headline selection ─────────────────────────────────────────────────

fn source_score(publisher_id: &str) -> u8 {
    match PUBLISHERS.get(publisher_id).map(|p| p.bias_category) {
        Some(BiasCategory::InvestigativeIndependent) => 4,
        Some(BiasCategory::CommercialIndependent) => 3,
        Some(BiasCategory::ChurchOwned) => 2,
        Some(BiasCategory::StateOwned) => 1,
        Some(BiasCategory::PartyOwnedPl) | Some(BiasCategory::PartyOwnedPn) => 0,
        None => 1,
    }
}

/// Pick the most representative headline for a cluster.
/// Input: (original_headline, translated_headline, language, publisher_id, snippet)
pub fn pick_best_headline(articles: &[(String, String, String, String, String)]) -> String {
    if articles.is_empty() {
        return String::new();
    }
    articles
        .iter()
        .map(|(headline, translated, lang, pub_id, _snippet)| {
            let en_headline = if lang == "en" {
                headline.as_str()
            } else if !translated.is_empty() {
                translated.as_str()
            } else {
                headline.as_str()
            };
            let score = source_score(pub_id);
            (en_headline, score, en_headline.len())
        })
        .max_by_key(|(_, score, len)| (*score, *len))
        .map(|(h, _, _)| h.to_string())
        .unwrap_or_else(|| articles[0].0.clone())
}
