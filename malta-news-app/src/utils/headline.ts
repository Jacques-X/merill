import type { Article, StoryCluster } from "@/types";

/** Pick the right headline for an article based on language preference.
 *  translated_headline is always the "other" language:
 *    - MT articles: translated_headline = English
 *    - EN articles: translated_headline = Maltese
 */
export function articleHeadline(article: Article, lang: "en" | "mt"): string {
  // If article is already in the requested language, use original
  if (lang === article.language) return article.original_headline;
  // Otherwise use the translation (falls back to original if empty)
  return article.translated_headline || article.original_headline;
}

/** Pick the right headline for a cluster based on language preference. */
export function clusterHeadline(cluster: StoryCluster, lang: "en" | "mt"): string {
  if (lang === "en") return cluster.primary_headline;
  // For Maltese: prefer a native MT article's headline
  const mtArticle = cluster.articles.find(a => a.language === "mt");
  if (mtArticle) return mtArticle.original_headline;
  // Otherwise use the EN->MT translation from the first article
  const first = cluster.articles[0];
  return first?.translated_headline || cluster.primary_headline;
}
