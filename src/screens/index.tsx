import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { formatDistanceToNow } from "date-fns";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeft, Check, ChevronDown, ChevronRight, ExternalLink, Filter, Info, MoreHorizontal, Plus, Search, Settings2, Trash2, X } from "lucide-react";
import { useClusters, usePublishers, useSavedStories, refreshFeed, searchStories, saveStory, unsaveStory, getRefreshStatus, addCustomPublisher, removeCustomPublisher, splitCluster, forceRecluster, wipeAllData, clusterKeys } from "@/api/clusters";
import { StoryCard } from "@/components/StoryCard/StoryCard";
import { BiasBar } from "@/components/BiasBar/BiasBar";
import { computeBiasCoverage } from "@/utils/bias";
import { BIAS_COLORS, LOCAL_BIAS_OPTIONS, GLOBAL_BIAS_OPTIONS } from "@/utils/constants";
import { articleHeadline, clusterHeadline } from "@/utils/headline";
import { t, format } from "@/utils/i18n";
import { useAppStore } from "@/store/useAppStore";
import type { StoryCluster, Category, BiasCategory, Article } from "@/types";

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

// ── Extractive summary: merge first paragraphs from all sources into one ──

function combineSummary(bodyTexts: string[]): string {
  const texts = bodyTexts.filter(Boolean);
  if (texts.length === 0) return "";
  if (texts.length === 1) {
    const first = texts[0].split("\n\n").slice(0, 2).join(" ");
    return first.slice(0, 400);
  }

  const allSentences: string[] = [];
  for (const text of texts) {
    const chunk = text.split("\n\n").slice(0, 2).join(" ");
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

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function articlePreviewText(article: Article): string {
  return article.body_text || article.snippet || "";
}

const GROUPING_STOP_WORDS = new Set([
  "about", "after", "are", "before", "for", "from", "its", "with", "that", "this", "they", "their", "will",
  "would", "could", "should", "says", "said", "news", "malta", "maltese",
  "today", "new", "more", "over", "under", "people", "government", "police", "malti",
  "court", "minister", "local", "public", "general", "candidate", "candidates",
  "counterpart", "counterparts", "district", "districts", "election", "elections",
  "electoral", "vote", "votes", "voter", "voters", "voting", "campaign", "party", "leader",
  "meeting", "chapter", "future", "lovin", "newsbook", "times", "talk", "europe",
  "world", "cup", "trophy", "final", "league", "huma", "kandidat", "kandidati", "distrett", "distretti",
  "tazza", "dinja",
  "elezzjoni", "elezzjonijiet", "generali", "vot", "voti", "votazzjoni", "jivvota",
]);

const BIAS_I18N: Record<BiasCategory, import("@/utils/i18n").LangKey> = {
  state_owned: "biasState",
  party_owned_pl: "biasPl",
  party_owned_pn: "biasPn",
  church_owned: "biasChurch",
  commercial_independent: "biasIndependent",
  investigative_independent: "biasInvestigative",
  left: "biasLeft",
  centre: "biasCentre",
  right: "biasRight",
};

function groupingTokens(text: string): Set<string> {
  const tokens = text
    .toLowerCase()
    .split(/[^a-z0-9\u0100-\u017f]+/i)
    .filter(w => w.length >= 4 && !GROUPING_STOP_WORDS.has(w));
  return new Set(tokens);
}

function groupingEvidence(cluster: StoryCluster) {
  const headlines = cluster.articles.map(a => articleHeadline(a, "en") || a.original_headline);
  if (headlines.length <= 1) {
    return { level: "single" as const, shared: [] as string[] };
  }
  const base = groupingTokens(headlines[0]);
  const sharedCounts = new Map<string, number>();
  for (const headline of headlines.slice(1)) {
    for (const token of groupingTokens(headline)) {
      if (base.has(token)) sharedCounts.set(token, (sharedCounts.get(token) ?? 0) + 1);
    }
  }
  const shared = [...sharedCounts.entries()]
    .filter(([, count]) => count >= 1)
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, 4)
    .map(([token]) => token);
  const level = shared.length >= 3
    ? "strong"
    : shared.length >= 1
      ? "medium"
      : "low";
  return { level, shared };
}

// ── Story Detail Screen ─────────────────────────────────────────────────────

export function StoryDetailScreen({
  cluster,
  internalBackRef,
  onBack,
}: {
  cluster: StoryCluster;
  internalBackRef?: React.MutableRefObject<(() => void) | null>;
  onBack: () => void;
}) {
  const lang = useAppStore(s => s.language);
  const biasOverrides = useAppStore(s => s.publisherBiasOverrides);
  const readerFontSize = useAppStore(s => s.readerFontSize);
  const setReaderFontSize = useAppStore(s => s.setReaderFontSize);
  const readerLineSpacing = useAppStore(s => s.readerLineSpacing);
  const setReaderLineSpacing = useAppStore(s => s.setReaderLineSpacing);
  const readerTextMode = useAppStore(s => s.readerTextMode);
  const setReaderTextMode = useAppStore(s => s.setReaderTextMode);
  const queryClient = useQueryClient();
  const [selectedArticle, setSelectedArticle] = useState<Article | null>(null);
  const [articleBody, setArticleBody] = useState<string>("");
  const [localizedArticleBody, setLocalizedArticleBody] = useState<string>("");
  const [loadingBody, setLoadingBody] = useState(false);
  const [articleError, setArticleError] = useState<string | null>(null);
  const [detailActionError, setDetailActionError] = useState<string | null>(null);
  const [imgError, setImgError] = useState(false);
  const [logoErrors, setLogoErrors] = useState<Set<string>>(new Set());
  const coverage = computeBiasCoverage(cluster.articles, biasOverrides);
  const imageUrl = !imgError ? cluster.articles.find(a => a.image_url)?.image_url : undefined;

  const [summaries, setSummaries] = useState<Map<string, string>>(new Map());
  const [summaryLoading, setSummaryLoading] = useState(true);
  const [translatedSummary, setTranslatedSummary] = useState<string>("");
  const [timelineOpen, setTimelineOpen] = useState(false);
  const [advancedTermsOpen, setAdvancedTermsOpen] = useState(false);
  const [reviewOpen, setReviewOpen] = useState(false);
  const [groupMenuOpen, setGroupMenuOpen] = useState(false);
  const groupEvidence = useMemo(() => groupingEvidence(cluster), [cluster]);

  const sortedByTime = useMemo(() =>
    [...cluster.articles].sort((a, b) => a.published_at.localeCompare(b.published_at)),
  [cluster.articles]);

  const selectedParagraphs = useMemo(
    () => {
      const body = readerTextMode === "translated" && localizedArticleBody ? localizedArticleBody : articleBody;
      return body ? body.split("\n\n").filter(Boolean).map(decodeHTMLEntities) : [];
    },
    [articleBody, localizedArticleBody, readerTextMode],
  );
  const selectedDomain = selectedArticle?.original_url.replace(/^https?:\/\//, "").split("/")[0] ?? "";
  const readingMins = Math.max(1, Math.round(articleBody.split(/\s+/).filter(Boolean).length / 200));

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

    async function buildSummaryInputs() {
      const seeded = cluster.articles
        .map(a => ({ id: a.id, text: articlePreviewText(a) }))
        .filter(r => r.text);
      if (seeded.length > 0) {
        if (!cancelled) {
          setSummaries(new Map(seeded.map(r => [r.id, r.text])));
          setSummaryLoading(false);
        }
        return;
      }

      const fetched: { id: string; text: string }[] = [];
      for (const article of cluster.articles.slice(0, 2)) {
        try {
          const result = await invoke<{ body_text: string; image_url: string }>("fetch_article_body", {
            articleId: article.id,
            url: article.original_url,
          });
          if (result.body_text) fetched.push({ id: article.id, text: result.body_text });
        } catch {
          // Summary can fall back to no text; opening the article still reports errors.
        }
      }
      if (cancelled) return;
      const results = new Map<string, string>();
      for (const { id, text } of fetched) {
        if (text) results.set(id, text);
      }
      setSummaries(results);
      setSummaryLoading(false);
    }
    buildSummaryInputs();
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

  useEffect(() => {
    let cancelled = false;
    setLocalizedArticleBody("");
    if (!selectedArticle || !articleBody || readerTextMode === "original" || selectedArticle.language === lang) {
      return;
    }
    invoke<string>("translate_summary", { text: articleBody, to: lang })
      .then(value => { if (!cancelled) setLocalizedArticleBody(value); })
      .catch(() => undefined);
    return () => { cancelled = true; };
  }, [articleBody, lang, readerTextMode, selectedArticle]);

  const openArticle = useCallback(async (a: Article) => {
    setSelectedArticle(a);
    setArticleError(null);
    setLocalizedArticleBody("");
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
        setArticleError(errorMessage(e));
      } finally {
        setLoadingBody(false);
      }
    }
  }, [summaries]);

  const handleSplit = useCallback(async (article: Article) => {
    const headline = article.language === "en" ? article.original_headline : (article.translated_headline || article.original_headline);
    setDetailActionError(null);
    try {
      await splitCluster(article.id, headline, article.published_at);
      await queryClient.invalidateQueries({ queryKey: clusterKeys.all() });
    } catch (err) {
      setDetailActionError(t(lang, "splitClusterError"));
      console.error("split cluster failed:", err);
    }
  }, [lang, queryClient]);

  // ── Article Reader View
  if (selectedArticle) {
    const a = selectedArticle;

    return (
      <SwipeToDismiss onDismiss={() => setSelectedArticle(null)}>
      <div className="animate-fade-up detail-scroll">
        <header className="overlay-topbar">
          <button className="overlay-icon-btn" onClick={() => setSelectedArticle(null)} aria-label={t(lang, "back")}><ArrowLeft size={21} /></button>
          <div className="overlay-title">
            <span>{a.publisher.name}</span>
            <small>{formatDistanceToNow(new Date(a.published_at), { addSuffix: true })}</small>
          </div>
          <a className="overlay-icon-btn" href={a.original_url} target="_blank" rel="noopener noreferrer" aria-label={`${t(lang, "readOn")} ${selectedDomain}`}><ExternalLink size={18} /></a>
        </header>
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
              {a.publisher.logo_url && !logoErrors.has(a.publisher_id) ? (
                <img src={a.publisher.logo_url} alt={a.publisher.name}
                  onError={() => setLogoErrors(s => new Set(s).add(a.publisher_id))} />
              ) : (
                <span>{a.publisher.name.slice(0, 2).toUpperCase()}</span>
              )}
            </div>
            <div style={{ flex: 1 }}>
              <p className="detail-pub-name">{a.publisher.name}</p>
              {selectedParagraphs.length > 0 && <p className="detail-pub-time">~{readingMins} {t(lang, "minRead")}</p>}
            </div>
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

          <div className="reader-preferences">
            <div className="segmented-control">
              <button data-active={readerTextMode === "translated"} onClick={() => setReaderTextMode("translated")}>{t(lang, "translatedText")}</button>
              <button data-active={readerTextMode === "original"} onClick={() => setReaderTextMode("original")}>{t(lang, "originalText")}</button>
            </div>
            <div className="segmented-control">
              {(["compact", "comfortable", "relaxed"] as const).map(spacing => (
                <button key={spacing} data-active={readerLineSpacing === spacing} onClick={() => setReaderLineSpacing(spacing)}>
                  {t(lang, spacing === "compact" ? "spacingCompact" : spacing === "comfortable" ? "spacingComfortable" : "spacingRelaxed")}
                </button>
              ))}
            </div>
          </div>

          <h2 className="detail-headline">{articleHeadline(a, lang)}</h2>

          {articleError && (
            <div className="inline-error">{t(lang, "articleLoadError")}</div>
          )}

          {selectedParagraphs.length > 0 ? (
            <div className={`detail-body font-${readerFontSize} spacing-${readerLineSpacing}`}>
              {selectedParagraphs.map((p, i) => (<p key={i}>{p}</p>))}
            </div>
          ) : loadingBody ? (
            <div className="detail-loading">
              <div className="spinner" />
              <span>{t(lang, "loadingArticle")}</span>
            </div>
          ) : a.snippet ? (
            <div className={`detail-body font-${readerFontSize} spacing-${readerLineSpacing}`}>
              <p>{decodeHTMLEntities(a.snippet)}</p>
            </div>
          ) : (
            <div className="detail-empty-body">{t(lang, "noBodyText")}</div>
          )}

          <a href={a.original_url} target="_blank" rel="noopener noreferrer" className="read-original-btn">
            <ExternalLink size={16} />
            {t(lang, "readOn")} {selectedDomain}
          </a>

          <button onClick={() => setSelectedArticle(null)} className="back-to-sources">
            {format(t(lang, "viewAllSources"), { n: String(cluster.articles.length) })}
          </button>
        </div>
      </div>
      </SwipeToDismiss>
    );
  }

  return (
    <div className="animate-fade-up detail-scroll">
      <header className="overlay-topbar">
        <button className="overlay-icon-btn" onClick={onBack} aria-label={t(lang, "back")}><ArrowLeft size={21} /></button>
        <span className="overlay-title single">{t(lang, "storyGroup")}</span>
        <div className="detail-menu-wrap">
          <button className="overlay-icon-btn" onClick={() => setGroupMenuOpen(open => !open)} aria-label={t(lang, "moreActions")} aria-expanded={groupMenuOpen}><MoreHorizontal size={21} /></button>
          {groupMenuOpen && (
            <button className="detail-menu-popover" onClick={() => { setReviewOpen(true); setGroupMenuOpen(false); }}>
              <Settings2 size={16} /> {t(lang, "reviewGrouping")}
            </button>
          )}
        </div>
      </header>
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

        {cluster.blindspot_explanation.missing_independent_coverage && (
          <div className="blindspot-explanation">
            <strong>{t(lang, "whyBlindspot")}</strong>
            <p>
              {format(t(lang, "blindspotReason"), {
                n: String(cluster.blindspot_explanation.publisher_count),
                categories: cluster.blindspot_explanation.covered_categories.map(category => t(lang, BIAS_I18N[category])).join(", "),
              })}
            </p>
          </div>
        )}

        {detailActionError && (
          <div className="inline-error">{detailActionError}</div>
        )}

        <div className="section-heading">
          <span>{t(lang, "perspectives")}</span>
          <small>{t(lang, "swipeToCompare")}</small>
        </div>
        <button
          className="perspective-advanced-trigger"
          onClick={() => setAdvancedTermsOpen(open => !open)}
          aria-expanded={advancedTermsOpen}
        >
          {t(lang, "advanced")}
          <ChevronDown size={15} className={advancedTermsOpen ? "open" : ""} />
        </button>
        <div className="perspective-groups">
          {cluster.perspective_groups.map(group => (
            <section key={group.bias_category} className="perspective-group">
              <div className="perspective-group-header">
                <span className="publisher-dot" style={{ background: BIAS_COLORS[group.bias_category] }} />
                <strong>{t(lang, BIAS_I18N[group.bias_category])}</strong>
                <small>{group.articles.length} {t(lang, group.articles.length === 1 ? "source" : "sources")}</small>
              </div>
              {advancedTermsOpen && group.common_terms.length > 0 && <p className="term-line"><b>{t(lang, "sharedTerms")}:</b> {group.common_terms.join(", ")}</p>}
              {advancedTermsOpen && group.distinct_terms.length > 0 && <p className="term-line"><b>{t(lang, "distinctTerms")}:</b> {group.distinct_terms.join(", ")}</p>}
              {group.articles.map(item => {
                const article = cluster.articles.find(candidate => candidate.id === item.article_id);
                if (!article) return null;
                return (
                  <button key={item.article_id} className="comparison-row" onClick={() => openArticle(article)}>
                    <span><strong>{item.publisher_name}</strong><small>{formatDistanceToNow(new Date(item.published_at), { addSuffix: true })}</small></span>
                    <p>{articleHeadline(article, lang)}</p>
                    <ChevronRight size={16} />
                  </button>
                );
              })}
            </section>
          ))}
        </div>

        {sortedByTime.length > 1 && (
          <div className="disclosure-panel">
            <button className="disclosure-trigger" onClick={() => setTimelineOpen(open => !open)} aria-expanded={timelineOpen}>
              {t(lang, "storyTimeline")} <ChevronDown size={17} className={timelineOpen ? "open" : ""} />
            </button>
            {timelineOpen && <div className="timeline-list">
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
                    <span className="timeline-headline">{articleHeadline(a, lang)}</span>
                  </div>
                </div>
              ))}
            </div>}
          </div>
        )}
      </div>
      {reviewOpen && (
        <div className="sheet-backdrop" onClick={() => setReviewOpen(false)}>
          <section className="bottom-sheet" onClick={event => event.stopPropagation()} aria-modal="true" role="dialog" aria-label={t(lang, "reviewGrouping")}>
            <div className="sheet-header">
              <div><h3>{t(lang, "reviewGrouping")}</h3><p>{t(lang, "reviewGroupingSub")}</p></div>
              <button className="overlay-icon-btn" onClick={() => setReviewOpen(false)} aria-label={t(lang, "close")}><X size={19} /></button>
            </div>
            <div className={`cluster-confidence ${groupEvidence.level}`}>
              <span>{t(lang, "groupingConfidence")}</span>
              <strong>{t(lang, GROUPING_I18N[groupEvidence.level])}</strong>
              {groupEvidence.shared.length > 0 && <small>{groupEvidence.shared.join(", ")}</small>}
            </div>
            <div className="review-source-list">
              {cluster.articles.map(article => (
                <div key={article.id} className="review-source-row">
                  <div><strong>{article.publisher.name}</strong><span>{articleHeadline(article, lang)}</span></div>
                  <button onClick={() => handleSplit(article)} aria-label={`${t(lang, "splitFromCluster")}: ${article.publisher.name}`}><Trash2 size={16} /></button>
                </div>
              ))}
            </div>
          </section>
        </div>
      )}
    </div>
  );
}

