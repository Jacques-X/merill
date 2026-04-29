use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

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
    pub token_set: HashSet<String>,
    pub last_updated: String, // ISO 8601
    /// Dominant category of this cluster's articles; None if not yet determined.
    pub category: Option<String>,
    /// Publisher IDs that have contributed an article to this cluster.
    pub publisher_ids: HashSet<String>,
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
    // Maltese function words (>2 chars that survive the length filter)
    "tal", "tas", "tad", "tar", "taz",   // genitive particle combos
    "fil", "bil", "mal", "bħal",          // preposition combos
    "dan", "din", "dawn", "dak", "dik", "dawk", // demonstratives
    "kif", "jew", "imma", "iżda",         // conjunctions
    "wara", "fuq", "taħt", "ġewwa", "barra", "qabel", "waqt", // prepositions
    "ukoll", "wkoll", "biss", "biex", "minn", "mhux",          // particles
    "meta", "bhal", "hemm", "hawn",
    "mill", "bejn", "talli", "lanqas", "wisq",
];

/// Known Malta-specific proper nouns that deserve weight-2 even when sentence-initial.
const MALTA_ENTITIES: &[&str] = &[
    // Politicians (recent/frequent)
    "abela", "grech", "metsola", "muscat", "busuttil", "gonzi", "mintoff",
    "delia", "bartolo", "bonnici", "scicluna", "mizzi", "fenech", "agius",
    "cutajar", "falzon", "buhagiar", "zammit", "vassallo", "camilleri",
    // Institutions & agencies
    "mepa", "mfsa", "mcast", "mita", "enemalta", "wasteserv", "arpa",
    "mfin", "mcast", "indis",
    // Local places beyond Malta/Gozo stop-word
    "mdina", "birgu", "senglea", "cospicua", "bormla", "marsaxlokk",
    "marsaskala", "mellieha", "naxxar", "mosta", "rabat", "qormi",
    "zejtun", "attard", "balzan", "lija", "iklin", "victoria", "sannat",
    "xaghra", "xewkija", "gharb", "kercem",
    // Party names (3+ chars to survive length filter)
    "adpd",
];

fn malta_entity_set() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| MALTA_ENTITIES.iter().copied().collect())
}

/// Similarity thresholds.
const COSINE_BASE: f32 = 0.35;
/// Lowered threshold when a high-specificity proper noun or number is shared.
const COSINE_ANCHOR: f32 = 0.26;
/// Stale-cluster penalty: multiply base threshold by this when gap > STALE_HOURS.
const COSINE_STALE_MULT: f32 = 1.5;
const STALE_HOURS: f64 = 72.0;
/// Minimum IDF a token needs to qualify as an "anchor".
const ANCHOR_MIN_IDF: f32 = 3.0;
/// Minimum distinct tokens an article must share with a cluster before cosine is computed.
const MIN_SHARED_TOKENS: usize = 2;

// #10: Adaptive threshold by cluster size
const ADAPTIVE_LARGE_CLUSTER_BUMP: f32 = 0.05; // clusters with ≥5 articles
const ADAPTIVE_SMALL_CLUSTER_REDUCTION: f32 = 0.03; // clusters with ≤2 articles

// #6b: Same-publisher threshold bump
const SAME_PUBLISHER_BUMP: f32 = 0.07;

// #8: Time-aware boost constants — rewards clusters updated recently
const TIME_BOOST_MAX: f32 = 0.20; // max multiplicative bonus
const TIME_BOOST_DECAY_H: f64 = 12.0; // half-life ~8h

