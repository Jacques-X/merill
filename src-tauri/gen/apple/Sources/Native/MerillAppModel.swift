import Foundation
import SwiftUI

@MainActor
final class MerillAppModel: ObservableObject {
    @Published private(set) var clusters: [StoryCluster] = []
    @Published private(set) var publishers: [Publisher] = []
    @Published private(set) var failedSources: [String] = []
    @Published var isLoading = false
    @Published var isRefreshing = false
    @Published var errorMessage: String?
    @Published var successMessage: String?
    @Published var scope: FeedScope {
        didSet { defaults.set(scope.rawValue, forKey: Keys.scope) }
    }
    @Published var savedClusterIds: Set<String> {
        didSet { defaults.set(Array(savedClusterIds), forKey: Keys.saved) }
    }
    @Published var disabledLocalPublisherIds: Set<String> {
        didSet { defaults.set(Array(disabledLocalPublisherIds), forKey: Keys.disabledLocal) }
    }
    @Published var biasOverrides: [String: BiasCategory] {
        didSet { defaults.set(biasOverrides.mapValues(\.rawValue), forKey: Keys.biasOverrides) }
    }
    @Published var readerScale: ReaderScale {
        didSet { defaults.set(readerScale.rawValue, forKey: Keys.readerScale) }
    }
    @Published var language: String {
        didSet { defaults.set(language, forKey: Keys.language) }
    }
    @Published private var feedOrderSeed = UInt64(Date().timeIntervalSince1970)

    private let defaults = UserDefaults.standard
    private let client = RustClient.shared
    private var translationCache: [String: String] = [:]

    private enum Keys {
        static let scope = "merill.native.feedScope"
        static let saved = "merill.native.savedClusterIds"
        static let disabledLocal = "merill.native.disabledLocalPublisherIds"
        static let biasOverrides = "merill.native.biasOverrides"
        static let readerScale = "merill.native.readerScale"
        static let language = "merill.native.language"
        static let clusteringVersion = "merill.native.clusteringVersion"
    }

    private static let currentClusteringVersion = 2

    init() {
        scope = FeedScope(rawValue: defaults.string(forKey: Keys.scope) ?? "") ?? .local
        savedClusterIds = Set(defaults.stringArray(forKey: Keys.saved) ?? [])
        disabledLocalPublisherIds = Set(defaults.stringArray(forKey: Keys.disabledLocal) ?? [])
        let storedBiases = defaults.dictionary(forKey: Keys.biasOverrides) as? [String: String] ?? [:]
        biasOverrides = storedBiases.compactMapValues(BiasCategory.init(rawValue:))
        readerScale = ReaderScale(rawValue: defaults.string(forKey: Keys.readerScale) ?? "") ?? .medium
        language = defaults.string(forKey: Keys.language) ?? "en"
    }

