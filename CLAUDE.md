# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Merill** is a Malta news aggregator desktop/mobile app (Tauri + React) that clusters related news stories and analyzes publisher bias. The app fetches from RSS feeds, groups articles by topic, and provides bias coverage analysis to highlight different perspectives.

## Tech Stack

- **Frontend:** React 19, TypeScript, Vite, TailwindCSS 4, Zustand (state), React Query
- **Desktop/Mobile:** Tauri 2 (Rust backend)
- **iOS Integration:** Swift bridge for on-device AI summaries
- **Database:** SQLite (local, on-device)

## Development Commands

### Frontend Development
- `npm run dev` — Start Vite dev server on `http://localhost:5173` with HMR
- `npm run build` — TypeScript check + Vite production build (outputs to `dist/`)
- `npm run lint` — Run ESLint on all files
- `npm run preview` — Preview production build locally

### Tauri (Desktop) Development
- `npm run tauri:dev` — Run desktop app in dev mode (pulls frontend from `localhost:5173`)
- `npm run tauri:build` — Build production desktop app
- `npm run tauri` — Run `tauri` CLI directly (e.g., `npm run tauri migrate`)

### Tauri (iOS) Development
- `npm run tauri:ios:init` — Initialize iOS project
- `npm run tauri:ios:dev` — Run on iOS simulator/device
- `npm run tauri:ios:build` — Build iOS app

## Architecture Overview

### Frontend Structure

**App Shell (`src/App.tsx`)**
- Navigation hub with three screens: Feed, Story Detail, Settings
- Bottom dock navigation (Back, Local/Global tabs, Settings)
- Theme and language syncing to store
- Swipe-right-to-back gesture handling
- Screen overlay logic (detail view hides dock tabs)

**State Management (`src/store/useAppStore.ts` via Zustand)**
- Theme, language, reader font size
- Publisher enable/disable toggles (separate local/global lists)
- Publisher bias overrides
- "Last opened at" timestamp for "New" story badges
- Persisted to localStorage with selective key serialization

**Data Fetching (`src/api/clusters.ts` via React Query)**
- `useClusters()` — Fetches story clusters, 15-min refetch interval
- `usePublishers()` — Fetches publisher list (cached indefinitely)
- `refreshFeed()`, `addCustomPublisher()`, `removeCustomPublisher()` — Tauri invocations
- `splitCluster()`, `forceRecluster()` — Manual clustering operations
- All calls go through Tauri's `invoke()` to the Rust backend

**Screens** (`src/screens/index.tsx`)
- **FeedScreen:** Pull-to-refresh feed, filter by local/global, bias bar, story cards
- **StoryDetailScreen:** Article reader with multiple stories in cluster, font size picker
- **SettingsScreen:** Theme, language, publisher toggles, custom publisher management

**Components**
- **StoryCard:** Displays story summary with "New" badge (compares against `sessionBaseline`), publisher logos, bias bar
- **BiasBar:** Visual coverage of 9 bias categories, interactive legend

**Utilities**
- `i18n.ts` — Translations (en/mt) for all UI strings
- `headline.ts` — Cluster headline selection and article snippet formatting
- `bias.ts` — Bias coverage calculation from article set
- `constants.ts` — Color mappings, bias option lists
- **Types** (`src/types/index.ts`) — `StoryCluster`, `Article`, `Publisher`, `BiasCategory`, etc.

### Backend Structure (Tauri/Rust)

**Key Modules** (`src-tauri/src/`)
- **lib.rs** — Tauri command exports, iOS/native summaries, data refresh pipeline
- **scraper.rs** — RSS fetching, HTML parsing, article extraction, language detection
- **clustering.rs** — Article similarity/clustering algorithm, headline generation
- **db.rs** — SQLite schema, queries (articles, clusters, publishers)
- **pipeline.rs** — Full refresh flow: fetch → scrape → cluster → store
- **publishers.rs** — Publisher registry with bias categories
- **category.rs** — Article category classification
- **translate.rs** — Language translation (Maltese ↔ English)

**Tauri Commands** (invoked from frontend via `invoke()`)
- `get_clusters` — Returns all clusters with articles, filtering for blindspots
- `get_publishers` — Returns publisher registry with bias metadata
- `refresh_feed` — Runs full fetch-scrape-cluster pipeline, returns failed sources
- `add_custom_publisher(url, name, isGlobal)` — Adds user's custom RSS feed
- `remove_custom_publisher(id)` — Removes custom publisher
- `split_cluster(articleId, headline, publishedAt)` — Breaks article into separate cluster
- `force_recluster()` — Rebuilds all clusters from articles in DB
- `wipeAllData()` — Clears all articles and clusters

