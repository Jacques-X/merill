use std::collections::{HashMap, HashSet};

use crate::models::BiasCategory;
use crate::publishers::PUBLISHERS;

pub struct ClusterAssignment {
    pub cluster_id: String,
    pub is_new: bool,
}

/// Words too generic to be useful for matching news stories.
const STOP_WORDS: &[&str] = &[
    "malta", "maltese", "gozo", "minister", "government", "says", "said",
    "claims", "updated", "breaking", "watch", "court", "police", "news",
    "report", "today", "people", "first", "last", "after", "before",
    "been", "being", "from", "have", "that", "this", "they", "their",
    "them", "then", "than", "these", "those", "were", "what", "when",
    "where", "which", "while", "with", "will", "would", "could", "should",
    "about", "also", "back", "been", "called", "come", "does", "down",
    "each", "even", "every", "gets", "give", "goes", "going", "gone",
    "good", "great", "here", "high", "however", "into", "just", "know",
    "left", "like", "long", "look", "made", "make", "many", "more",
    "most", "much", "must", "need", "next", "only", "open", "other",
    "over", "part", "same", "show", "some", "still", "such", "take",
    "tell", "time", "told", "took", "turn", "under", "upon", "used",
    "very", "want", "well", "went", "year", "years", "your",
    "new", "set", "held", "hold", "plan", "plans", "planned",
];

/// Extract significant words from a headline for comparison.
fn tokenize(text: &str) -> HashSet<String> {
    let stops: HashSet<&str> = STOP_WORDS.iter().copied().collect();
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3 && !stops.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// Jaccard similarity: |intersection| / |union|.
fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 { 0.0 } else { intersection as f32 / union as f32 }
}

const JACCARD_THRESHOLD: f32 = 0.30;
const MIN_SHARED_WORDS: usize = 2;

/// Assign an article to the best matching cluster based on headline keyword overlap.
pub fn assign_cluster(
    article_headline: &str,
    cluster_headlines: &HashMap<String, String>,
    new_cluster_id: &str,
) -> ClusterAssignment {
    let article_words = tokenize(article_headline);

    let mut best_id = "";
    let mut best_score = 0.0f32;

    for (cid, headline) in cluster_headlines {
        let cluster_words = tokenize(headline);
        let shared = article_words.intersection(&cluster_words).count();
        if shared < MIN_SHARED_WORDS {
            continue;
        }
        let score = jaccard(&article_words, &cluster_words);
        if score > best_score && score >= JACCARD_THRESHOLD {
            best_score = score;
            best_id = cid;
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
