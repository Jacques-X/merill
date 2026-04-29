const strings = {
  en: {
    // Navigation
    topStories: "Top Stories",
    blindspots: "Blindspots",
    filQosor: "Fil-Qosor",
    settings: "Settings",
    discoverTitle: "Discover Latest",
    discoverTitle2: "News",

    // Feed
    refresh: "Refresh",
    refreshing: "Refreshing...",
    seeMore: "See More",
    ago: "ago",
    sources: "Sources",
    source: "Source",
    blindspot: "Blindspot",

    // Categories
    catAll: "All",
    catPolitics: "Politics",
    catSport: "Sport",
    catLocal: "Local",
    catInternational: "World",
    catCrime: "Crime",
    catBusiness: "Business",
    catOpinion: "Opinion",
    catEntertainment: "Entertainment",
    catGeneral: "General",

    // Detail
    loadingArticle: "Loading article\u2026",
    noBodyText: "Full article text not available",
    readOn: "Read on",
    viewAllSources: "View all {n} sources",

    // Empty / error states
    noStories: "No stories yet",
    fetchingNews: "Fetching the latest news...",
    loadError: "Couldn't load the feed",
    loadErrorSub: "Check your connection and try again.",
    tryAgain: "Try Again",

    // Fil-Qosor
    filQosorTitle: "In Brief",
    filQosorSub: "Quick summaries of today's top stories.",
    noSummaries: "No summaries yet",
    noSummariesSub: "Refresh to load the latest news.",

    // Radar
    coverageBlindspots: "Coverage Blindspots",
    coverageBlindspotsSub: "Stories covered only by partisan outlets, missed by independent media.",
    noBlindspots: "No blindspots detected",
    noBlindspotsSub: "All stories have independent coverage.",

    // Stats
    readingStats: "Reading Stats",
    readingStatsSub: "Your reading habits and bias exposure will appear here.",

    // Sources settings
    sourcesLocal: "Local Sources",
    sourcesGlobal: "Global Sources",
    sourcesDesc: "Choose which outlets appear in each tab.",

    // Settings
    appearance: "Appearance",
    system: "System",
    light: "Light",
    dark: "Dark",
    feedLanguage: "Feed Language",

    // Tabs
    tabFeed: "Feed",
    tabSaved: "Saved",
    tabBlindspots: "Blindspots",
    tabLocal: "Local",
    tabGlobal: "Global",

    // Saved
    save: "Save",
    unsave: "Saved",
    noSaved: "No saved stories",
    noSavedSub: "Tap the bookmark on any story to save it.",

    // Source errors
    sourcesFailed: "{n} source(s) unavailable",

    // Add source form
    addSource: "Add Source",
    addingSource: "Adding…",
    addSourceUrl: "Website or RSS feed URL",
    addSourceName: "Name (optional — auto-detected)",
    addMaltaSource: "Add a Malta Source",
    addInternationalSource: "Add an International Source",
    addSourceError: "Could not add source",

    // Bias labels (full)
    biasState: "State Media",
    biasPl: "Labour (PL)",
    biasPn: "Nationalist (PN)",
    biasChurch: "Church",
    biasIndependent: "Independent",
    biasInvestigative: "Investigative",
    // New-since-last-visit badge
    newBadge: "New",

    // Story timeline
    storyTimeline: "Timeline",
    brokeTheStory: "broke it",

    // Compare framing
    compareFraming: "Compare Framing",
    labourSays: "PL",
    nationalistSays: "PN",

    // Reading experience
    minRead: "min read",
    fontSize: "Text Size",

    // Publisher article count (Settings)
    articlesToday: "articles",

    // Cluster management
    splitFromCluster: "Remove",
    forceRecluster: "Re-cluster Feed",
    reclustering: "Re-clustering\u2026",
    wipeAllData: "Wipe All Data",
    wipingData: "Wiping\u2026",
    wipeAllDataConfirm: "Are you sure? This will delete all articles and cannot be undone.",

    // Bias labels (short — used in bar legend)
    biasStateShort: "State",
    biasPlShort: "PL",
    biasPnShort: "PN",
    biasChurchShort: "Church",
    biasIndependentShort: "Indep.",
    biasInvestigativeShort: "Invest.",
    biasLeft: "Left",
    biasLeftShort: "Left",
    biasCentre: "Centre",
    biasCentreShort: "Centre",
    biasRight: "Right",
    biasRightShort: "Right",
  },
  mt: {
    topStories: "L-Aħbarijiet",
    blindspots: "Punti Mudlama",
    filQosor: "Fil-Qosor",
    settings: "Settings",
    discoverTitle: "Skopri l-Aħħar",
    discoverTitle2: "Aħbarijiet",

    refresh: "Aġġorna",
    refreshing: "Qed jaġġorna...",
    seeMore: "Ara Iktar",
    ago: "ilu",
    sources: "Sorsi",
    source: "Sors",
    blindspot: "Punt Mudlam",

    catAll: "Kollha",
    catPolitics: "Politika",
    catSport: "Sport",
    catLocal: "Lokali",
    catInternational: "Dinja",
    catCrime: "Kriminalit\u00e0",
    catBusiness: "Negozju",
    catOpinion: "Opinjoni",
    catEntertainment: "Divertiment",
    catGeneral: "\u0120enerali",

    loadingArticle: "Qed jitgħabba l-artiklu\u2026",
    noBodyText: "It-test tal-artiklu mhux disponibbli",
    readOn: "Aqra fuq",
    viewAllSources: "Ara s-{n} sorsi kollha",

    noStories: "Għad m'hawn aħbarijiet",
    fetchingNews: "Qed infittxu l-aħħar aħbarijiet...",
    loadError: "Ma stajtx tagħbija l-feed",
    loadErrorSub: "Iċċekkja l-konnessjoni u erġa' pprova.",
    tryAgain: "Erġa' Pprova",

    filQosorTitle: "Fil-Qosor",
    filQosorSub: "Sommarju ta' l-aħbarijiet tal-lum.",
    noSummaries: "Għad m'hawn sommarji",
    noSummariesSub: "Aġġorna biex tara l-aħħar aħbarijiet.",

    coverageBlindspots: "Punti Mudlama fil-Kopertura",
    coverageBlindspotsSub: "Storji koperti biss minn mezzi partiġġjani, mhux koperti minn mezzi indipendenti.",
    noBlindspots: "L-ebda punti mudlama",
    noBlindspotsSub: "L-istejjer kollha għandhom kopertura indipendenti.",

    readingStats: "Statistika tal-Qari",
    readingStatsSub: "Il-ħajja tal-qari u l-espożizzjoni għall-biża tiegħek se jidhru hawn.",

    sourcesLocal: "Sorsi Lokali",
    sourcesGlobal: "Sorsi Globali",
    sourcesDesc: "Agħżel liema sorsi jidhru f'kull tab.",

    appearance: "Dehra",
    system: "Sistema",
    light: "Ċar",
    dark: "Skur",
    feedLanguage: "Lingwa tal-Feed",

    tabFeed: "Feed",
    tabSaved: "Salvati",
    tabBlindspots: "Punt Mudlam",
    tabLocal: "Lokali",
    tabGlobal: "Globali",

    save: "Salva",
    unsave: "Salvat",
    noSaved: "L-ebda storji salvati",
    noSavedSub: "Agħfas il-bookmark fuq storja biex issalvaha.",

    sourcesFailed: "{n} sors/sorsi mhux disponibbli",

    addSource: "Żid Sors",
    addingSource: "Qed iżżid...",
    addSourceUrl: "URL tas-sit jew RSS feed",
    addSourceName: "Isem (mhux obbligatorju — jiġi skoprit)",
    addMaltaSource: "Żid Sors Malti",
    addInternationalSource: "Żid Sors Internazzjonali",
    addSourceError: "Ma setax iżżid is-sors",

    biasState: "Midja tal-Istat",
    biasPl: "Laburisti (PL)",
    biasPn: "Nazzjonalisti (PN)",
    biasChurch: "Knisja",
    biasIndependent: "Indipendenti",
    biasInvestigative: "Investigattiv",
    newBadge: "Ġdid",
    storyTimeline: "Kronolġija",
    brokeTheStory: "irrapporta l-ewwel",
    compareFraming: "Qabbel",
    labourSays: "PL",
    nationalistSays: "PN",
    minRead: "min qari",
    fontSize: "Daqs tat-Test",
    articlesToday: "artikli",

    splitFromCluster: "Neħħi",
    forceRecluster: "Erġa' Raggruppa",
    reclustering: "Qed jirraggera\u2026",
    wipeAllData: "Ħassar Kollox",
    wipingData: "Qed iħassar\u2026",
    wipeAllDataConfirm: "Ċert? Dan se jħassar l-artikli kollha u ma jistax jiġi revokat.",

    biasStateShort: "Stat",
    biasPlShort: "PL",
    biasPnShort: "PN",
    biasChurchShort: "Knisja",
    biasIndependentShort: "Indep.",
    biasInvestigativeShort: "Invest.",
    biasLeft: "Xellug",
    biasLeftShort: "Xellug",
    biasCentre: "Ċentru",
    biasCentreShort: "Ċentru",
    biasRight: "Lemin",
    biasRightShort: "Lemin",
  },
} as const;

export type LangKey = keyof typeof strings.en;

export function t(lang: "en" | "mt", key: LangKey): string {
  return strings[lang][key] ?? strings.en[key];
}
