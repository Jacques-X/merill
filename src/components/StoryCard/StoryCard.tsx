import { useState, useEffect, useMemo, useRef, memo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { formatDistanceToNow } from "date-fns";
import { enUS } from "date-fns/locale";
import { Bookmark, EyeOff, MoreHorizontal } from "lucide-react";
import { BiasBar } from "@/components/BiasBar/BiasBar";
import { computeBiasCoverage } from "@/utils/bias";
import { BIAS_COLORS } from "@/utils/constants";
import { clusterHeadline } from "@/utils/headline";
import { t } from "@/utils/i18n";
import { useAppStore, sessionBaseline } from "@/store/useAppStore";
import type { StoryCluster } from "@/types";

interface StoryCardProps {
  cluster: StoryCluster;
  onPress?: (c: StoryCluster) => void;
  onDismiss?: (id: string) => void;
  isSaved?: boolean;
  onToggleSaved?: (cluster: StoryCluster) => void;
  animationDelay?: string;
}

export const StoryCard = memo(function StoryCard({ cluster, onPress, onDismiss, isSaved = false, onToggleSaved, animationDelay = "0s" }: StoryCardProps) {
  const lang = useAppStore(s => s.language);
  const biasOverrides = useAppStore(s => s.publisherBiasOverrides);
  const articles = cluster.articles;
  const [imgError, setImgError] = useState(false);
  const [logoErrors, setLogoErrors] = useState<Set<string>>(new Set());
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  // AI-rewritten headline + summary — seeded from DB cache, then generated on first view.
  const [aiHeadline, setAiHeadline] = useState(cluster.ai_headline);
  const [aiSummary, setAiSummary]   = useState(cluster.ai_summary);

  useEffect(() => {
    if (aiHeadline && aiSummary) return; // already cached
    const headlines = articles.map(a => a.translated_headline).filter(Boolean);
    const snippets  = articles.map(a => a.snippet).filter(Boolean);
    if (!headlines.length) return;
    invoke<{ headline: string; summary: string }>("generate_cluster_summary", {
      clusterId: cluster.id,
      headlines,
      snippets,
    }).then(r => {
      if (r.headline) setAiHeadline(r.headline);
      if (r.summary)  setAiSummary(r.summary);
    }).catch(() => { /* keep fallback */ });
  // Re-run when the article count changes (new articles joined the cluster).
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cluster.id, articles]);

  // Close overflow menu on outside click or Escape.
  useEffect(() => {
    if (!menuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setMenuOpen(false); };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [menuOpen]);

  const isNew = cluster.first_reported_at > sessionBaseline.current;
  const coverage = useMemo(
    () => computeBiasCoverage(articles, biasOverrides),
    [articles, biasOverrides],
  );
  const timeAgo = formatDistanceToNow(new Date(cluster.first_reported_at), { addSuffix: false, locale: enUS });
  const imageUrl = !imgError ? articles.find(a => a.image_url)?.image_url : undefined;

  // Group articles by publisher.
  const { visiblePubs, overflow } = useMemo(() => {
    const byPublisher = articles.reduce((acc, a) => {
      if (!acc.has(a.publisher_id)) acc.set(a.publisher_id, []);
      acc.get(a.publisher_id)!.push(a);
      return acc;
    }, new Map<string, typeof articles>());
    const unique = [...byPublisher.entries()];
    return { visiblePubs: unique.slice(0, 4), overflow: Math.max(0, unique.length - 4) };
  }, [articles]);

  // Use AI summary when available, fall back to raw snippet / body_text.
  const snippet = useMemo(() => {
    if (aiSummary) return aiSummary;
    for (const a of articles) {
      if (a.snippet) return a.snippet.slice(0, 140);
      if (a.body_text) return a.body_text.split("\n\n")[0]?.slice(0, 140);
    }
    return null;
  }, [aiSummary, articles]);

  // Reading time — only shown when real body text is present. The short snippet/AI summary
  // doesn't represent the full article, so a word-count estimate from it would be misleading.
  const readMins = useMemo(() => {
    for (const a of articles) {
      if (a.body_text && a.body_text.length > 100) {
        const words = a.body_text.split(/\s+/).filter(Boolean).length;
        return Math.max(1, Math.round(words / 200));
      }
    }
    return 0;
  }, [articles]);

  // Memoize the cluster headline so it doesn't rerun when only aiHeadline state changes.
  const fallbackHeadline = useMemo(
    () => clusterHeadline(cluster, lang),
    [cluster, lang],
  );

  const isAiHeadline = !!aiHeadline;
  const visibleHeadline = aiHeadline || fallbackHeadline;

  return (
    // <article> is not interactive — no role="button" needed.
    // Action buttons and the main pressable area are separate siblings so
    // assistive tech never encounters nested interactive elements.
    <article
      className="story-card animate-fade-up"
      style={{ animationDelay }}
    >
      {/* Absolutely positioned action buttons — outside the main pressable button */}
      <div className="story-card-actions" aria-label="Story actions">
        <button
          type="button"
          className={`save-btn${isSaved ? " saved" : ""}`}
          aria-label={isSaved ? t(lang, "unsave") : t(lang, "save")}
          aria-pressed={isSaved}
          onClick={(e) => {
            e.stopPropagation();
            onToggleSaved?.(cluster);
          }}
        >
          <Bookmark size={17} fill={isSaved ? "currentColor" : "none"} />
        </button>
        {onDismiss && (
          <div className="card-overflow" ref={menuRef}>
            <button
              type="button"
              className="card-action-btn"
              aria-label={t(lang, "moreActions")}
              aria-expanded={menuOpen}
              onClick={(e) => { e.stopPropagation(); setMenuOpen(open => !open); }}
            >
              <MoreHorizontal size={18} />
            </button>
            {menuOpen && (
              <button
                type="button"
                className="card-overflow-item"
                onClick={(e) => { e.stopPropagation(); onDismiss(cluster.id); setMenuOpen(false); }}
              >
                <EyeOff size={15} />
                {t(lang, "hideStory")}
              </button>
            )}
          </div>
        )}
      </div>

      {/* Main navigable area — single focusable element for keyboard / assistive tech */}
      <button
        type="button"
        className="story-card-main"
        onClick={() => onPress?.(cluster)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onPress?.(cluster);
          }
        }}
        aria-label={visibleHeadline}
      >
        {imageUrl && (
          <div className="story-card-img">
            <img
              src={imageUrl}
              alt=""
              className="story-card-img-inner"
              loading="lazy"
              onError={() => setImgError(true)}
            />
          </div>
        )}

        <div className="story-card-body">
          {isNew && <span className="new-badge">{t(lang, "newBadge")}</span>}

          {/* Headline row: real headline + AI badge when the headline was rewritten */}
          <div className="story-card-headline-row">
            <h2 className="story-card-headline">{visibleHeadline}</h2>
            {isAiHeadline && (
              <span className="ai-label" aria-label={t(lang, "aiSummaryLabel")}>
                {t(lang, "aiSummaryLabel")}
              </span>
            )}
          </div>

          {snippet && (
            <p className="story-card-snippet">
              {snippet}{!aiSummary && "…"}
              <span className="story-card-seemore">{t(lang, "seeMore")}</span>
            </p>
          )}

          {cluster.blindspot_explanation.missing_independent_coverage && (
            <p className="story-blindspot-note">
              <EyeOff size={13} />
              {cluster.blindspot_explanation.publisher_count} {cluster.blindspot_explanation.publisher_count === 1 ? t(lang, "source") : t(lang, "sources")} · {t(lang, "noIndependentCoverage")}
            </p>
          )}

          <BiasBar coverage={coverage} compact />

          <div className="story-card-footer">
            <span className="story-card-time">
              {timeAgo} {t(lang, "ago")}
              {readMins > 0 && <> · ~{readMins} {t(lang, "minRead")}</>}
            </span>
            <div className="source-avatars">
              {visiblePubs.map(([pubId, pubArticles]) => {
                const a = pubArticles[0];
                const count = pubArticles.length;
                return (
                  <div key={pubId} style={{ position: "relative" }}>
                    <div
                      className="source-avatar"
                      style={{ backgroundColor: BIAS_COLORS[biasOverrides[pubId] ?? a.publisher.bias_category] ?? "#8E8E93" }}
                    >
                      {a.publisher.logo_url && !logoErrors.has(pubId) ? (
                        <img
                          src={a.publisher.logo_url}
                          alt={a.publisher.name}
                          onError={() => setLogoErrors(s => new Set(s).add(pubId))}
                        />
                      ) : (
                        <span>{a.publisher.name.slice(0, 2).toUpperCase()}</span>
                      )}
                    </div>
                    {count > 1 && (
                      <span className="avatar-count-badge">{count}</span>
                    )}
                  </div>
                );
              })}
              {overflow > 0 && (
                <div className="source-avatar overflow-avatar">
                  <span>+{overflow}</span>
                </div>
              )}
            </div>
          </div>
        </div>
      </button>
    </article>
  );
});
