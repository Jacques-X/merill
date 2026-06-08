import Foundation
import SwiftUI

enum BiasCategory: String, Codable, CaseIterable {
    case stateOwned = "state_owned"
    case partyOwnedPL = "party_owned_pl"
    case partyOwnedPN = "party_owned_pn"
    case churchOwned = "church_owned"
    case commercialIndependent = "commercial_independent"
    case investigativeIndependent = "investigative_independent"
    case left
    case centre
    case right

    var color: Color {
        switch self {
        case .stateOwned: return .purple
        case .partyOwnedPL: return .red
        case .partyOwnedPN: return .blue
        case .churchOwned: return .orange
        case .commercialIndependent: return .mint
        case .investigativeIndependent: return .teal
        case .left: return .red
        case .centre: return .gray
        case .right: return .blue
        }
    }
}

struct Publisher: Codable, Identifiable, Hashable {
    let id: String
    let name: String
    let biasCategory: BiasCategory
    let logoUrl: String
    let isGlobal: Bool
}

struct Article: Codable, Identifiable, Hashable {
    let id: String
    let publisherId: String
    let publisher: Publisher
    let originalUrl: String
    let originalHeadline: String
    let translatedHeadline: String
    let snippet: String
    let bodyText: String
    let imageUrl: String
    let language: String
    let publishedAt: String
    let storyClusterId: String
    let category: String

    func headline(language: String) -> String {
        if language == self.language { return originalHeadline }
        return translatedHeadline.isEmpty ? originalHeadline : translatedHeadline
    }

    var displayHeadline: String {
        headline(language: "en")
    }

    var publishedDate: Date {
        ISO8601DateFormatter().date(from: publishedAt) ?? .distantPast
    }
}

struct StoryCluster: Codable, Identifiable, Hashable {
    let id: String
    let primaryHeadline: String
    let firstReportedAt: String
    let lastUpdated: String
    let isBlindspot: Bool
    let aiHeadline: String
    let aiSummary: String
    let articles: [Article]

    func headline(language: String) -> String {
        if language == "mt" {
            if let malteseArticle = articles.first(where: { $0.language == "mt" }) {
                return malteseArticle.originalHeadline
            }
            if let translated = articles.first(where: {
                $0.language == "en"
                    && !$0.translatedHeadline.isEmpty
                    && $0.translatedHeadline != $0.originalHeadline
            })?.translatedHeadline {
                return translated
            }
        }
        if !aiHeadline.isEmpty { return aiHeadline }
        return primaryHeadline.isEmpty ? (articles.first?.headline(language: language) ?? "") : primaryHeadline
    }

    var displayHeadline: String {
        headline(language: "en")
    }

    var displaySummary: String {
        if !aiSummary.isEmpty { return aiSummary }
        return articles.first(where: { !$0.snippet.isEmpty })?.snippet ?? ""
    }

    var heroImageUrl: String? {
        articles.lazy.map(\.imageUrl).first(where: { !$0.isEmpty })
    }

    var lastUpdatedDate: Date {
        ISO8601DateFormatter().date(from: lastUpdated) ?? .distantPast
    }
}

struct ClustersResponse: Codable {
    let clusters: [StoryCluster]
}

struct RefreshResult: Codable {
    let message: String
    let failedSources: [String]
}

struct ArticleBody: Codable {
    let bodyText: String
    let imageUrl: String
}

struct SummaryResult: Codable {
    let headline: String
    let summary: String
}

enum FeedScope: String, CaseIterable, Identifiable {
    case local
    case global
    var id: Self { self }
}

enum RootTab: String, CaseIterable, Identifiable {
    case feed
    case blindspots
    case saved
    case settings
    var id: Self { self }

    func title(_ language: String) -> String {
        switch self {
        case .feed: return L10n.text(language, "Feed", "Aħbarijiet")
        case .blindspots: return L10n.text(language, "Blindspots", "Punti Mudlama")
        case .saved: return L10n.text(language, "Saved", "Salvati")
        case .settings: return L10n.text(language, "Settings", "Settings")
        }
    }

    var symbol: String {
        switch self {
        case .feed: return "newspaper"
        case .blindspots: return "eye"
        case .saved: return "bookmark"
        case .settings: return "gearshape"
        }
    }
}

enum ReaderScale: String, CaseIterable {
    case small
    case medium
    case large

    var font: Font {
        switch self {
        case .small: return .body
        case .medium: return .title3
        case .large: return .title2
        }
    }
}

extension JSONDecoder {
    static var merill: JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }
}
