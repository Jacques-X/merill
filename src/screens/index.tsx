import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { formatDistanceToNow } from "date-fns";
import { invoke } from "@tauri-apps/api/core";
import { useClusters, usePublishers, refreshFeed, addCustomPublisher, removeCustomPublisher, splitCluster, forceRecluster, wipeAllData, clusterKeys } from "@/api/clusters";
import { StoryCard } from "@/components/StoryCard/StoryCard";
import { BiasBar } from "@/components/BiasBar/BiasBar";
import { computeBiasCoverage } from "@/utils/bias";
import { BIAS_COLORS, LOCAL_BIAS_OPTIONS, GLOBAL_BIAS_OPTIONS } from "@/utils/constants";
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
  const refreshingRef = useRef(false);
  const pullDistanceRef = useRef(0);
  const THRESHOLD = 80;

  useEffect(() => {
    const el = containerRef.current;
    if (!el || !enabled) return;

    const onTouchStart = (e: TouchEvent) => {
      if (el.scrollTop <= 0 && !refreshingRef.current) {
        startY.current = e.touches[0].clientY;
        isPulling.current = true;
      }
    };
    const onTouchMove = (e: TouchEvent) => {
      if (!isPulling.current) return;
      const dy = e.touches[0].clientY - startY.current;
      if (dy > 0) {
        e.preventDefault();
        const d = Math.min(dy * 0.5, 120);
        pullDistanceRef.current = d;
        setPullDistance(d);
      } else {
        isPulling.current = false;
        pullDistanceRef.current = 0;
        setPullDistance(0);
      }
    };
    const onTouchEnd = async () => {
      if (!isPulling.current) return;
      isPulling.current = false;
      if (pullDistanceRef.current >= THRESHOLD) {
        refreshingRef.current = true;
        setRefreshing(true);
        setPullDistance(THRESHOLD);
        pullDistanceRef.current = THRESHOLD;
        try { await onRefresh(); } finally {
          refreshingRef.current = false;
          setRefreshing(false);
          setPullDistance(0);
          pullDistanceRef.current = 0;
        }
      } else {
        setPullDistance(0);
        pullDistanceRef.current = 0;
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
  }, [enabled, onRefresh]); // pullDistance removed — use ref inside handlers

  const progress = Math.min(pullDistance / THRESHOLD, 1);
  return { containerRef, pullDistance, refreshing, progress };
}

// ── Swipe-to-Dismiss wrapper ────────────────────────────────────────────────

function SwipeToDismiss({ children, onDismiss }: { children: React.ReactNode; onDismiss: () => void }) {
  const [offsetX, setOffsetX] = useState(0);
  const [dismissed, setDismissed] = useState(false);
  const startX = useRef(0);
  const startY = useRef(0);
  const tracking = useRef(false);
  const THRESHOLD = 110;

  const handleTouchStart = (e: React.TouchEvent) => {
    startX.current = e.touches[0].clientX;
    startY.current = e.touches[0].clientY;
    tracking.current = false;
  };
  const handleTouchMove = (e: React.TouchEvent) => {
    const dx = e.touches[0].clientX - startX.current;
    const dy = Math.abs(e.touches[0].clientY - startY.current);
    // Only track if clearly horizontal
    if (!tracking.current && Math.abs(dx) > 8 && dy < Math.abs(dx)) tracking.current = true;
    if (!tracking.current) return;
    if (dx < 0) setOffsetX(Math.max(dx, -180));
  };
  const handleTouchEnd = () => {
    if (!tracking.current) return;
    if (offsetX <= -THRESHOLD) {
      setDismissed(true);
      setTimeout(onDismiss, 280);
    } else {
      setOffsetX(0);
    }
    tracking.current = false;
  };

  const opacity = dismissed ? 0 : Math.max(0, 1 + offsetX / 180);
  const scale   = dismissed ? 0.88 : Math.max(0.88, 1 + offsetX / 900);

  return (
    <div
      onTouchStart={handleTouchStart}
      onTouchMove={handleTouchMove}
      onTouchEnd={handleTouchEnd}
      style={{
        transform: `translateX(${dismissed ? -320 : offsetX}px) scale(${scale})`,
        opacity,
        transition: (offsetX === 0 || dismissed) ? "transform 0.3s cubic-bezier(0.4,0,0.2,1), opacity 0.3s" : "none",
        transformOrigin: "center left",
      }}
    >
      {children}
    </div>
  );
}

// ── Swipe-to-remove row (reveals red action button on left swipe) ───────────

function SwipeRow({ children, onAction, label }: {
  children: React.ReactNode;
  onAction: () => void;
  label: string;
}) {
  const [offsetX, setOffsetX] = useState(0);
  const startX = useRef(0);
  const startY = useRef(0);
  const tracking = useRef(false);
  const PEEK = 72;
  const THRESHOLD = 48;

  const handleTouchStart = (e: React.TouchEvent) => {
    startX.current = e.touches[0].clientX;
    startY.current = e.touches[0].clientY;
    tracking.current = false;
  };
  const handleTouchMove = (e: React.TouchEvent) => {
    const dx = e.touches[0].clientX - startX.current;
    const dy = Math.abs(e.touches[0].clientY - startY.current);
    if (!tracking.current && Math.abs(dx) > 8 && dy < Math.abs(dx)) tracking.current = true;
    if (!tracking.current) return;
    if (dx < 0) setOffsetX(Math.max(dx, -PEEK));
    else if (offsetX < 0) setOffsetX(Math.min(0, offsetX + (dx > 0 ? dx * 0.5 : 0)));
  };
  const handleTouchEnd = () => {
    if (!tracking.current) { tracking.current = false; return; }
    tracking.current = false;
    setOffsetX(offsetX <= -THRESHOLD ? -PEEK : 0);
  };

  return (
    <div className="swipe-row">
      <button className="swipe-row-action" onClick={onAction} tabIndex={-1}>
        <span>{label}</span>
      </button>
      <div
        onTouchStart={handleTouchStart}
        onTouchMove={handleTouchMove}
        onTouchEnd={handleTouchEnd}
        style={{
          transform: `translateX(${offsetX}px)`,
          transition: tracking.current ? "none" : "transform 0.22s ease",
          background: "var(--color-bg)",
        }}
      >
        {children}
      </div>
    </div>
  );
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
    // Split on sentence boundaries only when followed by whitespace + capital letter,
    // avoiding false splits on abbreviations like "Dr.", "U.S.", initials, etc.
    const sentences = chunk.split(/(?<=[.!?])\s+(?=[A-Z])/).map(s => s.trim()).filter(Boolean);
    for (const s of sentences) {
      const trimmed = s.trim();
      if (trimmed.length > 25) allSentences.push(trimmed);
    }
  }

  const getWords = (s: string) =>
    new Set(s.toLowerCase().replace(/[^a-z\s]/g, "").split(/\s+/).filter(w => w.length > 3));
  const picked: string[] = [];
  const pickedWords: Set<string>[] = []; // cache word sets so getWords isn't called O(n²)
  for (const sent of allSentences) {
    const sentWords = getWords(sent);
    if (sentWords.size < 2) continue;
    const isDup = pickedWords.some(ew => {
      const shared = [...sentWords].filter(w => ew.has(w)).length;
      const smaller = Math.min(sentWords.size, ew.size);
      return smaller > 0 && shared / smaller > 0.5;
    });
    if (!isDup) {
      picked.push(sent);
      pickedWords.push(sentWords);
    }
    if (picked.length >= 5) break;
  }

  return picked.join(" ").slice(0, 500);
}

