export type BiasCategory = "state_owned"|"party_owned_pl"|"party_owned_pn"|"church_owned"|"commercial_independent"|"investigative_independent"|"left"|"centre"|"right";
export interface Publisher { id: string; name: string; bias_category: BiasCategory; logo_url: string; is_global: boolean; }
export type Category = "politics"|"sport"|"local"|"international"|"crime"|"business"|"opinion"|"entertainment"|"general";
export interface Article { id: string; publisher_id: string; publisher: Publisher; original_url: string; original_headline: string; translated_headline: string; snippet: string; body_text: string; image_url: string; language: "en"|"mt"; published_at: string; story_cluster_id: string; category: Category; }
export interface BlindspotExplanation { covered_categories: BiasCategory[]; missing_independent_coverage: boolean; publisher_count: number; }
export interface PerspectiveArticle { article_id: string; publisher_id: string; publisher_name: string; headline: string; snippet: string; published_at: string; }
export interface PerspectiveGroup { bias_category: BiasCategory; common_terms: string[]; distinct_terms: string[]; articles: PerspectiveArticle[]; }
export interface StoryCluster { id: string; story_key: string; primary_headline: string; first_reported_at: string; last_updated: string; is_blindspot: boolean; ai_headline: string; ai_summary: string; blindspot_explanation: BlindspotExplanation; perspective_groups: PerspectiveGroup[]; articles: Article[]; }
export interface BiasCoverage { state_owned: number; party_owned_pl: number; party_owned_pn: number; church_owned: number; commercial_independent: number; investigative_independent: number; left: number; centre: number; right: number; total_articles: number; }
export interface AppSettings { theme: "system"|"light"|"dark"; language: "en"|"mt"; feedScope: "local"|"global"; savedStoryKeys: string[]; localDisabledPublisherIds: string[]; globalDisabledPublisherIds: string[]; publisherBiasOverrides: Record<string, BiasCategory>; }
export interface ClustersResponse { clusters: StoryCluster[]; }
export interface RefreshResult { message: string; failed_sources: string[]; }
export interface RefreshStatus { last_refresh_at: string | null; cooldown_remaining_seconds: number; failed_sources: string[]; }