// #7: Threshold for second-pass cluster-to-cluster merge
const MERGE_THRESHOLD: f32 = 0.55;

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
///   2 — all-caps acronym (EU, PN), known Malta entity, or title-case proper noun at non-position-0
///   1 — ordinary word
pub fn tokenize_weighted(original: &str) -> Vec<Token> {
    let normalized = normalize_headline(original);
    let stops: HashSet<&str> = STOP_WORDS.iter().copied().collect();
    let entities = malta_entity_set();
    let lower = normalized.to_lowercase();

    let mut result = Vec::new();
    for (i, (orig_w, lw)) in normalized
        .split(|c: char| !c.is_alphanumeric())
        .zip(lower.split(|c: char| !c.is_alphanumeric()))
        .enumerate()
    {
        // Allow 2-char tokens only when they are all-uppercase acronyms (PL, PN, EU).
        let is_acronym = lw.len() == 2 && orig_w.chars().all(|c| c.is_uppercase());
        if (lw.len() < 2 || (lw.len() == 2 && !is_acronym)) || stops.contains(lw) {
            continue;
        }

        let weight = if lw.chars().all(|c| c.is_ascii_digit()) {
            3 // number
        } else if orig_w.len() >= 2
            && orig_w.chars().all(|c| c.is_uppercase() || !c.is_alphabetic())
        {
            2 // all-caps acronym — proper noun even at position 0
        } else if entities.contains(lw) {
            2 // known Malta entity regardless of sentence position (#5)
        } else if i > 0 && orig_w.chars().next().map_or(false, |c| c.is_uppercase()) {
            2 // title-case proper noun, but only at non-sentence-initial position
        } else {
            1
        };

        result.push(Token { word: lw.to_string(), weight });
    }

    add_bigrams(result)
}

/// Append compound tokens for adjacent proper nouns / numbers.
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

