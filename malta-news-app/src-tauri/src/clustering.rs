use std::collections::{HashMap, HashSet};

use crate::models::BiasCategory;
use crate::publishers::PUBLISHERS;

pub struct ClusterAssignment {
    pub cluster_id: String,
    pub is_new: bool,
}

/// Words too generic to be useful for matching news stories.
const STOP_WORDS: &[&str] = &[
    "malta", "maltese", "gozo", "says", "said", "claims", "updated",
    "breaking", "watch", "news", "report", "today", "people", "first",
    "last", "after", "before", "been", "being", "from", "have", "that",
    "this", "they", "their", "them", "then", "than", "these", "those",
    "were", "what", "when", "where", "which", "while", "with", "will",
    "would", "could", "should", "about", "also", "back", "called",
    "come", "does", "down", "each", "even", "every", "gets", "give",
    "goes", "going", "gone", "good", "great", "here", "high", "however",
    "into", "just", "know", "left", "like", "long", "look", "made",
    "make", "many", "more", "most", "much", "must", "need", "next",
    "only", "open", "other", "over", "part", "same", "show", "some",
    "still", "such", "take", "tell", "time", "told", "took", "turn",
    "under", "upon", "used", "very", "want", "well", "went", "year",
    "years", "your", "held", "hold",
];

/// Token weight: numbers and short uppercase-preserved tokens score highest.
///   number  → 3   (e.g. "2026", "€50m", "10")
///   proper  → 2   (capitalised in original, e.g. "Abela", "Gozo")
///   plain   → 1
#[derive(Clone)]
struct Token {
    word: String,
    weight: u32,
}

/// Normalise a headline before tokenisation:
/// • remove thousands separators:  "4,000" → "4000", "€2,500" → "€2500"
/// • expand k / m suffixes:        "4k" → "4000", "€22m" → "€22000000"
fn normalize_headline(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c.is_ascii_digit() {
            // Consume run of digits, dropping embedded commas (thousands separators)
            while i < n && (chars[i].is_ascii_digit() || chars[i] == ',') {
                if chars[i] != ',' {
                    out.push(chars[i]);
                }
                i += 1;
            }
            // Expand k / m suffix when not followed by another alphanumeric char
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
/// `original` is the mixed-case original text (needed to detect proper nouns).
fn tokenize_weighted(original: &str) -> Vec<Token> {
    let normalized = normalize_headline(original);
    let stops: HashSet<&str> = STOP_WORDS.iter().copied().collect();
    let lower = normalized.to_lowercase();

    normalized
        .split(|c: char| !c.is_alphanumeric())
        .zip(lower.split(|c: char| !c.is_alphanumeric()))
        .filter(|(_, lw)| lw.len() > 2 && !stops.contains(*lw))
        .map(|(orig_w, lw)| {
            let weight = if lw.chars().all(|c| c.is_ascii_digit()) || lw.starts_with('€') || lw.starts_with('$') {
                3 // number / currency
            } else if orig_w.chars().next().map_or(false, |c| c.is_uppercase()) && lw.len() >= 3 {
                2 // proper noun
            } else {
                1
            };
            Token { word: lw.to_string(), weight }
        })
        .collect()
}

/// Weighted Jaccard: sum of weights of shared tokens / sum of weights of union tokens.
fn weighted_jaccard(a: &[Token], b: &[Token]) -> (f32, u32) {
    let a_map: HashMap<&str, u32> = a.iter().map(|t| (t.word.as_str(), t.weight)).collect();
    let b_map: HashMap<&str, u32> = b.iter().map(|t| (t.word.as_str(), t.weight)).collect();

    let mut intersection_w = 0u32;
    let mut union_w = 0u32;
    let mut max_shared_weight = 0u32;

    let all_words: HashSet<&str> = a_map.keys().chain(b_map.keys()).copied().collect();
    for word in all_words {
        let wa = a_map.get(word).copied().unwrap_or(0);
        let wb = b_map.get(word).copied().unwrap_or(0);
        intersection_w += wa.min(wb);
        union_w += wa.max(wb);
        if wa > 0 && wb > 0 {
            max_shared_weight = max_shared_weight.max(wa.min(wb));
        }
    }

    let score = if union_w == 0 { 0.0 } else { intersection_w as f32 / union_w as f32 };
    (score, max_shared_weight)
}

/// Primary match threshold.
/// A single shared proper noun (weight 2) on short headlines gives ~0.125, which we
/// intentionally reject — two Momentum stories about *different* topics must not cluster.
const JACCARD_THRESHOLD: f32 = 0.15;

/// Secondary threshold: only fires when a NUMBER token (weight 3) is shared,
/// ensuring currency/figure-driven stories still group even at lower Jaccard scores.
const NUMBER_BOOST_THRESHOLD: f32 = 0.08;

/// Assign an article to the best matching cluster based on headline keyword overlap.
/// Compares against every headline variant stored in the cluster, takes the best score.
pub fn assign_cluster(
    article_headline: &str,
    cluster_headlines: &HashMap<String, Vec<String>>,
    new_cluster_id: &str,
) -> ClusterAssignment {
    let article_tokens = tokenize_weighted(article_headline);

    let mut best_id = "";
    let mut best_score = 0.0f32;

    for (cid, headlines) in cluster_headlines {
        for headline in headlines {
            let cluster_tokens = tokenize_weighted(headline);
            let (score, max_shared_weight) = weighted_jaccard(&article_tokens, &cluster_tokens);

            let passes = score >= JACCARD_THRESHOLD
                || (max_shared_weight >= 3 && score >= NUMBER_BOOST_THRESHOLD);

            if passes && score > best_score {
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

const INDEPENDENT: &[BiasCategory] = &[BiasCategory::CommercialIndependent, BiasCategory::InvestigativeIndependent];

/// A cluster is a blindspot when no independent outlet (commercial or investigative)
/// covered the story — regardless of which non-independent outlets did cover it.
/// This flags party-only, state-only, church-only, and any combination without independents.
pub fn is_blindspot(publisher_ids: &[&str]) -> bool {
    if publisher_ids.is_empty() { return false; }
    let categories: HashSet<BiasCategory> = publisher_ids.iter()
        .filter_map(|id| PUBLISHERS.get(id).map(|p| p.bias_category))
        .collect();
    if categories.is_empty() { return false; }
    !categories.iter().any(|c| INDEPENDENT.contains(c))
}

// ── Best headline selection ─────────────────────────────────────────────────

/// Score a publisher for headline neutrality.
/// Higher = more neutral/independent.
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
/// Prefers independent sources (most neutral framing), then longest headline.
/// Input: (original_headline, translated_headline, language, publisher_id)
pub fn pick_best_headline(
    articles: &[(String, String, String, String)],
) -> String {
    if articles.is_empty() {
        return String::new();
    }

    articles
        .iter()
        .map(|(headline, translated, lang, pub_id)| {
            // Get the English headline for the cluster display
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
