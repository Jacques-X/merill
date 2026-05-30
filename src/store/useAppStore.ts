import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import type { AppSettings, BiasCategory } from "@/types";

export type ReaderFontSize = "sm" | "md" | "lg";

interface AppState extends AppSettings {
  setTheme: (t: AppSettings["theme"]) => void;
  setLanguage: (l: AppSettings["language"]) => void;
  setFeedScope: (scope: AppSettings["feedScope"]) => void;
  toggleLocalPublisher: (id: string) => void;
  toggleGlobalPublisher: (id: string) => void;
  isLocalPublisherEnabled: (id: string) => boolean;
  isGlobalPublisherEnabled: (id: string) => boolean;
  setPublisherBias: (id: string, category: BiasCategory) => void;
  toggleSavedCluster: (id: string) => void;
  isClusterSaved: (id: string) => boolean;
  // Last time the user opened the app — used to mark "New" stories.
  lastOpenedAt: string;
  touchLastOpened: () => void;
  // Font size preference for the article reader.
  readerFontSize: ReaderFontSize;
  setReaderFontSize: (size: ReaderFontSize) => void;
}

// Mutable ref holding the previous session's timestamp.
// App.tsx writes it once on mount (before calling touchLastOpened).
// StoryCard reads it to decide "New" badges.
export const sessionBaseline = { current: new Date(0).toISOString() };

export const useAppStore = create<AppState>()(persist((set, get) => ({
  theme: "system",
  language: "en",
  feedScope: "local",
  savedClusterIds: [],
  localDisabledPublisherIds: [],
  globalDisabledPublisherIds: [],
  publisherBiasOverrides: {} as Record<string, BiasCategory>,
  lastOpenedAt: new Date(0).toISOString(), // epoch → all stories appear New on first run
  readerFontSize: "md",

  setTheme: (theme) => set({ theme }),
  setLanguage: (language) => set({ language }),
  setFeedScope: (feedScope) => set({ feedScope }),

  toggleLocalPublisher: (id) => {
    const s = new Set(get().localDisabledPublisherIds);
    if (s.has(id)) {
      s.delete(id);
    } else {
      s.add(id);
    }
    set({ localDisabledPublisherIds: [...s] });
  },
  toggleGlobalPublisher: (id) => {
    const s = new Set(get().globalDisabledPublisherIds);
    if (s.has(id)) {
      s.delete(id);
    } else {
      s.add(id);
    }
    set({ globalDisabledPublisherIds: [...s] });
  },
  isLocalPublisherEnabled: (id) => !get().localDisabledPublisherIds.includes(id),
  isGlobalPublisherEnabled: (id) => !get().globalDisabledPublisherIds.includes(id),
  setPublisherBias: (id, category) => set(s => ({
    publisherBiasOverrides: { ...s.publisherBiasOverrides, [id]: category },
  })),
  toggleSavedCluster: (id) => {
    const saved = new Set(get().savedClusterIds);
    if (saved.has(id)) {
      saved.delete(id);
    } else {
      saved.add(id);
    }
    set({ savedClusterIds: [...saved] });
  },
  isClusterSaved: (id) => get().savedClusterIds.includes(id),

  touchLastOpened: () => set({ lastOpenedAt: new Date().toISOString() }),
  setReaderFontSize: (readerFontSize) => set({ readerFontSize }),
}), {
  name: "malta-news-settings",
  storage: createJSONStorage(() => localStorage),
  partialize: (s) => ({
    theme: s.theme,
    language: s.language,
    feedScope: s.feedScope,
    savedClusterIds: s.savedClusterIds,
    localDisabledPublisherIds: s.localDisabledPublisherIds,
    globalDisabledPublisherIds: s.globalDisabledPublisherIds,
    publisherBiasOverrides: s.publisherBiasOverrides,
    lastOpenedAt: s.lastOpenedAt,
    readerFontSize: s.readerFontSize,
  }),
}));
