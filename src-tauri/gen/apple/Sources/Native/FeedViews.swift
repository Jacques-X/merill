import SwiftUI

struct FeedView: View {
    @EnvironmentObject private var model: MerillAppModel
    @Environment(\.merillLanguage) private var language
    let tab: RootTab
    @State private var topic: String?

    var body: some View {
        NavigationStack {
            Group {
                if model.isLoading {
                    EditorialSkeletonList()
                } else if let error = model.errorMessage, model.clusters.isEmpty {
                    MerillEmptyState(title: L10n.text(language, "Could not load Merill", "Merill ma setax jitgħabba"), symbol: "exclamationmark.triangle", description: error) {
                        Button(L10n.text(language, "Try Again", "Erġa' Pprova")) { Task { await model.start() } }
                    }
                } else if visibleClusters.isEmpty {
                    emptyState
                } else {
                    editorialFeed
                }
            }
            .navigationTitle(tab.title(language))
            .searchable(text: $model.searchText, prompt: L10n.text(language, "Search stories", "Fittex stejjer"))
            .onChange(of: model.searchText) { _ in
                Task { await model.search() }
            }
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Menu {
                        sortButton(.balanced, "For You", "Għalik")
                        sortButton(.latest, "Latest", "L-Aħħar")
                        sortButton(.covered, "Most covered", "L-aktar koperti")
                        sortButton(.blindspots, "Blindspots", "Punti mudlama")
                    } label: {
                        Image(systemName: "arrow.up.arrow.down")
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func sortButton(_ sort: FeedSort, _ english: String, _ maltese: String) -> some View {
        Button {
            model.feedSort = sort
        } label: {
            if model.feedSort == sort {
                Label(L10n.text(language, english, maltese), systemImage: "checkmark")
            } else {
                Text(L10n.text(language, english, maltese))
            }
        }
    }

    private var visibleClusters: [StoryCluster] {
        model.clusters(for: tab, topic: topic)
    }

    private var editorialFeed: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 18) {
                if !model.failedSources.isEmpty {
                    DisclosureGroup {
                        ForEach(model.failedSources, id: \.self) { source in
                            Text(source).font(.caption)
                        }
                        Button(L10n.text(language, "Retry sources", "Erġa' pprova s-sorsi")) {
                            Task { try? await model.refresh() }
                        }
                        .font(.footnote.weight(.semibold))
                    } label: {
                        Label(
                            L10n.count(
                                language,
                                model.failedSources.count,
                                english: "%d sources could not refresh",
                                maltese: "%d sorsi ma setgħux jiġu aġġornati"
                            ),
                            systemImage: "exclamationmark.triangle.fill"
                        )
                        .font(.footnote.weight(.medium))
                        .foregroundStyle(.orange)
                    }
                    .padding(.horizontal)
                }
                if let last = model.lastRefreshAt {
                    Text("\(L10n.text(language, "Updated", "Aġġornat")) \(last, style: .relative)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal)
                }
                ScopePicker(scope: $model.scope)
                    .padding(.horizontal)
                TopicStrip(topic: $topic)
                ForEach(visibleClusters) { cluster in
                    NavigationLink {
                        StoryGroupView(cluster: cluster)
                    } label: {
                        EditorialStoryCard(cluster: cluster)
                    }
                    .buttonStyle(.plain)
                    .contextMenu {
                        Button {
                            model.toggleSaved(cluster)
                        } label: {
                            Label(
                                model.isSaved(cluster)
                                    ? L10n.text(language, "Remove Bookmark", "Neħħi minn Salvati")
                                    : L10n.text(language, "Save Story", "Salva l-Istorja"),
                                systemImage: model.isSaved(cluster) ? "bookmark.slash" : "bookmark"
                            )
                        }
                    }
                    .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                        Button {
                            model.toggleSaved(cluster)
                        } label: {
                            Label(
                                model.isSaved(cluster)
                                    ? L10n.text(language, "Unsave", "Neħħi")
                                    : L10n.text(language, "Save", "Salva"),
                                systemImage: model.isSaved(cluster) ? "bookmark.slash" : "bookmark"
                            )
                        }
                        .tint(.blue)
                    }
                }
            }
            .padding(.vertical, 12)
        }
        .refreshable {
            try? await model.refresh()
        }
    }

