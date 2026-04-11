import { useState, useEffect, useCallback, useRef } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { FeedScreen, SettingsScreen, StoryDetailScreen } from "@/screens";
import type { FeedFilter } from "@/screens";
import { useAppStore, sessionBaseline } from "@/store/useAppStore";
import { t } from "@/utils/i18n";
import type { StoryCluster } from "@/types";
import "@/index.css";

const queryClient = new QueryClient();

/* ── Theme class syncing ───────────────────────────────────────────────── */
function useThemeSync() {
  const theme = useAppStore((s) => s.theme);
  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove("light", "dark");
    if (theme === "light" || theme === "dark") {
      root.classList.add(theme);
    }
  }, [theme]);
}

/* ── Bottom Dock ─────────────────────────────────────────────────────────── */
function BottomDock({
  active,
  onTabChange,
  onBack,
  onSettings,
  mode = "feed",
}: {
  active: FeedFilter;
  onTabChange: (t: FeedFilter) => void;
  onBack: () => void;
  onSettings: () => void;
  mode?: "feed" | "overlay";
}) {
  const lang = useAppStore(s => s.language);
  const overlay = mode === "overlay";

  return (
    <nav className="tab-bar">
      {/* Back — always visible, always same position */}
      <button className="dock-icon-btn" onClick={onBack} aria-label="Back">
        <svg width="11" height="18" viewBox="0 0 12 18" fill="none">
          <path d="M10 1L2 9l8 8" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </button>

      {/* Center pill — hidden in overlay mode */}
      <div className="dock-tab-pill" style={overlay ? { opacity: 0, pointerEvents: "none" } : undefined}>
        <button
          className={`tab-btn${active === "local" ? " active" : ""}`}
          onClick={() => onTabChange("local")}
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
            <path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
            <polyline points="9 22 9 12 15 12 15 22" />
          </svg>
          <span className="tab-label">{t(lang, "tabLocal")}</span>
        </button>
        <button
          className={`tab-btn${active === "global" ? " active" : ""}`}
          onClick={() => onTabChange("global")}
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="10" />
            <line x1="2" y1="12" x2="22" y2="12" />
            <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
          </svg>
          <span className="tab-label">{t(lang, "tabGlobal")}</span>
        </button>
      </div>

      {/* Settings — hidden in overlay mode */}
      <button
        className="dock-icon-btn"
        onClick={onSettings}
        aria-label="Settings"
        style={overlay ? { opacity: 0, pointerEvents: "none" } : undefined}
      >
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
          <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
          <circle cx="12" cy="12" r="3" />
        </svg>
      </button>
    </nav>
  );
}

/* ── App Shell ─────────────────────────────────────────────────────────── */
function AppShell() {
  useThemeSync();
  const lang = useAppStore(s => s.language);
  const touchLastOpened = useAppStore(s => s.touchLastOpened);

  useEffect(() => {
    // Snapshot the persisted value as our session baseline BEFORE we overwrite it,
    // so StoryCard can compare against it for "New" badges.
    sessionBaseline.current = useAppStore.getState().lastOpenedAt;
    // Record now for the NEXT session's comparison.
    touchLastOpened();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const [selectedCluster, setSelectedCluster] = useState<StoryCluster | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [activeTab, setActiveTab] = useState<FeedFilter>("local");

  // Ref that StoryDetailScreen populates when it has its own sub-back action
  // (e.g. article reader open → back goes to cluster view, not home)
  const detailInternalBack = useRef<(() => void) | null>(null);

  const handleBack = useCallback(() => {
    if (showSettings) { setShowSettings(false); return; }
    if (selectedCluster) {
      if (detailInternalBack.current) {
        detailInternalBack.current();
      } else {
        setSelectedCluster(null);
      }
    }
  }, [showSettings, selectedCluster]);

  // Swipe right-to-left-edge = back
  useEffect(() => {
    const startPos = { x: 0, y: 0 };
    const onStart = (e: TouchEvent) => {
      startPos.x = e.touches[0].clientX;
      startPos.y = e.touches[0].clientY;
    };
    const onEnd = (e: TouchEvent) => {
      const dx = e.changedTouches[0].clientX - startPos.x;
      const dy = Math.abs(e.changedTouches[0].clientY - startPos.y);
      if (dx > 60 && dy < 80 && startPos.x < 60) handleBack();
    };
    document.addEventListener("touchstart", onStart, { passive: true });
    document.addEventListener("touchend", onEnd, { passive: true });
    return () => {
      document.removeEventListener("touchstart", onStart);
      document.removeEventListener("touchend", onEnd);
    };
  }, [handleBack]);

  const isOverlay = showSettings || selectedCluster !== null;

  const dock = (
    <BottomDock
      active={activeTab}
      onTabChange={(tab) => { setShowSettings(false); setSelectedCluster(null); setActiveTab(tab); }}
      onBack={handleBack}
      onSettings={() => setShowSettings(true)}
      mode={isOverlay ? "overlay" : "feed"}
    />
  );

  // Settings overlay
  if (showSettings) {
    return (
      <div className="app-root" style={{ background: "var(--color-bg)" }}>
        <main className="screen-content no-pad-top">
          <SettingsScreen />
        </main>
        {dock}
      </div>
    );
  }

  // Story detail view
  if (selectedCluster) {
    return (
      <div className="app-root" style={{ background: "var(--color-bg)" }}>
        <main className="screen-content no-pad-top">
          <StoryDetailScreen cluster={selectedCluster} internalBackRef={detailInternalBack} />
        </main>
        {dock}
      </div>
    );
  }

  // Main feed
  return (
    <div className="app-root" style={{ background: "var(--color-bg)" }}>
      <main className="screen-content no-pad-top">
        <FeedScreen onSelectCluster={setSelectedCluster} filter={activeTab} />
      </main>
      {dock}
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