pub fn build_idf_table(cluster_data: &HashMap<String, ClusterData>) -> HashMap<String, f32> {
    let n = cluster_data.len() as f32;
    if n == 0.0 {
        return HashMap::new();
    }

    let mut df: HashMap<String, u32> = HashMap::new();

    for data in cluster_data.values() {
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

fn tfidf_cosine(
    a: &[Token],
    b: &[Token],
    idf: &HashMap<String, f32>,
) -> (f32, bool) {
    if a.is_empty() || b.is_empty() {
        return (0.0, false);
    }

    let default_idf = 1.0_f32;

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
        Some(year * 8766.0 + month * 730.5 + day * 24.0 + hour)
    }
    let ha = to_hours(a).unwrap_or(0.0);
    let hb = to_hours(b).unwrap_or(0.0);
    (ha - hb).abs()
}

// ── Cluster assignment ───────────────────────────────────────────────────────

/// Assign a new article to the best matching existing cluster, or create a new one.
///
/// Improvements over baseline:
///   #3  — category gating: skip clusters in a different category
///   #6b — same-publisher bump: raise threshold when cluster already has this publisher
///   #8  — time boost: multiplicative reward for clusters updated near the article's date
///   #10 — adaptive threshold: adjust by cluster size
pub fn assign_cluster(
    article_headline: &str,
    article_published_at: &str,
    article_category: &str,
    article_publisher_id: &str,
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

    let article_word_set: HashSet<&str> = article_tokens.iter().map(|t| t.word.as_str()).collect();

    for (cid, data) in cluster_data {
        // #3 — Category gating: skip cross-category clusters unless either side is "general"
        if let Some(ref cluster_cat) = data.category {
            if cluster_cat != "general"
                && article_category != "general"
                && cluster_cat.as_str() != article_category
            {
                continue;
            }
        }

        // Fast reject: require minimum shared token overlap
        let shared_count = article_word_set.iter()
            .filter(|w| data.token_set.contains(**w))
            .count();
        if shared_count < MIN_SHARED_TOKENS {
            continue;
        }

        let time_delta = hours_between(article_published_at, &data.last_updated);
        let is_stale = time_delta > STALE_HOURS;

        // #8 — Time boost: reward fresh clusters (decays with a ~8h half-life)
        let time_boost = 1.0_f32 + TIME_BOOST_MAX * ((-time_delta / TIME_BOOST_DECAY_H) as f32).exp();

        // #6b — Same-publisher bump
        let same_publisher = data.publisher_ids.contains(article_publisher_id);

        // #10 — Adaptive threshold by cluster size
        let cluster_size = data.tokenized_headlines.len();

        for cluster_tokens in &data.tokenized_headlines {
            let (raw_score, has_anchor) = tfidf_cosine(&article_tokens, cluster_tokens, idf);
            let score = raw_score * time_boost;

            let mut threshold = if is_stale {
                COSINE_BASE * COSINE_STALE_MULT
            } else if has_anchor {
                COSINE_ANCHOR
            } else {
                COSINE_BASE
            };

            if cluster_size >= 5 {
                threshold += ADAPTIVE_LARGE_CLUSTER_BUMP;
            } else if cluster_size <= 2 {
                threshold -= ADAPTIVE_SMALL_CLUSTER_REDUCTION;
            }

            if same_publisher {
                threshold += SAME_PUBLISHER_BUMP;
            }

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

// ── Second-pass cluster-to-cluster merge (#7) ───────────────────────────────

/// Aggregate all tokens from a cluster into a single bag (max weight per word).
fn aggregate_tokens(data: &ClusterData) -> Vec<Token> {
    let mut word_weights: HashMap<String, u32> = HashMap::new();
    for tokens in &data.tokenized_headlines {
        for t in tokens {
            let entry = word_weights.entry(t.word.clone()).or_insert(0);
            *entry = (*entry).max(t.weight);
        }
    }
    word_weights.into_iter().map(|(word, weight)| Token { word, weight }).collect()
}

/// Walk the merge chain to its final target (cycle-safe).
fn resolve_merge_target(id: &str, merges: &HashMap<String, String>) -> String {
    let mut current = id.to_string();
    for _ in 0..20 {
        match merges.get(&current) {
            Some(next) => current = next.clone(),
            None => break,
        }
    }
    current
}

/// After the greedy per-article assignment, sweep cluster-vs-cluster and merge
/// any pair whose aggregate centroid similarity exceeds MERGE_THRESHOLD.
/// Returns a map of cluster_id → merge_target_id for all clusters that should be absorbed.
pub fn find_cluster_merges(
    cluster_data: &HashMap<String, ClusterData>,
    idf: &HashMap<String, f32>,
) -> HashMap<String, String> {
    let mut merges: HashMap<String, String> = HashMap::new();

    // Sort by size descending so larger clusters become merge targets
    let mut ids: Vec<(&String, usize)> = cluster_data
        .iter()
        .map(|(id, data)| (id, data.tokenized_headlines.len()))
        .collect();
    ids.sort_by(|a, b| b.1.cmp(&a.1));

    for i in (0..ids.len()).rev() {
        let (cid_a, _) = ids[i];
        if merges.contains_key(cid_a.as_str()) {
            continue;
        }
        let data_a = &cluster_data[cid_a];
        let agg_a = aggregate_tokens(data_a);

        let mut best_target: Option<String> = None;
        let mut best_score = MERGE_THRESHOLD;

        // Only consider merging into larger clusters (j < i in sorted-descending order)
        for j in 0..i {
            let (cid_b, _) = ids[j];
            let final_b = resolve_merge_target(cid_b.as_str(), &merges);
            if final_b == cid_a.as_str() {
                continue;
            }

            let data_b = match cluster_data.get(&final_b) {
                Some(d) => d,
                None => continue,
            };

            // Category gating
            if let (Some(cat_a), Some(cat_b)) = (&data_a.category, &data_b.category) {
                if cat_a != "general" && cat_b != "general" && cat_a != cat_b {
                    continue;
                }
            }

            // Stricter shared-token filter for cluster merges
            let shared_count = data_a.token_set.iter()
                .filter(|w| data_b.token_set.contains(*w))
                .count();
            if shared_count < MIN_SHARED_TOKENS + 1 {
                continue;
            }

            let agg_b = aggregate_tokens(data_b);
            let (score, has_anchor) = tfidf_cosine(&agg_a, &agg_b, idf);

            if has_anchor && score > best_score {
                best_score = score;
                best_target = Some(cid_b.clone());
            }
        }

        if let Some(target) = best_target {
            merges.insert(cid_a.clone(), target);
        }
    }

    merges
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
/// Input: (original_headline, translated_headline, language, publisher_id, snippet, category)
pub fn pick_best_headline(articles: &[(String, String, String, String, String, String)]) -> String {
    if articles.is_empty() {
        return String::new();
    }
    articles
        .iter()
        .map(|(headline, translated, lang, pub_id, _snippet, _cat)| {
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
