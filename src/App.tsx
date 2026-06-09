import { useState, useEffect, useCallback, useRef, memo } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Bookmark, Check, ChevronRight, Eye, Newspaper, Settings } from "lucide-react";
import { FeedScreen, SettingsScreen, StoryDetailScreen } from "@/screens";
import { usePublishers } from "@/api/clusters";
import { useAppStore, sessionBaseline } from "@/store/useAppStore";
import { t } from "@/utils/i18n";
import type { StoryCluster } from "@/types";
import "@/index.css";

const queryClient = new QueryClient();
export type RootTab = "feed" | "blindspots" | "saved" | "settings";

function useThemeSync() {
  const theme = useAppStore((s) => s.theme);
  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove("light", "dark");
    if (theme === "light" || theme === "dark") root.classList.add(theme);
  }, [theme]);
}

const BottomDock = memo(function BottomDock({
  active,
  onChange,
}: {
  active: RootTab;
  onChange: (tab: RootTab) => void;
}) {
  const lang = useAppStore(s => s.language);
  const tabs = [
    ["feed", Newspaper, "tabFeed"],
    ["blindspots", Eye, "tabBlindspots"],
    ["saved", Bookmark, "tabSaved"],
    ["settings", Settings, "settings"],
  ] as const;

  return (
    <nav className="root-dock" aria-label="Primary">
      {tabs.map(([tab, Icon, label]) => (
        <button
          key={tab}
          className={`root-dock-btn${active === tab ? " active" : ""}`}
          onClick={() => onChange(tab)}
          aria-current={active === tab ? "page" : undefined}
        >
          <Icon size={20} strokeWidth={active === tab ? 2.4 : 1.9} />
          <span>{t(lang, label)}</span>
        </button>
      ))}
    </nav>
  );
});

function Onboarding() {
  const [step, setStep] = useState(0);
  const language = useAppStore(s => s.language);
  const setLanguage = useAppStore(s => s.setLanguage);
  const isLocalPublisherEnabled = useAppStore(s => s.isLocalPublisherEnabled);
  const toggleLocalPublisher = useAppStore(s => s.toggleLocalPublisher);
  const setOnboardingComplete = useAppStore(s => s.setOnboardingComplete);
  const { data: publishers = [] } = usePublishers();
  const localPublishers = publishers.filter(publisher => !publisher.is_global);

  return (
    <div className="onboarding-shell">
      <div className="onboarding-progress" aria-label={`Step ${step + 1} of 3`}>
        {[0, 1, 2].map(index => <span key={index} data-active={index <= step} />)}
      </div>
      {step === 0 && (
        <section>
          <p className="feed-eyebrow">Merill</p>
          <h1>News across perspectives</h1>
          <p>Choose the language Merill should use for headlines, summaries, and controls.</p>
          <div className="onboarding-options">
            {([["en", "English"], ["mt", "Malti"]] as const).map(([value, label]) => (
              <button key={value} data-active={language === value} onClick={() => setLanguage(value)}>
                {label}{language === value && <Check size={18} />}
              </button>
            ))}
          </div>
        </section>
      )}
      {step === 1 && (
        <section>
          <p className="feed-eyebrow">Sources</p>
          <h1>Build your Malta feed</h1>
          <p>Select the publishers you want included. You can change this at any time in Settings.</p>
          <div className="onboarding-source-list">
            {localPublishers.map(publisher => {
              const enabled = isLocalPublisherEnabled(publisher.id);
              return (
                <button key={publisher.id} onClick={() => toggleLocalPublisher(publisher.id)} aria-pressed={enabled}>
                  <span>{publisher.name}</span>
                  <Check size={17} style={{ opacity: enabled ? 1 : 0 }} />
                </button>
              );
            })}
          </div>
        </section>
      )}
      {step === 2 && (
        <section>
          <p className="feed-eyebrow">How Merill reads coverage</p>
          <h1>Ownership is context</h1>
          <p>Colors describe publisher ownership, not whether an article is true. A blindspot means Merill found coverage without an independent publisher in the story group.</p>
          <div className="onboarding-explainer">
            <div><span className="ownership-dot independent" /><strong>Independent</strong><small>Commercial or investigative outlets</small></div>
            <div><span className="ownership-dot party" /><strong>Party-owned</strong><small>PL or PN media organisations</small></div>
            <div><span className="ownership-dot state" /><strong>State-owned</strong><small>Publicly owned media</small></div>
          </div>
        </section>
      )}
      <button className="onboarding-next" onClick={() => {
        if (step < 2) setStep(current => current + 1);
        else setOnboardingComplete(true);
      }}>
        {step < 2 ? "Continue" : "Open Merill"} <ChevronRight size={18} />
      </button>
    </div>
  );
}

function AppShell() {
  useThemeSync();
  const touchLastOpened = useAppStore(s => s.touchLastOpened);
  const feedScope = useAppStore(s => s.feedScope);
  const setFeedScope = useAppStore(s => s.setFeedScope);
  const [selectedCluster, setSelectedCluster] = useState<StoryCluster | null>(null);
  const [activeTab, setActiveTab] = useState<RootTab>("feed");
  const onboardingComplete = useAppStore(s => s.onboardingComplete);
  const detailInternalBack = useRef<(() => void) | null>(null);

  useEffect(() => {
    sessionBaseline.current = useAppStore.getState().lastOpenedAt;
    touchLastOpened();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleBack = useCallback(() => {
    if (!selectedCluster) return;
    if (detailInternalBack.current) detailInternalBack.current();
    else setSelectedCluster(null);
  }, [selectedCluster]);

  useEffect(() => {
    if (!selectedCluster) return;
    const startPos = { x: 0, y: 0 };
    const onStart = (event: TouchEvent) => {
      startPos.x = event.touches[0].clientX;
      startPos.y = event.touches[0].clientY;
    };
    const onEnd = (event: TouchEvent) => {
      const dx = event.changedTouches[0].clientX - startPos.x;
      const dy = Math.abs(event.changedTouches[0].clientY - startPos.y);
      if (dx > 60 && dy < 80 && startPos.x < 60) handleBack();
    };
    document.addEventListener("touchstart", onStart, { passive: true });
    document.addEventListener("touchend", onEnd, { passive: true });
    return () => {
      document.removeEventListener("touchstart", onStart);
      document.removeEventListener("touchend", onEnd);
    };
  }, [handleBack, selectedCluster]);

  if (!onboardingComplete) {
    return <Onboarding />;
  }

  if (selectedCluster) {
    return (
      <div className="app-root">
        <main className="screen-content no-pad-top">
          <StoryDetailScreen cluster={selectedCluster} internalBackRef={detailInternalBack} onBack={handleBack} />
        </main>
      </div>
    );
  }

  return (
    <div className="app-root">
      <main className="screen-content no-pad-top">
        {activeTab === "settings" ? (
          <SettingsScreen />
        ) : (
          <FeedScreen
            onSelectCluster={setSelectedCluster}
            filter={feedScope}
            onFilterChange={setFeedScope}
            rootView={activeTab}
            onOpenSettings={() => setActiveTab("settings")}
          />
        )}
      </main>
      <BottomDock active={activeTab} onChange={setActiveTab} />
    </div>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AppShell />
    </QueryClientProvider>
  );
}
