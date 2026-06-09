use serde::{Deserialize, Serialize};

/// Bias category for a publisher — matches the frontend type exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BiasCategory {
    StateOwned,
    PartyOwnedPl,
    PartyOwnedPn,
    ChurchOwned,
    CommercialIndependent,
    InvestigativeIndependent,
}

/// How to scrape a publisher.
#[derive(Debug, Clone)]
pub enum ScrapeMethod {
    Rss {
        url: &'static str,
    },
    Html {
        url: &'static str,
        article_sel: &'static str,
        headline_sel: &'static str,
        image_sel: &'static str,
        link_attr: &'static str,
        base_url: &'static str,
    },
    Sitemap {
        url: &'static str,
    },
}

/// Static publisher definition.
#[derive(Debug, Clone)]
pub struct PublisherDef {
    pub id: &'static str,
    pub name: &'static str,
    pub bias_category: BiasCategory,
    pub primary_language: &'static str,
    pub logo_url: &'static str,
    pub scrape: ScrapeMethod,
}

/// Publisher info sent to the frontend (matches TS `Publisher` interface).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublisherInfo {
    pub id: String,
    pub name: String,
    pub bias_category: BiasCategory,
    pub logo_url: String,
    /// true = international/global source, false = Malta local source.
    pub is_global: bool,
}

/// User-added custom publisher stored in the DB.
/// `scrape_method` is one of: "rss" | "sitemap" | "html"
/// `scrape_config` carries extra data per method:
///   rss / sitemap → "" (URL is enough)
///   html          → the CSS selector string (e.g. "h2 a[href]")
#[derive(Debug, Clone)]
pub struct CustomPublisherDef {
    pub id: String,
    pub name: String,
    /// The URL to scrape (RSS feed URL, sitemap URL, or homepage URL for HTML).
    pub rss_url: String,
    /// The publisher's public website, which may differ from a hosted feed URL.
    pub site_url: String,
    /// The icon discovered from the publisher website.
    pub logo_url: String,
    pub scrape_method: String,
    pub scrape_config: String,
    pub is_global: bool,
}

/// A single article sent to the frontend (matches TS `Article` interface).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Article {
    pub id: String,
    pub publisher_id: String,
    pub publisher: PublisherInfo,
    pub original_url: String,
    pub original_headline: String,
    pub translated_headline: String,
    pub snippet: String,
    pub body_text: String,
    pub image_url: String,
    pub language: String,
    pub published_at: String,
    pub story_cluster_id: String,
    pub category: String,
}

/// A story cluster sent to the frontend (matches TS `StoryCluster` interface).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoryCluster {
    pub id: String,
    pub story_key: String,
    pub primary_headline: String,
    pub first_reported_at: String,
    pub last_updated: String,
    pub is_blindspot: bool,
    /// AI-rewritten headline (empty string until generated).
    pub ai_headline: String,
    /// AI-generated summary (empty string until generated).
    pub ai_summary: String,
    pub blindspot_explanation: BlindspotExplanation,
    pub perspective_groups: Vec<PerspectiveGroup>,
    pub articles: Vec<Article>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindspotExplanation {
    pub covered_categories: Vec<BiasCategory>,
    pub missing_independent_coverage: bool,
    pub publisher_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerspectiveArticle {
    pub article_id: String,
    pub publisher_id: String,
    pub publisher_name: String,
    pub headline: String,
    pub snippet: String,
    pub published_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerspectiveGroup {
    pub bias_category: BiasCategory,
    pub common_terms: Vec<String>,
    pub distinct_terms: Vec<String>,
    pub articles: Vec<PerspectiveArticle>,
}

/// Response wrapper (matches TS `ClustersResponse` interface).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClustersResponse {
    pub clusters: Vec<StoryCluster>,
}

/// Response for on-demand article body fetching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleBody {
    pub body_text: String,
    pub image_url: String,
}

/// Result returned by `refresh_feed` command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshResult {
    pub message: String,
    pub failed_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshStatus {
    pub last_refresh_at: Option<String>,
    pub cooldown_remaining_seconds: u64,
    pub failed_sources: Vec<String>,
}

/// Internal article representation before clustering.
#[derive(Debug, Clone)]
pub struct RawArticle {
    pub id: String,
    pub publisher_id: String,
    pub original_url: String,
    pub original_headline: String,
    pub translated_headline: String,
    pub body_snippet: String,
    pub body_text: String,
    pub image_url: String,
    pub language: String,
    pub published_at: String,
    pub category: String,
}