// ── HTML entity decoding ────────────────────────────────────────────────────

const HTML_ENTITIES: Record<string, string> = {
  "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'",
  "&nbsp;": " ", "&ndash;": "–", "&mdash;": "—", "&lsquo;": "\u2018",
  "&rsquo;": "\u2019", "&ldquo;": "\u201C", "&rdquo;": "\u201D",
  "&hellip;": "…", "&copy;": "©", "&reg;": "®", "&trade;": "™",
};

function decodeHTMLEntities(text: string): string {
  return text.replace(/&[^;]+;/g, match => HTML_ENTITIES[match] ?? match);
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
  const biasOverrides = useAppStore(s => s.publisherBiasOverrides);
  const readerFontSize = useAppStore(s => s.readerFontSize);
  const setReaderFontSize = useAppStore(s => s.setReaderFontSize);
  const [selectedArticle, setSelectedArticle] = useState<import("@/types").Article | null>(null);
  const [articleBody, setArticleBody] = useState<string>("");
  const [loadingBody, setLoadingBody] = useState(false);
  const [imgError, setImgError] = useState(false);
  const [logoErrors, setLogoErrors] = useState<Set<string>>(new Set());
  const coverage = computeBiasCoverage(cluster.articles, biasOverrides);
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
      const promises = cluster.articles.map((a) =>
        invoke<{ body_text: string; image_url: string }>("fetch_article_body", {
          articleId: a.id,
          url: a.original_url,
        }).then(r => ({ id: a.id, text: r.body_text }))
          .catch(() => ({ id: a.id, text: "" }))
      );
      const settled = await Promise.allSettled(promises);
      const all = settled.map(r => r.status === "fulfilled" ? r.value : { id: "", text: "" }).filter(r => r.id);
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
    const paragraphs = articleBody ? articleBody.split("\n\n").filter(Boolean).map(decodeHTMLEntities) : [];
    const domain = a.original_url.replace(/^https?:\/\//, "").split("/")[0];
    const wordCount = articleBody.split(/\s+/).filter(Boolean).length;
    const readingMins = Math.max(1, Math.round(wordCount / 200));

    return (
      <SwipeToDismiss onDismiss={() => setSelectedArticle(null)}>
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
              backgroundColor: BIAS_COLORS[biasOverrides[a.publisher_id] ?? a.publisher.bias_category] ?? "#8E8E93",
            }}>
              {a.publisher.logo_url && !logoErrors.has(a.id) ? (
                <img src={a.publisher.logo_url} alt={a.publisher.name}
                  onError={() => setLogoErrors(s => new Set(s).add(a.id))} />
              ) : (
                <span>{a.publisher.name.slice(0, 2).toUpperCase()}</span>
              )}
            </div>
            <div style={{ flex: 1 }}>
              <p className="detail-pub-name">{a.publisher.name}</p>
              <p className="detail-pub-time">
                {formatDistanceToNow(new Date(a.published_at), { addSuffix: true })}
                {paragraphs.length > 0 && (
                  <span className="reading-time"> · ~{readingMins} {t(lang, "minRead")}</span>
                )}
              </p>
            </div>
            {/* Font size controls */}
            <div className="font-controls">
              <button
                className="font-btn"
                onClick={() => setReaderFontSize(readerFontSize === "lg" ? "md" : "sm")}
                aria-label="Decrease font size"
              >A−</button>
              <button
                className="font-btn"
                onClick={() => setReaderFontSize(readerFontSize === "sm" ? "md" : "lg")}
                aria-label="Increase font size"
              >A+</button>
            </div>
          </div>

          <h2 className="detail-headline">{articleHeadline(a, lang)}</h2>

          {paragraphs.length > 0 ? (
            <div className={`detail-body font-${readerFontSize}`}>
              {paragraphs.map((p, i) => (<p key={i}>{p}</p>))}
            </div>
          ) : loadingBody ? (
            <div className="detail-loading">
              <div className="spinner" />
              <span>{t(lang, "loadingArticle")}</span>
            </div>
          ) : a.snippet ? (
            <div className={`detail-body font-${readerFontSize}`}>
              <p>{decodeHTMLEntities(a.snippet)}</p>
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
      </SwipeToDismiss>
    );
  }

  // ── Cluster Overview with combined summary
  const sortedByTime = useMemo(() =>
    [...cluster.articles].sort((a, b) => a.published_at.localeCompare(b.published_at)),
  [cluster.articles]);

  const plArticle = useMemo(() =>
    cluster.articles.find(a => (biasOverrides[a.publisher_id] ?? a.publisher.bias_category) === "party_owned_pl"),
  [cluster.articles, biasOverrides]);

  const pnArticle = useMemo(() =>
    cluster.articles.find(a => (biasOverrides[a.publisher_id] ?? a.publisher.bias_category) === "party_owned_pn"),
  [cluster.articles, biasOverrides]);

  const [compareOpen, setCompareOpen] = useState(false);

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

        {/* ── Both-sides compare ── */}
        {plArticle && pnArticle && (
          <div className="compare-section">
            <button className="compare-toggle" onClick={() => setCompareOpen(o => !o)}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M18 3H6a3 3 0 0 0-3 3v12a3 3 0 0 0 3 3h12a3 3 0 0 0 3-3V6a3 3 0 0 0-3-3z" />
                <line x1="12" y1="3" x2="12" y2="21" />
              </svg>
              {t(lang, "compareFraming")}
              <svg className={`compare-chevron ${compareOpen ? "open" : ""}`} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round">
                <path d="M6 9l6 6 6-6" />
              </svg>
            </button>
            {compareOpen && (
              <div className="compare-cols">
                <div className="compare-col pl">
                  <span className="compare-label" style={{ color: BIAS_COLORS.party_owned_pl }}>{t(lang, "labourSays")}</span>
                  <p className="compare-headline">{articleHeadline(plArticle, lang)}</p>
                  {plArticle.snippet && <p className="compare-snippet">{plArticle.snippet}</p>}
                </div>
                <div className="compare-divider" />
                <div className="compare-col pn">
                  <span className="compare-label" style={{ color: BIAS_COLORS.party_owned_pn }}>{t(lang, "nationalistSays")}</span>
                  <p className="compare-headline">{articleHeadline(pnArticle, lang)}</p>
                  {pnArticle.snippet && <p className="compare-snippet">{pnArticle.snippet}</p>}
                </div>
              </div>
            )}
          </div>
        )}

        <div className="source-headlines">
          {cluster.articles.map(a => (
            <SwipeRow
              key={a.id}
              label={t(lang, "splitFromCluster")}
              onAction={async () => {
                const headline = a.language === "en" ? a.original_headline : (a.translated_headline || a.original_headline);
                await splitCluster(a.id, headline, a.published_at).catch(() => {});
                // Parent will re-fetch on next query invalidation; for now just show the detail gone.
              }}
            >
              <button className="source-headline-row" onClick={() => openArticle(a)}>
                <div className="source-avatar sm" style={{
                  backgroundColor: BIAS_COLORS[biasOverrides[a.publisher_id] ?? a.publisher.bias_category] ?? "#8E8E93",
                }}>
                  {a.publisher.logo_url && !logoErrors.has(a.id) ? (
                    <img src={a.publisher.logo_url} alt={a.publisher.name}
                      onError={() => setLogoErrors(s => new Set(s).add(a.id))} />
                  ) : (
                    <span>{a.publisher.name.slice(0, 2).toUpperCase()}</span>
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
            </SwipeRow>
          ))}
        </div>

        {/* ── Story timeline ── */}
        {sortedByTime.length > 1 && (
          <div className="timeline-section">
            <p className="settings-label">{t(lang, "storyTimeline")}</p>
            <div className="timeline-list">
              {sortedByTime.map((a, i) => (
                <div key={a.id} className="timeline-item">
                  <div className="timeline-track">
                    <div className="timeline-dot" style={{ background: BIAS_COLORS[biasOverrides[a.publisher_id] ?? a.publisher.bias_category] ?? "#8E8E93" }} />
                    {i < sortedByTime.length - 1 && <div className="timeline-line" />}
                  </div>
                  <div className="timeline-text">
                    <span className="timeline-pub">
                      {a.publisher.name}
                      {i === 0 && <span className="timeline-first"> · {t(lang, "brokeTheStory")}</span>}
                    </span>
                    <span className="timeline-time">{formatDistanceToNow(new Date(a.published_at), { addSuffix: true })}</span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
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

const INDEPENDENT_BIAS: BiasCategory[] = ["commercial_independent", "investigative_independent"];

function recomputeBlindspot(articles: StoryCluster["articles"], overrides: Record<string, BiasCategory>): boolean {
  if (!articles.length) return false;
  return !articles.some(a => {
    const cat = overrides[a.publisher_id] ?? a.publisher.bias_category;
    return INDEPENDENT_BIAS.includes(cat);
  });
}

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
  const biasOverrides = useAppStore(s => s.publisherBiasOverrides);
  const queryClient = useQueryClient();
  const { data, isLoading, isError, refetch } = useClusters();
  const [refreshing, setRefreshing] = useState(false);
  const [shuffleKey, setShuffleKey] = useState(0);
  const [activeCategory, setActiveCategory] = useState<"all" | Category>("all");
  const [failedSources, setFailedSources] = useState<string[]>([]);
  const [dismissedIds, setDismissedIds] = useState<Set<string>>(new Set());

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
      .map(c => {
        const articles = c.articles.filter(a =>
          (filter === "local" ? !a.publisher.is_global : a.publisher.is_global) &&
          !disabledPubs.includes(a.publisher_id)
        );
        return {
          ...c,
          articles,
          // Re-evaluate blindspot using user's bias overrides so the flag stays accurate.
          is_blindspot: articles.length ? recomputeBlindspot(articles, biasOverrides) : c.is_blindspot,
        };
      })
      .filter(c => c.articles.length > 0);

    // Apply category filter.
    if (activeCategory !== "all") {
      arr = arr.filter(c => c.articles.some(a => a.category === activeCategory));
    }

    arr.sort((a, b) =>
      (shuffleScores.current.get(b.id) ?? 0) - (shuffleScores.current.get(a.id) ?? 0)
    );
    return arr;
  }, [rawClusters, shuffleKey, activeCategory, filter, localDisabledPublisherIds, globalDisabledPublisherIds, biasOverrides]);

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

      {/* Debug refresh button — desktop only */}
      {typeof window !== "undefined" && !("ontouchstart" in window) && (
        <button
          onClick={handleRefresh}
          disabled={isRefreshing}
          style={{ margin: "8px 16px 0", padding: "6px 14px", borderRadius: 8, fontSize: 12,
            background: "var(--color-accent)", color: "#fff", border: "none", opacity: isRefreshing ? 0.5 : 1 }}
        >
          {isRefreshing ? t(lang, "refreshing") : t(lang, "refresh")}
        </button>
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
        {clusters.filter(c => !dismissedIds.has(c.id)).map((c, i) => (
          <SwipeToDismiss
            key={c.id}
            onDismiss={() => setDismissedIds(s => new Set(s).add(c.id))}
          >
            <StoryCard
              cluster={c}
              onPress={onSelectCluster}
              animationDelay={`${Math.min(i * 0.05, 0.3)}s`}
            />
          </SwipeToDismiss>
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
  articleCount,
}: {
  publisher: import("@/types").Publisher;
  action: "remove" | "add" | "delete";
  onAction: () => void;
  isLast: boolean;
  dimmed?: boolean;
  articleCount?: number;
}) {
  const lang = useAppStore(s => s.language);
  const biasOverrides = useAppStore(s => s.publisherBiasOverrides);
  const setPublisherBias = useAppStore(s => s.setPublisherBias);
  const defaultBias = publisher.is_global ? "centre" : publisher.bias_category;
  const effectiveBias = biasOverrides[publisher.id] ?? defaultBias;
  const dotColor = (BIAS_COLORS as Record<string, string>)[effectiveBias] ?? "#8E8E93";
  const biasOptions = publisher.is_global ? GLOBAL_BIAS_OPTIONS : LOCAL_BIAS_OPTIONS;
  return (
    <div
      className="settings-row"
      style={{ borderBottom: isLast ? "none" : "0.5px solid var(--color-separator)", opacity: dimmed ? 0.5 : 1 }}
    >
      <div className="publisher-row-info">
        <span className="publisher-dot" style={{ background: dotColor }} />
        <div className="publisher-name-bias">
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <span className="settings-row-label">{publisher.name}</span>
            {articleCount !== undefined && articleCount > 0 && (
              <span className="publisher-count">{articleCount} {t(lang, "articlesToday")}</span>
            )}
          </div>
          <select
            className="bias-select"
            value={effectiveBias}
            onChange={e => setPublisherBias(publisher.id, e.target.value as import("@/types").BiasCategory)}
            onClick={e => e.stopPropagation()}
          >
            {biasOptions.map(([value, label]) => (
              <option key={value} value={value}>{label}</option>
            ))}
          </select>
        </div>
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
  articleCounts = {},
}: {
  label: string;
  publishers: import("@/types").Publisher[];
  isEnabled: (id: string) => boolean;
  onToggle: (id: string) => void;
  onDelete?: (id: string) => void;
  articleCounts?: Record<string, number>;
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
              articleCount={articleCounts[p.id]}
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
  const [reclustering, setReclustering] = useState(false);
  const [wiping, setWiping] = useState(false);
  const [wipeConfirm, setWipeConfirm] = useState(false);
  const { data: publishers = [] } = usePublishers();

  const localPublishers = publishers.filter(p => !p.is_global).sort((a, b) => a.name.localeCompare(b.name));
  const globalPublishers = publishers.filter(p => p.is_global).sort((a, b) => a.name.localeCompare(b.name));

  // Compute article counts per publisher from the cached cluster data (no extra fetch needed).
  const articleCounts = useMemo(() => {
    const cached = queryClient.getQueryData<import("@/types").ClustersResponse>(clusterKeys.list({}));
    const counts: Record<string, number> = {};
    for (const cluster of cached?.clusters ?? []) {
      for (const article of cluster.articles) {
        counts[article.publisher_id] = (counts[article.publisher_id] ?? 0) + 1;
      }
    }
    return counts;
  }, [queryClient]);

  const invalidatePublishers = () => queryClient.invalidateQueries({ queryKey: ["publishers"] });
  const invalidateClusters = () => queryClient.invalidateQueries({ queryKey: clusterKeys.all() });

  const handleDeleteCustom = async (id: string) => {
    try {
      await removeCustomPublisher(id);
      invalidatePublishers();
      // Articles from this publisher are deleted from DB, so clusters must be re-fetched.
      invalidateClusters();
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

      {/* Local Sources */}
      <SourcesSection
        label={t(language, "sourcesLocal")}
        publishers={localPublishers}
        isEnabled={isLocalPublisherEnabled}
        onToggle={toggleLocalPublisher}
        articleCounts={articleCounts}
      />

      {/* Add Malta Source */}
      <p className="settings-label" style={{ marginTop: 28 }}>{t(language, "addMaltaSource")}</p>
      <AddSourceForm isGlobal={false} onAdded={invalidatePublishers} />

      {/* Global Sources */}
      {globalPublishers.length > 0 && (
        <>
          <p className="settings-label" style={{ marginTop: 28 }}>{t(language, "sourcesGlobal")}</p>
          <div className="settings-group">
            {globalPublishers.map((p, i) => (
              <SourceRow
                key={p.id}
                publisher={p}
                action="delete"
                onAction={() => handleDeleteCustom(p.id)}
                isLast={i === globalPublishers.length - 1}
                articleCount={articleCounts[p.id]}
              />
            ))}
          </div>
        </>
      )}

      {/* Add International Source */}
      <p className="settings-label" style={{ marginTop: 28 }}>{t(language, "addInternationalSource")}</p>
      <AddSourceForm isGlobal={true} onAdded={invalidatePublishers} />

      {/* Re-cluster */}
      <p className="settings-label" style={{ marginTop: 28 }}>{t(language, "forceRecluster")}</p>
      <button
        className="danger-btn"
        disabled={reclustering}
        onClick={async () => {
          setReclustering(true);
          try {
            await forceRecluster();
            queryClient.invalidateQueries({ queryKey: clusterKeys.all() });
          } catch (e) { console.error(e); }
          finally { setReclustering(false); }
        }}
      >
        {reclustering ? t(language, "reclustering") : t(language, "forceRecluster")}
      </button>

      {/* Wipe All Data */}
      <p className="settings-label" style={{ marginTop: 28 }}>{t(language, "wipeAllData")}</p>
      {wipeConfirm ? (
        <div className="settings-group">
          <p style={{ padding: "12px 16px", fontSize: 14, color: "var(--color-text-secondary)" }}>
            {t(language, "wipeAllDataConfirm")}
          </p>
          <div style={{ display: "flex", borderTop: "0.5px solid var(--color-separator)" }}>
            <button
              className="settings-row"
              style={{ flex: 1, justifyContent: "center", color: "var(--color-text-secondary)" }}
              onClick={() => setWipeConfirm(false)}
            >
              Cancel
            </button>
            <button
              className="settings-row"
              style={{ flex: 1, justifyContent: "center", color: "var(--color-destructive, #ff3b30)", borderLeft: "0.5px solid var(--color-separator)", fontWeight: 600 }}
              disabled={wiping}
              onClick={async () => {
                setWiping(true);
                try {
                  await wipeAllData();
                  queryClient.invalidateQueries({ queryKey: clusterKeys.all() });
                } catch (e) { console.error(e); }
                finally { setWiping(false); setWipeConfirm(false); }
              }}
            >
              {wiping ? t(language, "wipingData") : t(language, "wipeAllData")}
            </button>
          </div>
        </div>
      ) : (
        <button className="danger-btn" onClick={() => setWipeConfirm(true)}>
          {t(language, "wipeAllData")}
        </button>
      )}

      {/* About */}
      <div className="settings-about">
        <div className="app-icon lg"><img src="/app-icon.png" alt="Merill" /></div>
        <p className="settings-app-name">Merill</p>
        <p className="settings-version">v0.1.0</p>
      </div>
    </div>
  );
}
