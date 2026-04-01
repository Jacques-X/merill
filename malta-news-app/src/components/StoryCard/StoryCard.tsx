import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatDistanceToNow } from "date-fns";
import { BiasBar } from "@/components/BiasBar/BiasBar";
import { computeBiasCoverage } from "@/utils/bias";
import { BIAS_COLORS } from "@/utils/constants";
import { clusterHeadline } from "@/utils/headline";
import { t } from "@/utils/i18n";
import { useAppStore } from "@/store/useAppStore";
import type { StoryCluster } from "@/types";

// Per-category accent colours for the image placeholder.
const CATEGORY_COLORS: Record<string, string> = {
  politics: "#3B82F6",
  sport: "#10B981",
  local: "#8B5CF6",
  international: "#06B6D4",
  crime: "#EF4444",
  business: "#F59E0B",
  opinion: "#EC4899",
  entertainment: "#F97316",
  general: "#6B7280",
};

interface StoryCardProps {
  cluster: StoryCluster;
  onPress?: (c: StoryCluster) => void;
  animationDelay?: string;
}

export function StoryCard({ cluster, onPress, animationDelay = "0s" }: StoryCardProps) {
  const lang = useAppStore(s => s.language);
  const [imgError, setImgError] = useState(false);
  const [logoErrors, setLogoErrors] = useState<Set<string>>(new Set());

  // AI-rewritten headline + summary — seeded from DB cache, then generated on first view.
  const [aiHeadline, setAiHeadline] = useState(cluster.ai_headline);
  const [aiSummary, setAiSummary]   = useState(cluster.ai_summary);

  useEffect(() => {
    if (aiHeadline && aiSummary) return; // already cached
    const headlines = cluster.articles.map(a => a.translated_headline).filter(Boolean);
    const snippets  = cluster.articles.map(a => a.snippet).filter(Boolean);
    if (!headlines.length) return;
    invoke<{ headline: string; summary: string }>("generate_cluster_summary", {
      clusterId: cluster.id,
      headlines,
      snippets,
    }).then(r => {
      if (r.headline) setAiHeadline(r.headline);
      if (r.summary)  setAiSummary(r.summary);
    }).catch(() => { /* keep fallback */ });
  }, [cluster.id]); // eslint-disable-line react-hooks/exhaustive-deps

  const coverage = computeBiasCoverage(cluster.articles);
  const timeAgo = formatDistanceToNow(new Date(cluster.first_reported_at), { addSuffix: false });
  const visible = cluster.articles.slice(0, 4);
  const overflow = Math.max(0, cluster.articles.length - 4);
  const imageUrl = !imgError ? cluster.articles.find(a => a.image_url)?.image_url : undefined;

  // Use AI summary when available, fall back to raw snippet / body_text.
  const snippet = aiSummary || (() => {
    for (const a of cluster.articles) {
      if (a.snippet) return a.snippet.slice(0, 140);
      if (a.body_text) return a.body_text.split("\n\n")[0]?.slice(0, 140);
    }
    return null;
  })();

  // Dominant category for the placeholder colour.
  const dominantCategory = cluster.articles[0]?.category ?? "general";
  const placeholderColor = CATEGORY_COLORS[dominantCategory] ?? CATEGORY_COLORS.general;

  return (
    <button
      className="story-card animate-fade-up"
      style={{ animationDelay }}
      onClick={() => onPress?.(cluster)}
    >
      {/* Image or category colour placeholder */}
      {imageUrl ? (
        <div className="story-card-img">
          <img
            src={imageUrl}
            alt=""
            className="story-card-img-inner"
            loading="lazy"
            onError={() => setImgError(true)}
          />
        </div>
      ) : (
        <div
          className="story-card-img story-card-img-placeholder"
          style={{ background: `linear-gradient(135deg, ${placeholderColor}33, ${placeholderColor}11)` }}
        >
          <span className="story-card-cat-label">
            {t(lang, `cat${dominantCategory.charAt(0).toUpperCase()}${dominantCategory.slice(1)}` as Parameters<typeof t>[1])}
          </span>
        </div>
      )}

      {/* Content */}
      <div className="story-card-body">
        {/* Headline — AI-rewritten when available, raw cluster headline as fallback */}
        <h2 className="story-card-headline">
          {aiHeadline || clusterHeadline(cluster, lang)}
        </h2>

        {/* Snippet */}
        {snippet && (
          <p className="story-card-snippet">
            {snippet}{!aiSummary && "…"}
            <span className="story-card-seemore">{t(lang, "seeMore")}</span>
          </p>
        )}

        {/* Bias bar */}
        <BiasBar coverage={coverage} compact />

        {/* Bottom row: time + source avatars */}
        <div className="story-card-footer">
          <span className="story-card-time">{timeAgo} {t(lang, "ago")}</span>
          <div className="source-avatars">
            {visible.map(a => (
              <div
                key={a.id}
                className="source-avatar"
                style={{ backgroundColor: BIAS_COLORS[a.publisher.bias_category] ?? "#8E8E93" }}
              >
                {a.publisher.logo_url && !logoErrors.has(a.id) ? (
                  <img
                    src={a.publisher.logo_url}
                    alt={a.publisher.name}
                    onError={() => setLogoErrors(s => new Set(s).add(a.id))}
                  />
                ) : (
                  <span>{a.publisher.name.charAt(0)}</span>
                )}
              </div>
            ))}
            {overflow > 0 && (
              <div className="source-avatar overflow-avatar">
                <span>+{overflow}</span>
              </div>
            )}
          </div>
        </div>
      </div>
    </button>
  );
}
