import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import type { AppSettings } from "@/types";

interface AppState extends AppSettings {
  setTheme: (t: AppSettings["theme"]) => void;
  setLanguage: (l: AppSettings["language"]) => void;
  toggleLocalPublisher: (id: string) => void;
  toggleGlobalPublisher: (id: string) => void;
  isLocalPublisherEnabled: (id: string) => boolean;
  isGlobalPublisherEnabled: (id: string) => boolean;
}

export const useAppStore = create<AppState>()(persist((set, get) => ({
  theme: "system",
  language: "en",
  savedClusterIds: [],
  localDisabledPublisherIds: [],
  globalDisabledPublisherIds: [],

  setTheme: (theme) => set({ theme }),
  setLanguage: (language) => set({ language }),

  toggleLocalPublisher: (id) => {
    const ids = get().localDisabledPublisherIds;
    set({ localDisabledPublisherIds: ids.includes(id) ? ids.filter(i => i !== id) : [...ids, id] });
  },
  toggleGlobalPublisher: (id) => {
    const ids = get().globalDisabledPublisherIds;
    set({ globalDisabledPublisherIds: ids.includes(id) ? ids.filter(i => i !== id) : [...ids, id] });
  },
  isLocalPublisherEnabled: (id) => !get().localDisabledPublisherIds.includes(id),
  isGlobalPublisherEnabled: (id) => !get().globalDisabledPublisherIds.includes(id),
}), {
  name: "malta-news-settings",
  storage: createJSONStorage(() => localStorage),
  partialize: (s) => ({
    theme: s.theme,
    language: s.language,
    savedClusterIds: s.savedClusterIds,
    localDisabledPublisherIds: s.localDisabledPublisherIds,
    globalDisabledPublisherIds: s.globalDisabledPublisherIds,
  }),
}));