    private var emptyState: some View {
        VStack(spacing: 20) {
            ScopePicker(scope: $model.scope)
                .padding(.horizontal)
            MerillEmptyState(title: emptyTitle, symbol: emptySymbol, description: emptyDescription) {
                if tab == .feed {
                    Button(L10n.text(language, "Refresh", "Aġġorna")) { Task { try? await model.refresh() } }
                }
            }
        }
    }

    private var emptyTitle: String {
        switch tab {
        case .feed: return L10n.text(language, "No stories yet", "Għad m'hemmx stejjer")
        case .blindspots: return L10n.text(language, "No blindspots right now", "Bħalissa m'hemmx punti mudlama")
        case .saved: return L10n.text(language, "No saved stories", "M'hemmx stejjer salvati")
        case .settings: return ""
        }
    }

    private var emptyDescription: String {
        switch tab {
        case .feed: return L10n.text(language, "Pull to refresh or enable more sources in Settings.", "Iġbed biex taġġorna jew ixgħel aktar sorsi fis-Settings.")
        case .blindspots: return L10n.text(language, "Stories missing independent coverage will appear here.", "Stejjer mingħajr kopertura indipendenti jidhru hawn.")
        case .saved: return L10n.text(language, "Bookmark stories to return to them later.", "Salva stejjer biex terġa' ssibhom aktar tard.")
        case .settings: return ""
        }
    }

    private var emptySymbol: String {
        switch tab {
        case .feed: return "newspaper"
        case .blindspots: return "eye"
        case .saved: return "bookmark"
        case .settings: return "gearshape"
        }
    }
}

struct ScopePicker: View {
    @Environment(\.merillLanguage) private var language
    @Binding var scope: FeedScope

    var body: some View {
        Picker(L10n.text(language, "Feed scope", "Firxa tal-aħbarijiet"), selection: $scope) {
            Text(L10n.text(language, "Local", "Lokali")).tag(FeedScope.local)
            Text(L10n.text(language, "Global", "Globali")).tag(FeedScope.global)
        }
        .pickerStyle(.segmented)
    }
}

struct TopicStrip: View {
    @Environment(\.merillLanguage) private var language
    @Binding var topic: String?
    private var topics: [(String, String?)] {
        [
            (L10n.text(language, "All", "Kollha"), nil),
            (L10n.text(language, "Politics", "Politika"), "politics"),
            (L10n.text(language, "Local", "Lokali"), "local"),
            (L10n.text(language, "Business", "Negozju"), "business"),
            (L10n.text(language, "Sport", "Sport"), "sport"),
        ]
    }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(topics, id: \.0) { title, value in
                    Button(title) { topic = value }
                        .font(.footnote.weight(.semibold))
                        .buttonStyle(.bordered)
                        .tint(topic == value ? .accentColor : .secondary)
                }
            }
            .padding(.horizontal)
        }
    }
}

struct EditorialStoryCard: View {
    @EnvironmentObject private var model: MerillAppModel
    @Environment(\.merillLanguage) private var language
    let cluster: StoryCluster

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let imageUrl = cluster.heroImageUrl {
                HeroImage(url: imageUrl)
            }
            TranslatedText(
                original: cluster.headline(language: "en"),
                sourceLanguage: "en",
                existingTranslation: cluster.headline(language: "mt")
            )
                .font(.title3.weight(.bold))
                .foregroundStyle(.primary)
                .multilineTextAlignment(.leading)
            if !cluster.displaySummary.isEmpty {
                TranslatedText(
                    original: cluster.displaySummary,
                    sourceLanguage: "en"
                )
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
                    .multilineTextAlignment(.leading)
            }
            BiasCoverageBar(articles: cluster.articles)
            if cluster.blindspotExplanation.missingIndependentCoverage {
                Label(
                    L10n.text(language, "No independent coverage found", "Ma nstabitx kopertura indipendenti"),
                    systemImage: "eye.slash"
                )
                .font(.caption.weight(.semibold))
                .foregroundStyle(.orange)
            }
            HStack {
                PublisherAvatars(articles: cluster.articles)
                Spacer()
                Text(cluster.lastUpdatedDate, style: .relative)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Image(systemName: model.isSaved(cluster) ? "bookmark.fill" : "bookmark")
                    .foregroundStyle(model.isSaved(cluster) ? Color.accentColor : .secondary)
            }
        }
        .padding(.horizontal)
        .padding(.bottom, 4)
        .contentShape(Rectangle())
    }
}

