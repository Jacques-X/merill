import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { formatDistanceToNow } from "date-fns";
import { invoke } from "@tauri-apps/api/core";
import { useClusters, usePublishers, refreshFeed, addCustomPublisher, removeCustomPublisher, clusterKeys } from "@/api/clusters";
import { StoryCard } from "@/components/StoryCard/StoryCard";
import { BiasBar } from "@/components/BiasBar/BiasBar";
import { computeBiasCoverage } from "@/utils/bias";
import { BIAS_COLORS } from "@/utils/constants";
import { articleHeadline, clusterHeadline } from "@/utils/headline";
import { t } from "@/utils/i18n";
import { useAppStore } from "@/store/useAppStore";
import type { StoryCluster, Category } from "@/types";

// ── Pull-to-Refresh Hook ────────────────────────────────────────────────────

function usePullToRefresh(onRefresh: () => Promise<void>, enabled: boolean) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [pullDistance, setPullDistance] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const startY = useRef(0);
  const isPulling = useRef(false);
  const THRESHOLD = 80;

  useEffect(() => {
    const el = containerRef.current;
    if (!el || !enabled) return;

    const onTouchStart = (e: TouchEvent) => {
      if (el.scrollTop <= 0) {
        startY.current = e.touches[0].clientY;
        isPulling.current = true;
      }
    };
    const onTouchMove = (e: TouchEvent) => {
      if (!isPulling.current) return;
      const dy = e.touches[0].clientY - startY.current;
      if (dy > 0) {
        e.preventDefault();
        setPullDistance(Math.min(dy * 0.5, 120));
      } else {
        isPulling.current = false;
        setPullDistance(0);
      }
    };
    const onTouchEnd = async () => {
      if (!isPulling.current) return;
      isPulling.current = false;
      if (pullDistance >= THRESHOLD) {
        setRefreshing(true);
        setPullDistance(THRESHOLD);
        try { await onRefresh(); } finally { setRefreshing(false); setPullDistance(0); }
      } else {
        setPullDistance(0);
      }
    };

    el.addEventListener("touchstart", onTouchStart, { passive: true });
    el.addEventListener("touchmove", onTouchMove, { passive: false });
    el.addEventListener("touchend", onTouchEnd);
    return () => {
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("touchmove", onTouchMove);
      el.removeEventListener("touchend", onTouchEnd);
    };
  }, [enabled, onRefresh, pullDistance]);

  const progress = Math.min(pullDistance / THRESHOLD, 1);
  return { containerRef, pullDistance, refreshing, progress };
}

// ── Extractive summary: merge first paragraphs from all sources into one ──

function combineSummary(bodyTexts: string[]): string {
  const texts = bodyTexts.filter(Boolean);
  if (texts.length === 0) return "";
  if (texts.length === 1) {
    const first = texts[0].split("\n\n").slice(0, 2).join(" ");
    return first.slice(0, 400);
  }

  const allSentences: string[] = [];
  for (const t of texts) {
    const chunk = t.split("\n\n").slice(0, 2).join(" ");
    const sentences = chunk.match(/[^.!?]+[.!?]+/g) || [chunk];
    for (const s of sentences) {
      const trimmed = s.trim();
      if (trimmed.length > 25) allSentences.push(trimmed);
    }
  }

  const getWords = (s: string) =>
    new Set(s.toLowerCase().replace(/[^a-z\s]/g, "").split(/\s+/).filter(w => w.length > 3));
  const picked: string[] = [];
  for (const sent of allSentences) {
    const sentWords = getWords(sent);
    if (sentWords.size < 2) continue;
    const isDup = picked.some(existing => {
      const ew = getWords(existing);
      const shared = [...sentWords].filter(w => ew.has(w)).length;
      const smaller = Math.min(sentWords.size, ew.size);
      return smaller > 0 && shared / smaller > 0.5;
    });
    if (!isDup) picked.push(sent);
    if (picked.length >= 5) break;
  }

  return picked.join(" ").slice(0, 500);
}

