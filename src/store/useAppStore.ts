import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import type { AppSettings, BiasCategory } from "@/types";

export type ReaderFontSize = "sm" | "md" | "lg";

interface AppState extends AppSettings {
  setTheme: (t: AppSettings["theme"]) => void;
  setLanguage: (l: AppSettings["language"]) => void;
  toggleLocalPublisher: (id: string) => void;
  toggleGlobalPublisher: (id: string) => void;
  isLocalPublisherEnabled: (id: string) => boolean;
  isGlobalPublisherEnabled: (id: string) => boolean;
  setPublisherBias: (id: string, category: BiasCategory) => void;
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
  savedClusterIds: [],
  localDisabledPublisherIds: [],
  globalDisabledPublisherIds: [],
  publisherBiasOverrides: {} as Record<string, BiasCategory>,
  lastOpenedAt: new Date(0).toISOString(), // epoch → all stories appear New on first run
  readerFontSize: "md",

  setTheme: (theme) => set({ theme }),
  setLanguage: (language) => set({ language }),

  toggleLocalPublisher: (id) => {
    const s = new Set(get().localDisabledPublisherIds);
    s.has(id) ? s.delete(id) : s.add(id);
    set({ localDisabledPublisherIds: [...s] });
  },
  toggleGlobalPublisher: (id) => {
    const s = new Set(get().globalDisabledPublisherIds);
    s.has(id) ? s.delete(id) : s.add(id);
    set({ globalDisabledPublisherIds: [...s] });
  },
  isLocalPublisherEnabled: (id) => !get().localDisabledPublisherIds.includes(id),
  isGlobalPublisherEnabled: (id) => !get().globalDisabledPublisherIds.includes(id),
  setPublisherBias: (id, category) => set(s => ({
    publisherBiasOverrides: { ...s.publisherBiasOverrides, [id]: category },
  })),

  touchLastOpened: () => set({ lastOpenedAt: new Date().toISOString() }),
  setReaderFontSize: (readerFontSize) => set({ readerFontSize }),
}), {
  name: "malta-news-settings",
  storage: createJSONStorage(() => localStorage),
  partialize: (s) => ({
    theme: s.theme,
    language: s.language,
    savedClusterIds: s.savedClusterIds,
    localDisabledPublisherIds: s.localDisabledPublisherIds,
    globalDisabledPublisherIds: s.globalDisabledPublisherIds,
    publisherBiasOverrides: s.publisherBiasOverrides,
    lastOpenedAt: s.lastOpenedAt,
    readerFontSize: s.readerFontSize,
  }),
}));
