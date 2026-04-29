/// Classify an article into a category based on its URL path and headline.
///
/// Categories: politics, sport, local, international, crime, business, opinion, entertainment, general
pub fn classify(url: &str, headline: &str) -> &'static str {
    let lower_url = url.to_lowercase();
    let lower_hl = headline.to_lowercase();

    // URL path patterns (most reliable signal)
    let url_cats: &[(&[&str], &str)] = &[
        (&["/sport", "/sports", "/futbol", "/football", "/soccer", "/rugby", "/tennis", "/f1"], "sport"),
        (&["/politics", "/politika", "/parlament", "/parliament", "/election"], "politics"),
        (&["/crime", "/court", "/police", "/pulizija", "/qorti", "/law-courts"], "crime"),
        (&["/business", "/economy", "/finance", "/negozju", "/ekonomija", "/property", "/market"], "business"),
        (&["/opinion", "/editorial", "/blog", "/comment", "/columnist", "/letters"], "opinion"),
        (&["/entertainment", "/culture", "/lifestyle", "/arts", "/music", "/film", "/tv", "/divertiment"], "entertainment"),
        (&["/world", "/international", "/europe", "/global", "/foreign", "/dinja"], "international"),
        (&["/local", "/malta", "/gozo", "/national", "/lokali"], "local"),
    ];

    for (patterns, cat) in url_cats {
        for p in *patterns {
            if lower_url.contains(p) {
                return cat;
            }
        }
    }

    // Headline keyword fallback
    let hl_cats: &[(&[&str], &str)] = &[
        (&["football", "goal", "goalkeeper", "referee", "champions league", "premier league",
           "serie a", "futbol", "marathon", "athlete", "olympic", "fifa", "uefa",
           "boxing", "mfa", "hibernians", "valletta fc", "hamrun", "floriana", "birkirkara",
           "sliema wanderers"], "sport"),
        (&["minister", "parliament", "election", "vote", "opposition", "labour party",
           "nationalist party", "robert abela", "bernard grech", "metsola", "cabinet",
           "legislation", "bill", "act of parliament", "ministru", "parlament",
           "elezzjoni"], "politics"),
        (&["arrested", "charged", "murder", "theft", "robbery", "drug", "fraud",
           "magistrate", "arraigned", "prison", "sentence", "guilty", "victim",
           "assault", "trafficking", "arrest", "arrestat", "droga"], "crime"),
        (&["economy", "investment", "company", "shares", "bank", "profit", "revenue",
           "startup", "inflation", "gdp", "budget", "tax", "property", "developer",
           "planning authority", "ekonomija", "negozju"], "business"),
        (&["opinion", "editorial", "letter to", "i think", "commentary", "column",
           "view:", "analysis:"], "opinion"),
        (&["festival", "concert", "film", "movie", "theatre", "music", "singer",
           "actor", "actress", "celebrity", "tv show", "netflix", "exhibition",
           "carnival", "festa", "pjazza"], "entertainment"),
        (&["eu ", "european", "united nations", "nato", "ukraine", "russia", "china",
           "trump", "biden", "uk ", "italy", "libya", "brussels", "summit",
           "foreign affairs", "ewropa", "dinja"], "international"),
    ];

    for (keywords, cat) in hl_cats {
        for kw in *keywords {
            if lower_hl.contains(kw) {
                return cat;
            }
        }
    }

    "general"
}