// ── Skeletons ────────────────────────────────────────────────────────────────

function CardSkeleton({ delay = "0s" }: { delay?: string }) {
  return (
    <div className="story-card skeleton-card animate-fade-up" style={{ animationDelay: delay }}>
      <div className="skeleton" style={{ width: "100%", height: 200, borderRadius: 0 }} />
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
const GROUPING_I18N: Record<ReturnType<typeof groupingEvidence>["level"], import("@/utils/i18n").LangKey> = {
  single: "groupingSingle",
  low: "groupingLow",
  medium: "groupingMedium",
  strong: "groupingStrong",
};

export type FeedFilter = "local" | "global";
type FeedSort = "balanced" | "latest" | "covered" | "blindspots";
type FeedRootView = "feed" | "blindspots" | "saved";

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
  onFilterChange,
  rootView = "feed",
  onOpenSettings,
}: {
  onSelectCluster: (c: StoryCluster) => void;
  filter?: FeedFilter;
  onFilterChange: (filter: FeedFilter) => void;
  rootView?: FeedRootView;
  onOpenSettings: () => void;
}) {
  const lang = useAppStore(s => s.language);
  const localDisabledPublisherIds = useAppStore(s => s.localDisabledPublisherIds);
  const globalDisabledPublisherIds = useAppStore(s => s.globalDisabledPublisherIds);
  const biasOverrides = useAppStore(s => s.publisherBiasOverrides);
  const savedStoryKeys = useAppStore(s => s.savedStoryKeys);
  const savedStoryKeySet = useMemo(() => new Set(savedStoryKeys), [savedStoryKeys]);
  const setStorySaved = useAppStore(s => s.setStorySaved);
  const replaceSavedStoryKeys = useAppStore(s => s.replaceSavedStoryKeys);
  const legacySavedClusterIds = useAppStore(s => s.legacySavedClusterIds);
  const clearLegacySavedClusterIds = useAppStore(s => s.clearLegacySavedClusterIds);
  const queryClient = useQueryClient();
  const { data, isLoading, isError, refetch } = useClusters();
  const { data: savedData, refetch: refetchSaved } = useSavedStories();
  const { data: publishers = [], isLoading: publishersLoading } = usePublishers();
  const [refreshing, setRefreshing] = useState(false);
  const [activeCategory, setActiveCategory] = useState<"all" | Category>("all");
  const [feedSort, setFeedSort] = useState<FeedSort>("balanced");
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [failedSources, setFailedSources] = useState<string[]>([]);
  const [refreshError, setRefreshError] = useState<string | null>(null);
  const [dismissedIds, setDismissedIds] = useState<Set<string>>(new Set());
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResult, setSearchResult] = useState<StoryCluster[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [lastRefreshLabel, setLastRefreshLabel] = useState<string>("");
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);

  useEffect(() => {
    if (savedData) {
      replaceSavedStoryKeys(savedData.clusters.map(cluster => cluster.story_key));
    }
  }, [replaceSavedStoryKeys, savedData]);

  useEffect(() => {
    if (!data || legacySavedClusterIds.length === 0) return;
    const legacyIds = new Set(legacySavedClusterIds);
    const clustersToMigrate = data.clusters.filter(cluster => legacyIds.has(cluster.id));
    Promise.all(
      clustersToMigrate.map(cluster =>
        saveStory(cluster.story_key, cluster.articles.map(article => article.id))
      ),
    )
      .then(async () => {
        clearLegacySavedClusterIds();
        await refetchSaved();
      })
      .catch(err => console.error("saved story migration failed", err));
  }, [clearLegacySavedClusterIds, data, legacySavedClusterIds, refetchSaved]);

  const rawClusters = useMemo(
    () => searchResult ?? (rootView === "saved" ? savedData?.clusters ?? [] : data?.clusters ?? []),
    [data?.clusters, rootView, savedData?.clusters, searchResult],
  );
  const enabledPublisherCount = useMemo(() => {
    const disabled = filter === "local" ? localDisabledPublisherIds : globalDisabledPublisherIds;
    return publishers.filter(p => (filter === "local" ? !p.is_global : p.is_global) && !disabled.includes(p.id)).length;
  }, [publishers, filter, localDisabledPublisherIds, globalDisabledPublisherIds]);

  const clusters = useMemo(() => {
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

    if (rootView === "blindspots") {
      arr = arr.filter(c => c.is_blindspot);
    } else if (rootView === "saved") {
      arr = arr.filter(c => savedStoryKeySet.has(c.story_key));
    }

    // Apply category filter.
    if (activeCategory !== "all") {
      arr = arr.filter(c => c.articles.some(a => a.category === activeCategory));
    }

    if (feedSort === "latest") {
      arr.sort((a, b) => b.last_updated.localeCompare(a.last_updated));
    } else if (feedSort === "covered") {
      arr.sort((a, b) => b.articles.length - a.articles.length || b.last_updated.localeCompare(a.last_updated));
    } else if (feedSort === "blindspots") {
      arr.sort((a, b) => Number(b.is_blindspot) - Number(a.is_blindspot) || b.last_updated.localeCompare(a.last_updated));
    } else {
      const score = (cluster: StoryCluster) => {
        const publisherCount = new Set(cluster.articles.map(a => a.publisher_id)).size;
        const freshnessHours = Math.max(0, (Date.now() - new Date(cluster.last_updated).getTime()) / 3_600_000);
        const freshness = 1 - Math.min(freshnessHours, 72) / 72;
        const independentCoverage = cluster.is_blindspot ? 0 : 1;
        return freshness * 0.45 + Math.min(publisherCount, 4) / 4 * 0.35 + independentCoverage * 0.15 + Number(cluster.is_blindspot) * 0.05;
      };
      arr.sort((a, b) => score(b) - score(a) || b.last_updated.localeCompare(a.last_updated));
    }
    return arr;
  }, [rawClusters, activeCategory, feedSort, rootView, filter, localDisabledPublisherIds, globalDisabledPublisherIds, biasOverrides, savedStoryKeys]);

  useEffect(() => {
    let cancelled = false;
    const q = searchQuery.trim();
    if (!q) {
      setSearchResult(null);
      setSearching(false);
      return;
    }
    setSearching(true);
    const timer = window.setTimeout(async () => {
      try {
        const result = await searchStories(q);
        if (!cancelled) setSearchResult(result.clusters);
      } catch (err) {
        console.error("search failed", err);
        if (!cancelled) setSearchResult([]);
      } finally {
        if (!cancelled) setSearching(false);
      }
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [searchQuery]);

  useEffect(() => {
    getRefreshStatus()
      .then(status => {
        if (status.last_refresh_at) {
          setLastRefreshLabel(formatDistanceToNow(new Date(status.last_refresh_at), { addSuffix: true }));
        }
        if (status.failed_sources.length > 0) setFailedSources(status.failed_sources);
      })
      .catch(() => undefined);
  }, []);

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    setFailedSources([]);
    setRefreshError(null);
    try {
      const result = await refreshFeed();
      if (result.failed_sources.length > 0) {
        setFailedSources(result.failed_sources);
      }
      await queryClient.invalidateQueries({ queryKey: clusterKeys.all() });
      await refetch();
      await refetchSaved();
      const status = await getRefreshStatus().catch(() => null);
      if (status?.last_refresh_at) setLastRefreshLabel(formatDistanceToNow(new Date(status.last_refresh_at), { addSuffix: true }));
    } catch (e) {
      console.error("refresh failed:", e);
      setRefreshError(errorMessage(e));
    }
    finally { setRefreshing(false); }
  }, [queryClient, refetch, refetchSaved]);

  const toggleSaved = useCallback(async (cluster: StoryCluster) => {
    const currentlySaved = savedStoryKeySet.has(cluster.story_key);
    setStorySaved(cluster.story_key, !currentlySaved);
    try {
      if (currentlySaved) {
        await unsaveStory(cluster.story_key);
      } else {
        await saveStory(cluster.story_key, cluster.articles.map(article => article.id));
      }
      await queryClient.invalidateQueries({ queryKey: clusterKeys.all() });
      await refetchSaved();
    } catch (err) {
      setStorySaved(cluster.story_key, currentlySaved);
      setRefreshError(errorMessage(err));
    }
  }, [queryClient, refetchSaved, savedStoryKeys, setStorySaved]);

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

  if (isLoading || publishersLoading || (isRefreshing && rawClusters.length === 0))
    return (
      <div className="feed-list">
        {[...Array(3)].map((_, i) => <CardSkeleton key={i} delay={`${i * 0.08}s`} />)}
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


  if (clusters.length === 0) {
    const emptyTitle = enabledPublisherCount === 0
      ? t(lang, "noEnabledSources")
      : rootView === "saved"
        ? t(lang, "noSaved")
        : rootView === "blindspots"
          ? t(lang, "noBlindspots")
        : activeCategory !== "all"
          ? t(lang, "noStoriesForFilter")
          : failedSources.length > 0
            ? t(lang, "allSourcesFailed")
            : t(lang, "noStories");
    const emptySub = enabledPublisherCount === 0
      ? t(lang, "noEnabledSourcesSub")
      : rootView === "saved"
        ? t(lang, "noSavedSub")
        : rootView === "blindspots"
          ? t(lang, "noBlindspotsSub")
        : activeCategory !== "all"
          ? t(lang, "noStoriesForFilterSub")
          : failedSources.length > 0
            ? t(lang, "allSourcesFailedSub")
            : t(lang, "noStoriesSub");
    const showRefresh = enabledPublisherCount > 0 && rootView === "feed" && activeCategory === "all";
    return (
      <div className="feed-scroll">
        <header className="feed-header">
          <div>
            <p className="feed-eyebrow">Merill</p>
        <h1>{searchQuery ? t(lang, "searchResults") : rootView === "saved" ? t(lang, "tabSaved") : rootView === "blindspots" ? t(lang, "tabBlindspots") : t(lang, "topStories")}</h1>
          </div>
        </header>
        <div className="feed-scope segmented-control" role="group" aria-label={t(lang, "feedScope")}>
          {(["local", "global"] as const).map(scope => (
            <button key={scope} data-active={filter === scope} aria-pressed={filter === scope} onClick={() => onFilterChange(scope)}>
              {t(lang, scope === "local" ? "tabLocal" : "tabGlobal")}
            </button>
          ))}
        </div>
        <div className="empty-state with-feed-header">
          <div className="empty-icon">
            <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="var(--color-label-tertiary)" strokeWidth="1.5">
              <rect x="3" y="3" width="7" height="7" rx="2" /><rect x="14" y="3" width="7" height="18" rx="2" /><rect x="3" y="14" width="7" height="7" rx="2" />
            </svg>
          </div>
          <p className="empty-title">{emptyTitle}</p>
          <p className="empty-sub">{emptySub}</p>
          {searchQuery ? (
            <button onClick={() => setSearchQuery("")} className="primary-btn">
              {t(lang, "clearSearch")}
            </button>
          ) : enabledPublisherCount === 0 ? (
            <button onClick={onOpenSettings} className="primary-btn">
              {t(lang, "openSettings")}
            </button>
          ) : activeCategory !== "all" ? (
            <button onClick={() => setActiveCategory("all")} className="primary-btn">
              {t(lang, "clearFilter")}
            </button>
          ) : showRefresh && (
            <button onClick={handleRefresh} className="primary-btn" disabled={isRefreshing}>
              {isRefreshing ? t(lang, "refreshing") : t(lang, "refresh")}
            </button>
          )}
        </div>
      </div>
    );
  }

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
          {format(t(lang, "sourcesFailed"), { n: String(failedSources.length) })}
          <button className="banner-dismiss" onClick={() => setFailedSources([])}>×</button>
        </div>
      )}

      {refreshError && (
        <div className="failed-sources-banner">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
            <circle cx="12" cy="12" r="10" /><path d="M12 8v4M12 16h.01" />
          </svg>
          {t(lang, "refreshError")}
          <button className="banner-dismiss" onClick={() => setRefreshError(null)}>×</button>
        </div>
      )}

      <header className="feed-header">
        <div>
          <p className="feed-eyebrow">Merill</p>
          <h1>{searchQuery ? t(lang, "searchResults") : rootView === "saved" ? t(lang, "tabSaved") : rootView === "blindspots" ? t(lang, "tabBlindspots") : t(lang, "topStories")}</h1>
          {lastRefreshLabel && <small className="refresh-status-label">{t(lang, "lastUpdated")} {lastRefreshLabel}</small>}
        </div>
        <div className="feed-header-actions">
          {(failedSources.length > 0 || refreshError) && (
            <button className={`header-filter-btn${diagnosticsOpen ? " active" : ""}`} onClick={() => setDiagnosticsOpen(open => !open)} aria-label={t(lang, "sourceDiagnostics")} aria-expanded={diagnosticsOpen}>
              <Info size={18} />
            </button>
          )}
          <button className={`header-filter-btn${filtersOpen ? " active" : ""}`} onClick={() => setFiltersOpen(open => !open)} aria-label={t(lang, "filters")} aria-expanded={filtersOpen}>
            <Filter size={18} />
          </button>
        </div>
      </header>
      <div className="feed-search">
        <Search size={16} />
        <input value={searchQuery} onChange={event => setSearchQuery(event.target.value)} placeholder={t(lang, "searchPlaceholder")} />
        {searching ? <div className="mini-spinner" /> : searchQuery && <button onClick={() => setSearchQuery("")} aria-label={t(lang, "clearSearch")}><X size={15} /></button>}
      </div>
      {diagnosticsOpen && (
        <div className="source-diagnostics">
          <strong>{t(lang, "sourceDiagnostics")}</strong>
          {refreshError && <p>{t(lang, "refreshError")}</p>}
          {failedSources.map(source => (
            <div key={source} className="source-diagnostic-row">
              <span>{source}</span>
              <button onClick={() => {
                const publisher = publishers.find(p => p.id === source || p.name === source);
                if (!publisher) return;
                if (publisher.is_global) useAppStore.getState().toggleGlobalPublisher(publisher.id);
                else useAppStore.getState().toggleLocalPublisher(publisher.id);
              }}>{t(lang, "disableSource")}</button>
            </div>
          ))}
          <button className="inline-add-trigger" onClick={handleRefresh}>{t(lang, "retrySources")}</button>
        </div>
      )}
      <div className="feed-scope segmented-control" role="group" aria-label={t(lang, "feedScope")}>
        {(["local", "global"] as const).map(scope => (
          <button key={scope} data-active={filter === scope} aria-pressed={filter === scope} onClick={() => onFilterChange(scope)}>
            {t(lang, scope === "local" ? "tabLocal" : "tabGlobal")}
          </button>
        ))}
      </div>
      {filtersOpen && (
        <div className="feed-filter-panel">
          <span>{t(lang, "feedSort")}</span>
          <div className="filter-options">
            {([["balanced", "sortBalanced"], ["latest", "sortLatest"], ["covered", "sortCovered"], ["blindspots", "sortBlindspots"]] as const).map(([value, label]) => (
              <button key={value} data-active={feedSort === value} onClick={() => { setFeedSort(value); setFiltersOpen(false); }}>
                {t(lang, label)} {feedSort === value && <Check size={15} />}
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Category filter pills */}
      {(
        <div className="category-pills">
          {ALL_CATEGORIES.map(cat => (
            <button
              key={cat}
              className={`category-pill ${activeCategory === cat ? "active" : ""}`}
              aria-pressed={activeCategory === cat}
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
              isSaved={savedStoryKeySet.has(c.story_key)}
              onToggleSaved={toggleSaved}
              onDismiss={(id) => setDismissedIds(s => new Set(s).add(id))}
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
  action: "toggle" | "delete";
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
        className={action === "toggle" ? "source-toggle" : "source-delete-btn"}
        onClick={onAction}
        aria-label={action === "toggle" ? `${publisher.name}: ${dimmed ? t(lang, "disabled") : t(lang, "enabled")}` : `${t(lang, "remove")} ${publisher.name}`}
        aria-pressed={action === "toggle" ? !dimmed : undefined}
      >
        {action === "toggle" ? <span /> : <Trash2 size={16} />}
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
              action={onDelete ? "delete" : "toggle"}
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

function AddSourceForm({ isGlobal, onAdded }: { isGlobal: boolean; onAdded: () => void | Promise<void> }) {
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
      await onAdded();
    } catch (err) {
      setError(`${t(lang, "addSourceError")}: ${errorMessage(err)}`);
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

function SettingsSection({
  title,
  open,
  onToggle,
  children,
}: {
  title: string;
  open: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  return (
    <section className="settings-accordion">
      <button className="settings-accordion-trigger" onClick={onToggle} aria-expanded={open}>
        <span>{title}</span>
        <ChevronDown size={18} className={open ? "open" : ""} />
      </button>
      {open && <div className="settings-accordion-body">{children}</div>}
    </section>
  );
}

// ── Settings Screen ─────────────────────────────────────────────────────────

export function SettingsScreen() {
  const { theme, setTheme, language, setLanguage, toggleLocalPublisher, isLocalPublisherEnabled } = useAppStore();
  const queryClient = useQueryClient();
  const [reclustering, setReclustering] = useState(false);
  const [wiping, setWiping] = useState(false);
  const [wipeConfirm, setWipeConfirm] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [settingsSuccess, setSettingsSuccess] = useState<string | null>(null);
  const [openSection, setOpenSection] = useState<"appearance" | "sources" | "advanced" | "about" | null>("appearance");
  const [showLocalForm, setShowLocalForm] = useState(false);
  const [showGlobalForm, setShowGlobalForm] = useState(false);
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
  const invalidatePublisherData = async () => {
    await invalidatePublishers();
    await invalidateClusters();
  };

  const handleDeleteCustom = async (id: string) => {
    setSettingsError(null);
    try {
      await removeCustomPublisher(id);
      // Articles from this publisher are deleted from DB, so clusters must be re-fetched.
      await invalidatePublisherData();
      setSettingsSuccess(t(language, "sourceRemoved"));
    } catch (e) {
      console.error("Failed to remove publisher:", e);
      setSettingsError(t(language, "removeSourceError"));
    }
  };

  const themeLabels: Record<string, string> = {
    system: t(language, "system"),
    light: t(language, "light"),
    dark: t(language, "dark"),
  };
  const toggleSection = (section: NonNullable<typeof openSection>) => setOpenSection(current => current === section ? null : section);
  const notifySourceAdded = async () => {
    await invalidatePublisherData();
    setSettingsSuccess(t(language, "sourceAdded"));
    setShowLocalForm(false);
    setShowGlobalForm(false);
  };

  return (
    <div className="settings-page animate-fade-up">
      <header className="settings-header">
        <p className="feed-eyebrow">Merill</p>
        <h1>{t(language, "settings")}</h1>
      </header>
      {settingsError && (
        <div className="settings-error">
          <span>{settingsError}</span>
          <button onClick={() => setSettingsError(null)} aria-label="Dismiss">×</button>
        </div>
      )}
      {settingsSuccess && <div className="settings-success"><Check size={16} /><span>{settingsSuccess}</span><button onClick={() => setSettingsSuccess(null)} aria-label={t(language, "close")}><X size={16} /></button></div>}
      <SettingsSection title={t(language, "appearance")} open={openSection === "appearance"} onToggle={() => toggleSection("appearance")}>
        <p className="settings-label">{t(language, "appearance")}</p>
        <div className="segmented-control">
          {(["system", "light", "dark"] as const).map(v => <button key={v} data-active={theme === v} aria-pressed={theme === v} onClick={() => setTheme(v)}>{themeLabels[v]}</button>)}
        </div>
        <p className="settings-label settings-sub-label">{t(language, "feedLanguage")}</p>
        <div className="segmented-control">
          {([{ v: "en" as const, l: "English" }, { v: "mt" as const, l: "Malti" }]).map(opt => <button key={opt.v} data-active={language === opt.v} aria-pressed={language === opt.v} onClick={() => setLanguage(opt.v)}>{opt.l}</button>)}
        </div>
      </SettingsSection>
      <SettingsSection title={t(language, "sources")} open={openSection === "sources"} onToggle={() => toggleSection("sources")}>
        <SourcesSection label={t(language, "sourcesLocal")} publishers={localPublishers} isEnabled={isLocalPublisherEnabled} onToggle={toggleLocalPublisher} articleCounts={articleCounts} />
        <button className="inline-add-trigger" onClick={() => setShowLocalForm(open => !open)}><Plus size={16} />{t(language, "addMaltaSource")}</button>
        {showLocalForm && <AddSourceForm isGlobal={false} onAdded={notifySourceAdded} />}
        <SourcesSection label={t(language, "sourcesGlobal")} publishers={globalPublishers} isEnabled={() => true} onToggle={() => undefined} onDelete={handleDeleteCustom} articleCounts={articleCounts} />
        <button className="inline-add-trigger" onClick={() => setShowGlobalForm(open => !open)}><Plus size={16} />{t(language, "addInternationalSource")}</button>
        {showGlobalForm && <AddSourceForm isGlobal onAdded={notifySourceAdded} />}
      </SettingsSection>
      <SettingsSection title={t(language, "advanced")} open={openSection === "advanced"} onToggle={() => toggleSection("advanced")}>
        <p className="settings-note">{t(language, "advancedSub")}</p>
        <button className="danger-btn quiet" disabled={reclustering} onClick={async () => {
          setReclustering(true); setSettingsError(null);
          try { await forceRecluster(); await invalidateClusters(); setSettingsSuccess(t(language, "reclusterSuccess")); }
          catch (e) { console.error(e); setSettingsError(t(language, "reclusterError")); }
          finally { setReclustering(false); }
        }}>{reclustering ? t(language, "reclustering") : t(language, "forceRecluster")}</button>
        {wipeConfirm ? (
          <div className="settings-group wipe-confirm">
            <p className="wipe-confirm-copy">{t(language, "wipeAllDataConfirm")}</p>
            <div className="wipe-confirm-actions">
              <button className="wipe-confirm-btn" onClick={() => setWipeConfirm(false)}>{t(language, "cancel")}</button>
              <button className="wipe-confirm-btn danger" disabled={wiping} onClick={async () => {
                setWiping(true); setSettingsError(null);
                try { await wipeAllData(); await invalidateClusters(); setSettingsSuccess(t(language, "wipeSuccess")); }
                catch (e) { console.error(e); setSettingsError(t(language, "wipeError")); }
                finally { setWiping(false); setWipeConfirm(false); }
              }}>{wiping ? t(language, "wipingData") : t(language, "wipeAllData")}</button>
            </div>
          </div>
        ) : <button className="danger-btn" onClick={() => setWipeConfirm(true)}>{t(language, "wipeAllData")}</button>}
      </SettingsSection>
      <SettingsSection title={t(language, "about")} open={openSection === "about"} onToggle={() => toggleSection("about")}>
        <div className="settings-about"><div className="app-icon lg"><img src="/app-icon.png" alt="Merill" /></div><p className="settings-app-name">Merill</p><p className="settings-version">v0.1.0</p></div>
      </SettingsSection>
    </div>
  );
}