struct HeroImage: View {
    let url: String?

    var body: some View {
        if let url, let parsed = URL(string: url) {
            AsyncImage(url: parsed) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .scaledToFill()
                        .frame(height: 210)
                        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                case .empty:
                    ProgressView()
                        .frame(maxWidth: .infinity)
                        .frame(height: 210)
                case .failure:
                    EmptyView()
                @unknown default:
                    EmptyView()
                }
            }
        }
    }
}

struct PublisherAvatars: View {
    let articles: [Article]

    var body: some View {
        HStack(spacing: -7) {
            ForEach(Array(articles.prefix(4))) { article in
                PublisherAvatar(publisher: article.publisher)
                    .frame(width: 28, height: 28)
            }
            if articles.count > 4 {
                Text("+\(articles.count - 4)")
                    .font(.caption2.weight(.bold))
                    .frame(width: 28, height: 28)
                    .background(.thinMaterial, in: Circle())
            }
        }
    }
}

struct PublisherAvatar: View {
    @EnvironmentObject private var model: MerillAppModel
    let publisher: Publisher

    var body: some View {
        ZStack {
            Circle().fill(model.bias(for: publisher).color)
            if let url = URL(string: publisher.logoUrl), !publisher.logoUrl.isEmpty {
                AsyncImage(url: url) { phase in
                    if case .success(let image) = phase {
                        image.resizable().scaledToFit().clipShape(Circle()).padding(2)
                    } else {
                        initials
                    }
                }
            } else {
                initials
            }
        }
        .overlay { Circle().stroke(.background, lineWidth: 2) }
        .accessibilityLabel(publisher.name)
    }

    private var initials: some View {
        Text(String(publisher.name.prefix(2)).uppercased())
            .font(.caption2.weight(.bold))
            .foregroundStyle(.white)
    }
}

struct BiasCoverageBar: View {
    @Environment(\.merillLanguage) private var language
    let articles: [Article]

    var body: some View {
        GeometryReader { proxy in
            HStack(spacing: 2) {
                ForEach(Array(grouped.enumerated()), id: \.offset) { _, item in
                    Capsule()
                        .fill(item.category.color)
                        .frame(width: max(4, proxy.size.width * CGFloat(item.count) / CGFloat(max(articles.count, 1))))
                }
            }
        }
        .frame(height: 5)
        .accessibilityLabel(
            L10n.count(language, articles.count, english: "Coverage across %d sources", maltese: "Kopertura minn %d sorsi")
        )
    }

    private var grouped: [(category: BiasCategory, count: Int)] {
        Dictionary(grouping: articles, by: \.publisher.biasCategory)
            .map { ($0.key, $0.value.count) }
            .sorted { $0.category.rawValue < $1.category.rawValue }
    }
}

struct EditorialSkeletonList: View {
    var body: some View {
        ScrollView {
            LazyVStack(spacing: 22) {
                ForEach(0..<3, id: \.self) { _ in
                    VStack(alignment: .leading, spacing: 12) {
                        RoundedRectangle(cornerRadius: 14).fill(.secondary.opacity(0.12)).frame(height: 210)
                        RoundedRectangle(cornerRadius: 4).fill(.secondary.opacity(0.12)).frame(height: 20)
                        RoundedRectangle(cornerRadius: 4).fill(.secondary.opacity(0.09)).frame(width: 260, height: 16)
                    }
                    .padding(.horizontal)
                }
            }
            .padding(.vertical)
        }
    }
}