**Database Schema**
- Articles table with publisher, headline, body, snippet, image, category, language
- Clusters table with first_reported, last_updated, ai_headline, ai_summary, blindspot flag
- Publishers table with name, bias_category, logo_url, custom flag

**iOS-specific**
- `ios_ai` module bridges to Swift `merill_generate_summary()` for on-device summaries
- Falls back to headline + snippet if Swift unavailable

## Key Design Patterns

1. **Inverse Navigation:** Back button visible in overlay mode (detail/settings) but hidden nav tabs—keeps app structure clear
2. **Session Baseline:** `sessionBaseline.current` captures previous session's `lastOpenedAt` before touching the store, so "New" badges persist accurately across sessions
3. **Bias Overrides:** Publisher bias can be manually corrected per-user without modifying the registry
4. **Pull-to-Refresh:** Custom implementation with 80px threshold; prevents while already refreshing
5. **Local vs. Global Scope:** Feed can be filtered by whether stories come from local/global publishers; toggles persist per scope
6. **Lazy i18n:** Translations computed on demand via `t(lang, key)` helper

## Debugging & Testing

**Type Checking**
```bash
npx tsc -b
```

**Linting**
```bash
npm run lint
# Fix issues:
npm run lint -- --fix
```

**Tauri Logs**
- Rust stderr goes to console when running `npm run tauri:dev`
- Frontend console via dev tools (Ctrl/Cmd+Shift+I in dev mode)

**Database Inspection**
- SQLite file at `$HOME/Library/Application Support/mt.merill.app/` (macOS)
- Use `sqlite3` CLI or DB browser to inspect

**Custom Publishers Testing**
- Add a test RSS feed URL via Settings → "Add Publisher"
- Check Rust scraper output in console for parse issues

## File Organization

```
src/
  App.tsx                      # App shell, navigation, overlay logic
  main.tsx                     # Entry point, error boundary
  index.css                    # Tailwind + custom vars (theme colors)
  api/clusters.ts              # React Query hooks for Tauri commands
  components/
    StoryCard/                 # Story summary card component
    BiasBar/                   # Bias coverage visualization
  screens/
    index.tsx                  # FeedScreen, StoryDetailScreen, SettingsScreen
  store/useAppStore.ts         # Zustand store with localStorage persistence
  types/index.ts               # TypeScript interfaces for data models
  utils/
    i18n.ts                    # Translation helper
    headline.ts                # Headline + snippet selection
    bias.ts                    # Bias coverage calculations
    constants.ts               # Colors, bias options

src-tauri/
  tauri.conf.json              # Window size (430x932), dev/build config
  Cargo.toml                   # Rust dependencies, lib/bin config
  src/
    lib.rs                     # Tauri commands, iOS bridge
    scraper.rs                 # RSS + HTML parsing
    clustering.rs              # Article grouping, headline gen
    db.rs                      # SQLite schema + queries
    pipeline.rs                # Refresh orchestration
    publishers.rs              # Publisher registry
    category.rs                # Category classification
    translate.rs               # Translation service
```

## Common Workflows

**Adding a Translation Key**
1. Add key-value pair to `src/utils/i18n.ts` under both "en" and "mt" objects
2. Use `t(lang, "keyName")` in components
3. No rebuild needed; hot reload picks it up

**Adjusting Publisher Bias Colors**
- Edit `BIAS_COLORS` in `src/utils/constants.ts`
- Rebuild CSS if needed; TailwindCSS will pick up new palette

**Customizing Story Clustering**
- Threshold and algorithm live in `src-tauri/src/clustering.rs`
- Rebuild Tauri backend: `npm run tauri:build`
- In dev mode, changes auto-detect and recompile on save

**Fixing a Feed Parse Error**
- Check `src-tauri/src/scraper.rs` for HTML selectors (often break when site updates)
- Add test case with sample HTML
- Rebuild and test via `npm run tauri:dev` → add that publisher

**Running on iOS**
1. `npm run tauri:ios:init` (one-time setup)
2. `npm run tauri:ios:dev` for simulator, or build + deploy to device
3. Check Xcode console for runtime logs; Rust output also appears there

## Notes for Future Work

- Type-aware ESLint rules available if needed (see README.md for config upgrade path)
- React Compiler can be enabled to optimize performance, but disabled by default due to dev/build overhead
- Stale time for clusters (15 min) matches refetch interval to avoid render churn from background refreshes
- "Blindspot" clusters (underreported stories) can be filtered via `blindspotsOnly` in `get_clusters`
- Custom publishers are persisted in DB; removing a publisher clears its articles on next refresh
