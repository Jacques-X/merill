# Merill

Malta news aggregator that clusters related stories across publishers and makes differences in coverage visible — including which stories are missing independent voices.

Built with Tauri 2 (Rust backend) + React 19 (TypeScript frontend). Runs as a desktop app and iOS app. All data stays on-device.

## Quick Start

```bash
npm install

# Frontend only (browser preview)
npm run dev

# Desktop app (Tauri)
npm run tauri:dev

# iOS (requires Xcode + initialized project)
npm run tauri:ios:dev
```

## Tech Stack

| Layer | Technology |
|---|---|
| UI | React 19, TypeScript, Vite, TailwindCSS 4 |
| State | Zustand (persisted to localStorage) |
| Data fetching | React Query |
| Desktop/mobile shell | Tauri 2 |
| Backend | Rust |
| Database | SQLite (on-device) |
| iOS AI | Swift bridge for on-device summaries |

## Project Structure

```
src/                        # React frontend
  App.tsx                   # App shell, navigation, theme sync
  api/clusters.ts           # React Query hooks for Tauri commands
  components/
    BiasBar/                # Bias coverage bar + legend
    StoryCard/              # Feed card with headline, snippet, publisher avatars
  screens/index.tsx         # FeedScreen · StoryDetailScreen · SettingsScreen
  store/useAppStore.ts      # Zustand store (settings, saved stories, preferences)
  types/index.ts            # Shared TypeScript interfaces
  utils/
    bias.ts                 # BIAS_META, coverage computation
    constants.ts            # BIAS_COLORS (derived from BIAS_META), bias dropdown options
    headline.ts             # Cluster/article headline selection
    i18n.ts                 # en/mt translations + t() + format() helpers

src-tauri/src/              # Rust backend
  lib.rs                    # Tauri command exports, iOS bridge
  scraper.rs                # RSS fetching and HTML parsing
  clustering.rs             # Article similarity and grouping
  db.rs                     # SQLite schema and queries
  pipeline.rs               # Full refresh orchestration
  publishers.rs             # Publisher registry and bias metadata
  category.rs               # Article category classification
  translate.rs              # Maltese ↔ English translation
```

## Key Concepts

**Story cluster** — a group of articles Merill believes cover the same event, drawn from multiple publishers.

**Blindspot** — a cluster where no independent publisher (commercial or investigative) is represented; only state-owned, party-owned, or church-owned sources covered it.

**Bias categories** — publisher ownership, not editorial quality. Local: `state_owned`, `party_owned_pl`, `party_owned_pn`, `church_owned`, `commercial_independent`, `investigative_independent`. Global: `left`, `centre`, `right`. Users can override any publisher's category.

**Session baseline** — on app launch, the previous session's `lastOpenedAt` is captured before updating it. StoryCards compare `cluster.first_reported_at` against this baseline to show "New" badges.

## Common Tasks

### Add a translation key
1. Add key + value to both `en` and `mt` objects in `src/utils/i18n.ts`
2. Use `t(lang, "keyName")` in components
3. For strings with named placeholders (e.g. `{n}`), use `format(t(lang, "key"), { n: "3" })`

### Change bias colours
Edit the `hex` field in the relevant entry in `BIAS_META` (`src/utils/bias.ts`). `BIAS_COLORS` in `constants.ts` is derived from `BIAS_META` automatically — do not edit it directly.

### Adjust story clustering
Edit `src-tauri/src/clustering.rs`. Rebuild the Tauri backend: `npm run tauri:build` (or let `tauri:dev` recompile on save).

### Inspect the local database
```
sqlite3 "$HOME/Library/Application Support/mt.merill.app/merill.db"
```

### Type-check without building
```bash
npx tsc -b
```

## Notes

- `staleTime` for clusters is 15 min, matching `refetchInterval`, to avoid unnecessary re-renders from background polling.
- React Compiler is intentionally disabled (dev/build overhead); enable via `src/App.tsx` if needed.
- The iOS build uses a Swift bridge (`AIEmbedding.swift`, `AISummary.swift`) for on-device ML; the Rust backend falls back gracefully when the bridge is absent.
