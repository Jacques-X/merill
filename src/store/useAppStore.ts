import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import type { AppSettings, BiasCategory } from "@/types";

export type ReaderFontSize = "sm" | "md" | "lg";
export type ReaderLineSpacing = "compact" | "comfortable" | "relaxed";
export type ReaderTextMode = "translated" | "original";

interface AppState extends AppSettings {
  setTheme: (t: AppSettings["theme"]) => void;
  setLanguage: (l: AppSettings["language"]) => void;
  setFeedScope: (scope: AppSettings["feedScope"]) => void;
  toggleLocalPublisher: (id: string) => void;
  toggleGlobalPublisher: (id: string) => void;
  isLocalPublisherEnabled: (id: string) => boolean;
  isGlobalPublisherEnabled: (id: string) => boolean;
  setPublisherBias: (id: string, category: BiasCategory) => void;
  setStorySaved: (storyKey: string, saved: boolean) => void;
  replaceSavedStoryKeys: (storyKeys: string[]) => void;
  isStorySaved: (storyKey: string) => boolean;
  legacySavedClusterIds: string[];
  clearLegacySavedClusterIds: () => void;
  // Last time the user opened the app — used to mark "New" stories.
  lastOpenedAt: string;
  touchLastOpened: () => void;
  // Font size preference for the article reader.
  readerFontSize: ReaderFontSize;
  setReaderFontSize: (size: ReaderFontSize) => void;
  readerLineSpacing: ReaderLineSpacing;
  setReaderLineSpacing: (spacing: ReaderLineSpacing) => void;
  readerTextMode: ReaderTextMode;
  setReaderTextMode: (mode: ReaderTextMode) => void;
  onboardingComplete: boolean;
  setOnboardingComplete: (complete: boolean) => void;
}

// Mutable ref holding the previous session's timestamp.
// App.tsx writes it once on mount (before calling touchLastOpened).
// StoryCard reads it to decide "New" badges.
export const sessionBaseline = { current: new Date(0).toISOString() };

export const useAppStore = create<AppState>()(persist((set, get) => ({
  theme: "system",
  language: "en",
  feedScope: "local",
  savedStoryKeys: [],
  legacySavedClusterIds: [],
  localDisabledPublisherIds: [],
  globalDisabledPublisherIds: [],
  publisherBiasOverrides: {} as Record<string, BiasCategory>,
  lastOpenedAt: new Date(0).toISOString(), // epoch → all stories appear New on first run
  readerFontSize: "md",
  readerLineSpacing: "comfortable",
  readerTextMode: "translated",
  onboardingComplete: false,

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
  setStorySaved: (storyKey, isSaved) => {
    const saved = new Set(get().savedStoryKeys);
    if (isSaved) saved.add(storyKey);
    else saved.delete(storyKey);
    set({ savedStoryKeys: [...saved] });
  },
  replaceSavedStoryKeys: (savedStoryKeys) => set({ savedStoryKeys }),
  isStorySaved: (storyKey) => get().savedStoryKeys.includes(storyKey),
  clearLegacySavedClusterIds: () => set({ legacySavedClusterIds: [] }),

  touchLastOpened: () => set({ lastOpenedAt: new Date().toISOString() }),
  setReaderFontSize: (readerFontSize) => set({ readerFontSize }),
  setReaderLineSpacing: (readerLineSpacing) => set({ readerLineSpacing }),
  setReaderTextMode: (readerTextMode) => set({ readerTextMode }),
  setOnboardingComplete: (onboardingComplete) => set({ onboardingComplete }),
}), {
  name: "malta-news-settings",
  storage: createJSONStorage(() => localStorage),
  partialize: (s) => ({
    theme: s.theme,
    language: s.language,
    feedScope: s.feedScope,
    savedStoryKeys: s.savedStoryKeys,
    legacySavedClusterIds: s.legacySavedClusterIds,
    localDisabledPublisherIds: s.localDisabledPublisherIds,
    globalDisabledPublisherIds: s.globalDisabledPublisherIds,
    publisherBiasOverrides: s.publisherBiasOverrides,
    lastOpenedAt: s.lastOpenedAt,
    readerFontSize: s.readerFontSize,
    readerLineSpacing: s.readerLineSpacing,
    readerTextMode: s.readerTextMode,
    onboardingComplete: s.onboardingComplete,
  }),
  version: 2,
  migrate: (persisted, version) => {
    const state = persisted as Record<string, unknown>;
    if (version < 2) {
      state.savedStoryKeys = [];
      state.legacySavedClusterIds = Array.isArray(state.savedClusterIds)
        ? state.savedClusterIds
        : [];
      delete state.savedClusterIds;
      state.onboardingComplete = false;
    }
    return state as unknown as AppState;
  },
}));
