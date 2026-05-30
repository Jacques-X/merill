import { useState, useEffect, useCallback, useRef, memo } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Bookmark, Eye, Newspaper, Settings } from "lucide-react";
import { FeedScreen, SettingsScreen, StoryDetailScreen } from "@/screens";
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

function AppShell() {
  useThemeSync();
  const touchLastOpened = useAppStore(s => s.touchLastOpened);
  const feedScope = useAppStore(s => s.feedScope);
  const setFeedScope = useAppStore(s => s.setFeedScope);
  const [selectedCluster, setSelectedCluster] = useState<StoryCluster | null>(null);
  const [activeTab, setActiveTab] = useState<RootTab>("feed");
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