// ── Story Detail Screen ─────────────────────────────────────────────────────

export function StoryDetailScreen({
  cluster,
  internalBackRef,
}: {
  cluster: StoryCluster;
  internalBackRef?: React.MutableRefObject<(() => void) | null>;
}) {
  const lang = useAppStore(s => s.language);
  const [selectedArticle, setSelectedArticle] = useState<import("@/types").Article | null>(null);
  const [articleBody, setArticleBody] = useState<string>("");
  const [loadingBody, setLoadingBody] = useState(false);
  const [imgError, setImgError] = useState(false);
  const [logoErrors, setLogoErrors] = useState<Set<string>>(new Set());
  const coverage = computeBiasCoverage(cluster.articles);
  const imageUrl = !imgError ? cluster.articles.find(a => a.image_url)?.image_url : undefined;

  const [summaries, setSummaries] = useState<Map<string, string>>(new Map());
  const [summaryLoading, setSummaryLoading] = useState(true);
  const [translatedSummary, setTranslatedSummary] = useState<string>("");

  // Register internal back with parent so dock/swipe back works correctly:
  // article open → back goes to cluster view; cluster view → back goes to feed.
  useEffect(() => {
    if (!internalBackRef) return;
    internalBackRef.current = selectedArticle ? () => setSelectedArticle(null) : null;
    return () => { internalBackRef.current = null; };
  }, [selectedArticle, internalBackRef]);

  useEffect(() => {
    let cancelled = false;
    setSummaryLoading(true);

    async function fetchAll() {
      const promises = cluster.articles.map(async (a) => {
        try {
          const r = await invoke<{ body_text: string; image_url: string }>("fetch_article_body", {
            articleId: a.id,
            url: a.original_url,
          });
          return { id: a.id, text: r.body_text };
        } catch {
          return { id: a.id, text: "" };
        }
      });
      const all = await Promise.all(promises);
      if (cancelled) return;
      const results = new Map<string, string>();
      for (const { id, text } of all) {
        if (text) results.set(id, text);
      }
      setSummaries(results);
      setSummaryLoading(false);
    }
    fetchAll();
    return () => { cancelled = true; };
  }, [cluster.articles]);

  useEffect(() => {
    if (summaryLoading || summaries.size === 0) return;
    const combined = combineSummary([...summaries.values()]);
    if (!combined) { setTranslatedSummary(""); return; }

    const mtCount = cluster.articles.filter(a => a.language === "mt").length;
    const majorityLang = mtCount > cluster.articles.length / 2 ? "mt" : "en";

    if (lang === majorityLang) {
      setTranslatedSummary(combined);
      return;
    }

    let cancelled = false;
    setTranslatedSummary("");
    invoke<string>("translate_summary", { text: combined, to: lang })
      .then(translated => { if (!cancelled) setTranslatedSummary(translated); })
      .catch(() => { if (!cancelled) setTranslatedSummary(combined); });
    return () => { cancelled = true; };
  }, [summaryLoading, summaries, lang, cluster.articles]);

  const openArticle = useCallback(async (a: import("@/types").Article) => {
    setSelectedArticle(a);
    const cached = summaries.get(a.id);
    setArticleBody(cached || a.body_text || "");
    if (!cached && !a.body_text) {
      setLoadingBody(true);
      try {
        const result = await invoke<{ body_text: string; image_url: string }>("fetch_article_body", {
          articleId: a.id,
          url: a.original_url,
        });
        setArticleBody(result.body_text);
      } catch (e) {
        console.error("failed to fetch article body:", e);
      } finally {
        setLoadingBody(false);
      }
    }
  }, [summaries]);

  // ── Article Reader View
  if (selectedArticle) {
    const a = selectedArticle;
    const paragraphs = articleBody ? articleBody.split("\n\n").filter(Boolean) : [];
    const domain = a.original_url.replace(/^https?:\/\//, "").split("/")[0];

    return (
      <div className="animate-fade-up detail-scroll">
        {a.image_url && (
          <div className="detail-hero">
            <img src={a.image_url} alt="" />
            <div className="detail-hero-fade" />
          </div>
        )}

        <div className={`detail-content ${a.image_url ? "has-hero" : ""}`}>
          <div className="detail-publisher">
            <div className="source-avatar lg" style={{
              backgroundColor: BIAS_COLORS[a.publisher.bias_category] ?? "#8E8E93",
            }}>
              {a.publisher.logo_url && !logoErrors.has(a.id) ? (
                <img src={a.publisher.logo_url} alt={a.publisher.name}
                  onError={() => setLogoErrors(s => new Set(s).add(a.id))} />
              ) : (
                <span>{a.publisher.name.charAt(0)}</span>
              )}
            </div>
            <div>
              <p className="detail-pub-name">{a.publisher.name}</p>
              <p className="detail-pub-time">{formatDistanceToNow(new Date(a.published_at), { addSuffix: true })}</p>
            </div>
          </div>

          <h2 className="detail-headline">{articleHeadline(a, lang)}</h2>

          {paragraphs.length > 0 ? (
            <div className="detail-body">
              {paragraphs.map((p, i) => (<p key={i}>{p}</p>))}
            </div>
          ) : loadingBody ? (
            <div className="detail-loading">
              <div className="spinner" />
              <span>{t(lang, "loadingArticle")}</span>
            </div>
          ) : (
            <div className="detail-empty-body">{t(lang, "noBodyText")}</div>
          )}

          <a href={a.original_url} target="_blank" rel="noopener noreferrer" className="read-original-btn">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--color-accent)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
              <polyline points="15 3 21 3 21 9" />
              <line x1="10" y1="14" x2="21" y2="3" />
            </svg>
            {t(lang, "readOn")} {domain}
          </a>

          <button onClick={() => setSelectedArticle(null)} className="back-to-sources">
            {t(lang, "viewAllSources").replace("{n}", String(cluster.articles.length))}
          </button>
        </div>
      </div>
    );
  }

  // ── Cluster Overview with combined summary
  return (
    <div className="animate-fade-up detail-scroll">
      {imageUrl && (
        <div className="detail-hero tall">
          <img src={imageUrl} alt="" onError={() => setImgError(true)} />
          <div className="detail-hero-fade" />

          {cluster.articles.length > 1 && (
            <span className="hero-badge">
              {cluster.articles.length} {t(lang, "sources")}
            </span>
          )}
        </div>
      )}

      <div className={`detail-content ${imageUrl ? "has-hero" : ""}`}>
        <h2 className="detail-headline lg">{clusterHeadline(cluster, lang)}</h2>

        {summaryLoading || (summaries.size > 0 && !translatedSummary) ? (
          <div className="detail-loading">
            <div className="spinner" />
            <span>{t(lang, "loadingArticle")}</span>
          </div>
        ) : translatedSummary ? (
          <p className="detail-summary">{translatedSummary}</p>
        ) : null}

        <div className="detail-bias-section">
          <BiasBar coverage={coverage} />
        </div>

        <div className="source-headlines">
          {cluster.articles.map(a => (
            <button key={a.id} className="source-headline-row" onClick={() => openArticle(a)}>
              <div className="source-avatar sm" style={{
                backgroundColor: BIAS_COLORS[a.publisher.bias_category] ?? "#8E8E93",
              }}>
                {a.publisher.logo_url && !logoErrors.has(a.id) ? (
                  <img src={a.publisher.logo_url} alt={a.publisher.name}
                    onError={() => setLogoErrors(s => new Set(s).add(a.id))} />
                ) : (
                  <span>{a.publisher.name.charAt(0)}</span>
                )}
              </div>
              <div className="source-headline-text">
                <p className="source-headline">
                  {articleHeadline(a, lang)}
                </p>
                <span className="source-headline-meta">
                  {a.publisher.name} · {formatDistanceToNow(new Date(a.published_at), { addSuffix: true })}
                </span>
              </div>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

// ── Skeletons ────────────────────────────────────────────────────────────────

function CardSkeleton({ delay = "0s", showImage = false }: { delay?: string; showImage?: boolean }) {
  return (
    <div className="story-card skeleton-card animate-fade-up" style={{ animationDelay: delay }}>
      {showImage && <div className="skeleton" style={{ width: "100%", height: 200, borderRadius: 0 }} />}
      <div style={{ padding: 16, display: "flex", flexDirection: "column", gap: 12 }}>
        <div className="skeleton" style={{ height: 16, width: "90%" }} />
        <div className="skeleton" style={{ height: 16, width: "70%" }} />
        <div className="skeleton" style={{ height: 12, width: "100%" }} />
        <div style={{ display: "flex", gap: 6 }}>
          {[0, 1, 2].map(i => <div key={i} className="skeleton" style={{ width: 28, height: 28, borderRadius: 14 }} />)}
        </div>
      </div>
    </div>
  );
}

// ── Feed Screen ─────────────────────────────────────────────────────────────

const ALL_CATEGORIES: ("all" | Category)[] = ["all", "politics", "sport", "local", "international", "crime", "business", "opinion", "entertainment", "general"];
const CAT_I18N: Record<string, import("@/utils/i18n").LangKey> = {
  all: "catAll", politics: "catPolitics", sport: "catSport", local: "catLocal",
  international: "catInternational", crime: "catCrime", business: "catBusiness",
  opinion: "catOpinion", entertainment: "catEntertainment", general: "catGeneral",
};

export type FeedFilter = "local" | "global";

export function FeedScreen({
  onSelectCluster,
  filter = "local",
}: {
  onSelectCluster: (c: StoryCluster) => void;
  filter?: FeedFilter;
}) {
  const lang = useAppStore(s => s.language);
  const localDisabledPublisherIds = useAppStore(s => s.localDisabledPublisherIds);
  const globalDisabledPublisherIds = useAppStore(s => s.globalDisabledPublisherIds);
  const queryClient = useQueryClient();
  const { data, isLoading, isError, refetch } = useClusters();
  const [refreshing, setRefreshing] = useState(false);
  const [shuffleKey, setShuffleKey] = useState(0);
  const [activeCategory, setActiveCategory] = useState<"all" | Category>("all");
  const [failedSources, setFailedSources] = useState<string[]>([]);

  const rawClusters = data?.clusters ?? [];

  // Pin shuffle scores in a ref — only recompute when shuffleKey changes.
  const shuffleScores = useRef<Map<string, number>>(new Map());
  const prevShuffleKey = useRef(-1);

  const clusters = useMemo(() => {
    // Recompute scores only on explicit refresh.
    if (shuffleKey !== prevShuffleKey.current) {
      prevShuffleKey.current = shuffleKey;
      shuffleScores.current = new Map(
        rawClusters.map(c => {
          let h = shuffleKey;
          for (let i = 0; i < c.id.length; i++) h = (h * 31 + c.id.charCodeAt(i)) | 0;
          const rand = ((h >>> 0) % 1000) / 1000;
          return [c.id, Math.log(1 + c.articles.length) + rand * 2];
        })
      );
    }

    let arr = [...rawClusters];

    // Restrict each cluster to articles matching the active tab's locality,
    // then drop clusters that have no articles left.
    const disabledPubs = filter === "local" ? localDisabledPublisherIds : globalDisabledPublisherIds;
    arr = arr
      .map(c => ({
        ...c,
        articles: c.articles.filter(a =>
          (filter === "local" ? !a.publisher.is_global : a.publisher.is_global) &&
          !disabledPubs.includes(a.publisher_id)
        ),
      }))
      .filter(c => c.articles.length > 0);

    // Apply category filter.
    if (activeCategory !== "all") {
      arr = arr.filter(c => c.articles.some(a => a.category === activeCategory));
    }

    arr.sort((a, b) =>
      (shuffleScores.current.get(b.id) ?? 0) - (shuffleScores.current.get(a.id) ?? 0)
    );
    return arr;
  }, [rawClusters, shuffleKey, activeCategory, filter, localDisabledPublisherIds, globalDisabledPublisherIds]);

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    setFailedSources([]);
    try {
      const result = await refreshFeed();
      if (result.failed_sources.length > 0) {
        setFailedSources(result.failed_sources);
      }
      await queryClient.invalidateQueries({ queryKey: clusterKeys.all() });
      await refetch();
    } catch (e) { console.error("refresh failed:", e); }
    finally { setShuffleKey(k => k + 1); setRefreshing(false); }
  }, [queryClient, refetch]);

  const { containerRef, pullDistance, refreshing: pullRefreshing, progress } = usePullToRefresh(
    handleRefresh, !isLoading && !refreshing && rawClusters.length > 0,
  );
  const isRefreshing = refreshing || pullRefreshing;

  const [didAutoRefresh, setDidAutoRefresh] = useState(false);
  useEffect(() => {
    if (!isLoading && rawClusters.length === 0 && !didAutoRefresh) {
      setDidAutoRefresh(true);
      handleRefresh();
    }
  }, [isLoading, rawClusters.length, didAutoRefresh, handleRefresh]);

  if (isLoading || (isRefreshing && rawClusters.length === 0))
    return (
      <div className="feed-list">
        <CardSkeleton delay="0s" showImage />
        {[...Array(2)].map((_, i) => <CardSkeleton key={i} delay={`${(i + 1) * 0.08}s`} showImage />)}
      </div>
    );

  if (isError)
    return (
      <div className="empty-state">
        <div className="empty-icon">
          <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="var(--color-label-tertiary)" strokeWidth="1.5">
            <circle cx="12" cy="12" r="10" /><path d="M12 8v4M12 16h.01" strokeLinecap="round" />
          </svg>
        </div>
        <p className="empty-title">{t(lang, "loadError")}</p>
        <p className="empty-sub">{t(lang, "loadErrorSub")}</p>
        <button onClick={() => refetch()} className="primary-btn">{t(lang, "tryAgain")}</button>
      </div>
    );


  if (clusters.length === 0)
    return (
      <div className="empty-state">
        <div className="empty-icon">
          <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="var(--color-label-tertiary)" strokeWidth="1.5">
            <rect x="3" y="3" width="7" height="7" rx="2" /><rect x="14" y="3" width="7" height="18" rx="2" /><rect x="3" y="14" width="7" height="7" rx="2" />
          </svg>
        </div>
        <p className="empty-title">{t(lang, "noStories")}</p>
        <p className="empty-sub">{t(lang, "fetchingNews")}</p>
        <div className="spinner" />
      </div>
    );

  return (
    <div ref={containerRef} className="feed-scroll">
      {/* Pull indicator */}
      <div className="ptr-area" style={{ height: pullDistance > 0 ? pullDistance : isRefreshing ? 48 : 0 }}>
        <div className="ptr-spinner" style={{
          transform: isRefreshing ? "scale(1)" : `scale(${progress}) rotate(${progress * 360}deg)`,
          opacity: isRefreshing ? 1 : progress,
        }}>
          <svg className={isRefreshing ? "animate-spin" : ""} width="22" height="22" viewBox="0 0 20 20" fill="none"
            stroke="var(--color-accent)" strokeWidth="2" strokeLinecap="round">
            <path d="M2 10a8 8 0 0 1 14-5.3M18 10a8 8 0 0 1-14 5.3" />
            <path d="M16.5 2v3.5H13M3.5 18v-3.5H7" />
          </svg>
        </div>
      </div>

      {/* Failed sources banner */}
      {failedSources.length > 0 && (
        <div className="failed-sources-banner">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
            <circle cx="12" cy="12" r="10" /><path d="M12 8v4M12 16h.01" />
          </svg>
          {t(lang, "sourcesFailed").replace("{n}", String(failedSources.length))}
          <button className="banner-dismiss" onClick={() => setFailedSources([])}>×</button>
        </div>
      )}

      {/* Category filter pills */}
      {(
        <div className="category-pills">
          {ALL_CATEGORIES.map(cat => (
            <button
              key={cat}
              className={`category-pill ${activeCategory === cat ? "active" : ""}`}
              onClick={() => setActiveCategory(cat)}
            >
              {t(lang, CAT_I18N[cat])}
            </button>
          ))}
        </div>
      )}

      <div className="feed-list">
        {clusters.map((c, i) => (
          <StoryCard
            key={c.id}
            cluster={c}
            onPress={onSelectCluster}
            animationDelay={`${Math.min(i * 0.05, 0.3)}s`}
          />
        ))}
      </div>

    </div>
  );
}

// ── Publisher Source Rows ───────────────────────────────────────────────────

function SourceRow({
  publisher,
  action,
  onAction,
  isLast,
  dimmed = false,
}: {
  publisher: import("@/types").Publisher;
  action: "remove" | "add" | "delete";
  onAction: () => void;
  isLast: boolean;
  dimmed?: boolean;
}) {
  const dotColor = (BIAS_COLORS as Record<string, string>)[publisher.bias_category] ?? "#8E8E93";
  return (
    <div
      className="settings-row"
      style={{ borderBottom: isLast ? "none" : "0.5px solid var(--color-separator)", opacity: dimmed ? 0.5 : 1 }}
    >
      <div className="publisher-row-info">
        <span className="publisher-dot" style={{ background: dotColor }} />
        <span className="settings-row-label">{publisher.name}</span>
      </div>
      <button
        className={action === "add" ? "source-add-btn" : "source-remove-btn"}
        onClick={onAction}
        aria-label={`${action} ${publisher.name}`}
      >
        {action === "add" ? "+" : "−"}
      </button>
    </div>
  );
}

function SourcesSection({
  label,
  publishers,
  isEnabled,
  onToggle,
  onDelete,
}: {
  label: string;
  publishers: import("@/types").Publisher[];
  isEnabled: (id: string) => boolean;
  onToggle: (id: string) => void;
  onDelete?: (id: string) => void;
}) {
  const sorted = [...publishers].sort((a, b) => a.name.localeCompare(b.name));
  return (<>
    <p className="settings-label" style={{ marginTop: 28 }}>{label}</p>
    {sorted.length > 0 && (
      <div className="settings-group">
        {sorted.map((p, i) => {
          const enabled = isEnabled(p.id);
          return (
            <SourceRow
              key={p.id}
              publisher={p}
              action={onDelete ? "delete" : (enabled ? "remove" : "add")}
              onAction={() => onDelete ? onDelete(p.id) : onToggle(p.id)}
              isLast={i === sorted.length - 1}
              dimmed={!onDelete && !enabled}
            />
          );
        })}
      </div>
    )}
  </>);
}

// ── Add Source Form ─────────────────────────────────────────────────────────

function AddSourceForm({ isGlobal, onAdded }: { isGlobal: boolean; onAdded: () => void }) {
  const lang = useAppStore(s => s.language);
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const trimUrl = url.trim();
    if (!trimUrl) return;
    setLoading(true);
    setError(null);
    try {
      await addCustomPublisher(trimUrl, name.trim(), isGlobal);
      setUrl("");
      setName("");
      onAdded();
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  return (
    <form onSubmit={handleSubmit} className="add-source-form">
      <input
        className="add-source-input"
        type="text"
        placeholder={t(lang, "addSourceUrl")}
        value={url}
        onChange={e => setUrl(e.target.value)}
        disabled={loading}
        required
      />
      <input
        className="add-source-input"
        type="text"
        placeholder={t(lang, "addSourceName")}
        value={name}
        onChange={e => setName(e.target.value)}
        disabled={loading}
      />
      {error && <p className="add-source-error">{error}</p>}
      <button type="submit" className="add-source-btn" disabled={loading || !url.trim()}>
        {loading ? t(lang, "addingSource") : t(lang, "addSource")}
      </button>
    </form>
  );
}

// ── Settings Screen ─────────────────────────────────────────────────────────

export function SettingsScreen() {
  const { theme, setTheme, language, setLanguage, toggleLocalPublisher, toggleGlobalPublisher,
    isLocalPublisherEnabled, isGlobalPublisherEnabled } = useAppStore();
  const queryClient = useQueryClient();
  const { data: publishers = [] } = usePublishers();

  const localPublishers = publishers.filter(p => !p.is_global).sort((a, b) => a.name.localeCompare(b.name));
  const globalPublishers = publishers.filter(p => p.is_global).sort((a, b) => a.name.localeCompare(b.name));

  const invalidatePublishers = () => queryClient.invalidateQueries({ queryKey: ["publishers"] });

  const handleDeleteCustom = async (id: string) => {
    try {
      await removeCustomPublisher(id);
      invalidatePublishers();
    } catch (e) {
      console.error("Failed to remove publisher:", e);
    }
  };

  const themeLabels: Record<string, string> = {
    system: t(language, "system"),
    light: t(language, "light"),
    dark: t(language, "dark"),
  };

  return (
    <div className="settings-page animate-fade-up">
      {/* Appearance */}
      <p className="settings-label">{t(language, "appearance")}</p>
      <div className="segmented-control">
        {(["system", "light", "dark"] as const).map(v => (
          <button key={v} data-active={theme === v} onClick={() => setTheme(v)}>
            {themeLabels[v]}
          </button>
        ))}
      </div>

      {/* Language */}
      <p className="settings-label" style={{ marginTop: 28 }}>{t(language, "feedLanguage")}</p>
      <div className="settings-group">
        {([{ v: "en" as const, l: "English" }, { v: "mt" as const, l: "Malti" }]).map((opt, i) => (
          <button key={opt.v} className="settings-row" onClick={() => setLanguage(opt.v)}
            style={{ borderBottom: i === 0 ? "0.5px solid var(--color-separator)" : "none" }}>
            <span className="settings-row-label">{opt.l}</span>
            {language === opt.v && (
              <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
                <circle cx="10" cy="10" r="10" fill="var(--color-accent)" />
                <path d="M6 10l2.5 2.5L14 7.5" stroke="white" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            )}
          </button>
        ))}
      </div>

      {/* Malta Sources */}
      <SourcesSection
        label={t(language, "sourcesLocal")}
        publishers={localPublishers}
        isEnabled={isLocalPublisherEnabled}
        onToggle={toggleLocalPublisher}
      />

      {/* International / Global Sources */}
      <p className="settings-label" style={{ marginTop: 28 }}>{t(language, "sourcesGlobal")}</p>
      {globalPublishers.length > 0 && (
        <div className="settings-group">
          {globalPublishers.map((p, i) => (
            <SourceRow
              key={p.id}
              publisher={p}
              action="delete"
              onAction={() => handleDeleteCustom(p.id)}
              isLast={i === globalPublishers.length - 1}
            />
          ))}
        </div>
      )}
      <AddSourceForm isGlobal={true} onAdded={invalidatePublishers} />

      {/* Custom Malta Sources (user-added local) */}
      <p className="settings-label" style={{ marginTop: 28 }}>{t(language, "addMaltaSource")}</p>
      <AddSourceForm isGlobal={false} onAdded={invalidatePublishers} />

      {/* About */}
      <div className="settings-about">
        <div className="app-icon lg"><span>H</span></div>
        <p className="settings-app-name">Ħabbar</p>
        <p className="settings-version">v0.1.0</p>
      </div>
    </div>
  );
}