    func start() async {
        isLoading = true
        defer { isLoading = false }
        do {
            try await reload()
            if clusters.isEmpty {
                try await refresh()
                defaults.set(Self.currentClusteringVersion, forKey: Keys.clusteringVersion)
            } else if defaults.integer(forKey: Keys.clusteringVersion) < Self.currentClusteringVersion {
                let _: String = try await client.call("force_recluster")
                defaults.set(Self.currentClusteringVersion, forKey: Keys.clusteringVersion)
                try await reload()
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func reload() async throws {
        async let clusterResponse: ClustersResponse = client.call("get_clusters", payload: ["blindspots_only": false])
        async let publisherResponse: [Publisher] = client.call("get_publishers")
        clusters = try await clusterResponse.clusters
        publishers = try await publisherResponse
    }

    func refresh() async throws {
        guard !isRefreshing else { return }
        isRefreshing = true
        defer { isRefreshing = false }
        do {
            let result: RefreshResult = try await client.call("refresh_feed")
            failedSources = result.failedSources
            try await reload()
            feedOrderSeed &+= 1
        } catch {
            errorMessage = error.localizedDescription
            throw error
        }
    }

    func clusters(for tab: RootTab, topic: String?) -> [StoryCluster] {
        let filtered = clusters
            .filter { cluster in
                switch tab {
                case .feed: return true
                case .blindspots: return cluster.isBlindspot
                case .saved: return savedClusterIds.contains(cluster.id)
                case .settings: return false
                }
            }
            .filter { cluster in
                cluster.articles.contains { article in
                    scope == .global ? article.publisher.isGlobal : !article.publisher.isGlobal
                }
            }
            .filter { cluster in
                !cluster.articles.allSatisfy { disabledLocalPublisherIds.contains($0.publisherId) }
            }
            .filter { cluster in
                guard let topic else { return true }
                return cluster.articles.contains { $0.category == topic }
            }

        if tab == .feed {
            return filtered.sorted {
                balancedFeedScore(for: $0) > balancedFeedScore(for: $1)
            }
        }

        return filtered.sorted { $0.lastUpdatedDate > $1.lastUpdatedDate }
    }

    private func balancedFeedScore(for cluster: StoryCluster) -> Double {
        let publisherCount = Set(cluster.articles.map(\.publisherId)).count
        let coverage = min(Double(publisherCount), 4) / 4
        let ageHours = max(0, -cluster.lastUpdatedDate.timeIntervalSinceNow / 3_600)
        let freshness = 1 - min(ageHours, 72) / 72
        return coverage * 0.5 + freshness * 0.75 + stableFeedRandom(for: cluster.id) * 1.5
    }

    private func stableFeedRandom(for clusterID: String) -> Double {
        var hash: UInt64 = 1_469_598_103_934_665_603 ^ feedOrderSeed
        for byte in clusterID.utf8 {
            hash ^= UInt64(byte)
            hash &*= 1_099_511_628_211
        }
        return Double(hash % 10_000) / 10_000
    }

    func toggleSaved(_ cluster: StoryCluster) {
        if savedClusterIds.contains(cluster.id) {
            savedClusterIds.remove(cluster.id)
        } else {
            savedClusterIds.insert(cluster.id)
        }
    }

    func isSaved(_ cluster: StoryCluster) -> Bool {
        savedClusterIds.contains(cluster.id)
    }

    func isPublisherEnabled(_ publisher: Publisher) -> Bool {
        !disabledLocalPublisherIds.contains(publisher.id)
    }

    func togglePublisher(_ publisher: Publisher) {
        if disabledLocalPublisherIds.contains(publisher.id) {
            disabledLocalPublisherIds.remove(publisher.id)
        } else {
            disabledLocalPublisherIds.insert(publisher.id)
        }
    }

    func bias(for publisher: Publisher) -> BiasCategory {
        biasOverrides[publisher.id] ?? publisher.biasCategory
    }

    func fetchBody(for article: Article) async throws -> ArticleBody {
        try await client.call("fetch_article_body", payload: [
            "article_id": article.id,
            "url": article.originalUrl,
        ])
    }

    func translate(_ text: String, from: String, to: String) async throws -> String {
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, from != to else {
            return text
        }
        let key = "\(from)|\(to)|\(text)"
        if let cached = translationCache[key] {
            return cached
        }
        let translated: String = try await client.call("translate_summary", payload: [
            "text": text,
            "to": to,
        ])
        translationCache[key] = translated
        return translated
    }

    func split(_ article: Article) async throws {
        let _: String = try await client.call("split_cluster", payload: [
            "article_id": article.id,
            "headline": article.displayHeadline,
            "published_at": article.publishedAt,
        ])
        try await reload()
    }

    func addPublisher(url: String, name: String, isGlobal: Bool) async throws {
        let _: Publisher = try await client.call("add_custom_publisher", payload: [
            "url": url,
            "name": name,
            "is_global": isGlobal,
        ])
        publishers = try await client.call("get_publishers")
        successMessage = L10n.text(language, "Source added", "Is-sors ġie miżjud")
    }

    func removePublisher(_ publisher: Publisher) async throws {
        try await client.callVoid("remove_custom_publisher", payload: ["id": publisher.id])
        try await reload()
        successMessage = L10n.text(language, "Source removed", "Is-sors tneħħa")
    }

    func forceRecluster() async throws {
        let _: String = try await client.call("force_recluster")
        try await reload()
        defaults.set(Self.currentClusteringVersion, forKey: Keys.clusteringVersion)
        successMessage = L10n.text(language, "Stories re-clustered", "L-istejjer reġgħu ġew raggruppati")
    }

    func wipe() async throws {
        try await client.callVoid("wipe_all_data")
        try await reload()
        successMessage = L10n.text(language, "Local news data cleared", "Id-dejta lokali tal-aħbarijiet tħassret")
    }
}
