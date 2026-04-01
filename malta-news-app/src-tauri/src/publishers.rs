use std::collections::HashMap;
use std::sync::LazyLock;

use crate::models::{BiasCategory, PublisherDef, PublisherInfo, ScrapeMethod};

static PUBLISHER_DEFS: &[PublisherDef] = &[
    // ── RSS-based ───────────────────────────────────────────────────────────
    PublisherDef {
        id: "newsbook",
        name: "Newsbook",
        bias_category: BiasCategory::ChurchOwned,
        primary_language: "en",
        logo_url: "https://newsbook.com.mt/favicon.png",
        scrape: ScrapeMethod::Rss {
            url: "https://newsbook.com.mt/en/feed/",
        },
    },
    PublisherDef {
        id: "lovin_malta",
        name: "Lovin Malta",
        bias_category: BiasCategory::CommercialIndependent,
        primary_language: "en",
        logo_url: "https://lovinmalta.com/wp-content/uploads/2022/09/cropped-fav2022-180x180.png",
        scrape: ScrapeMethod::Rss {
            url: "https://lovinmalta.com/feed/",
        },
    },
    PublisherDef {
        id: "the_shift",
        name: "The Shift News",
        bias_category: BiasCategory::InvestigativeIndependent,
        primary_language: "en",
        logo_url: "https://theshiftnews.com/wp-content/uploads/2024/09/favicon.png",
        scrape: ScrapeMethod::Rss {
            url: "https://theshiftnews.com/feed/",
        },
    },
    // ── HTML-scraped ────────────────────────────────────────────────────────
    PublisherDef {
        id: "malta_today",
        name: "Malta Today",
        bias_category: BiasCategory::CommercialIndependent,
        primary_language: "en",
        logo_url: "https://www.maltatoday.com.mt/ui/images/icons/Icon-60@3x.png",
        scrape: ScrapeMethod::Html {
            url: "https://www.maltatoday.com.mt/news",
            article_sel: "div.news-article[data-url]",
            headline_sel: "h3",
            image_sel: "img[src]",
            link_attr: "data-url",
            base_url: "https://www.maltatoday.com.mt",
        },
    },
    PublisherDef {
        id: "independent",
        name: "The Malta Independent",
        bias_category: BiasCategory::CommercialIndependent,
        primary_language: "en",
        logo_url: "https://www.independent.com.mt/favicon.ico",
        scrape: ScrapeMethod::Html {
            url: "https://www.independent.com.mt/",
            article_sel: "article.entry-wrapper a[href]",
            headline_sel: "h2",
            image_sel: "img",
            link_attr: "href",
            base_url: "https://www.independent.com.mt",
        },
    },
    PublisherDef {
        id: "tvm_news",
        name: "TVM News",
        bias_category: BiasCategory::StateOwned,
        primary_language: "en",
        logo_url: "https://tvmnews.mt/favicon.ico",
        scrape: ScrapeMethod::Html {
            url: "https://tvmnews.mt/en/",
            article_sel: "h3.magcat-titlte a[href]",
            headline_sel: "",
            image_sel: "",
            link_attr: "href",
            base_url: "https://tvmnews.mt",
        },
    },
    // ── Maltese-language ────────────────────────────────────────────────────
    PublisherDef {
        id: "one_news",
        name: "ONE News",
        bias_category: BiasCategory::PartyOwnedPl,
        primary_language: "mt",
        logo_url: "https://one.com.mt/wp-content/themes/soledad-child/images/favicon-180.png",
        scrape: ScrapeMethod::Rss {
            url: "https://one.com.mt/feed/",
        },
    },
    PublisherDef {
        id: "net_news",
        name: "NET News",
        bias_category: BiasCategory::PartyOwnedPn,
        primary_language: "mt",
        logo_url: "https://netnews.com.mt/wp-content/themes/netnews/assets/img/logo192x192.png",
        scrape: ScrapeMethod::Html {
            url: "https://netnews.com.mt/",
            article_sel: ".entry-title a[href]",
            headline_sel: "",
            image_sel: "",
            link_attr: "href",
            base_url: "https://netnews.com.mt",
        },
    },
    // ── Sitemap-based ──────────────────────────────────────────────────────
    PublisherDef {
        id: "times_of_malta",
        name: "Times of Malta",
        bias_category: BiasCategory::CommercialIndependent,
        primary_language: "en",
        logo_url: "https://timesofmalta.com/apple-touch-icon.png",
        scrape: ScrapeMethod::Sitemap {
            url: "https://timesofmalta.com/sitemap_latest.xml",
        },
    },
];

pub static PUBLISHERS: LazyLock<HashMap<&'static str, &'static PublisherDef>> =
    LazyLock::new(|| PUBLISHER_DEFS.iter().map(|p| (p.id, p)).collect());

pub fn all_publisher_defs() -> &'static [PublisherDef] {
    PUBLISHER_DEFS
}

pub fn publisher_info(id: &str) -> PublisherInfo {
    match PUBLISHERS.get(id) {
        Some(p) => PublisherInfo {
            id: p.id.to_string(),
            name: p.name.to_string(),
            bias_category: p.bias_category,
            logo_url: p.logo_url.to_string(),
            is_global: false, // all static publishers are Malta-local
        },
        None => PublisherInfo {
            id: id.to_string(),
            name: id.to_string(),
            bias_category: BiasCategory::CommercialIndependent,
            logo_url: String::new(),
            is_global: false,
        },
    }
}
