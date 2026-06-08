import SwiftUI

struct StoryGroupView: View {
    @EnvironmentObject private var model: MerillAppModel
    @Environment(\.merillLanguage) private var language
    let cluster: StoryCluster
    @State private var reviewOpen = false
    @State private var timelineOpen = false

    var body: some View {
        GeometryReader { viewport in
            let contentWidth = max(0, viewport.size.width - 32)
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    if let image = cluster.heroImageUrl {
                        HeroImage(url: image, category: cluster.articles.first?.category)
                            .frame(width: contentWidth, height: min(220, contentWidth * 9 / 16))
                            .clipped()
                            .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
                    }
                    TranslatedText(
                        original: cluster.headline(language: "en"),
                        sourceLanguage: "en",
                        existingTranslation: cluster.headline(language: "mt")
                    )
                        .font(.title.weight(.bold))
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(width: contentWidth, alignment: .leading)
                    if !cluster.displaySummary.isEmpty {
                        TranslatedText(
                            original: cluster.displaySummary,
                            sourceLanguage: "en"
                        )
                            .font(.body)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(width: contentWidth, alignment: .leading)
                    }
                    BiasCoverageBar(articles: cluster.articles)
                        .frame(height: 6)
                    Text(
                        L10n.count(
                            language,
                            cluster.articles.count,
                            english: "%d perspectives",
                            maltese: "%d perspettivi"
                        )
                    )
                        .font(.headline)
                    PerspectiveCarousel(articles: cluster.articles)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .clipped()
                    DisclosureGroup(L10n.text(language, "Story timeline", "Kronoloġija tal-istorja"), isExpanded: $timelineOpen) {
                        TimelineView(articles: cluster.articles)
                            .padding(.top, 8)
                    }
                    .font(.headline)
                }
                .frame(width: contentWidth, alignment: .leading)
                .padding(.leading, 16)
                .padding(.trailing, 16)
                .padding(.vertical, 16)
            }
            .frame(width: viewport.size.width)
        }
        .navigationTitle(L10n.text(language, "Story", "Storja"))
        .merillInlineNavigationTitle()
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    Button {
                        reviewOpen = true
                    } label: {
                        Label(L10n.text(language, "Review Grouping", "Ivverifika l-Grupp"), systemImage: "rectangle.3.group")
                    }
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
                } label: {
                    Image(systemName: "ellipsis")
                }
                .accessibilityLabel(L10n.text(language, "Story actions", "Azzjonijiet tal-istorja"))
            }
        }
        .sheet(isPresented: $reviewOpen) {
            ReviewGroupingSheet(cluster: cluster)
        }
    }
}

struct PerspectiveCarousel: View {
    let articles: [Article]

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            LazyHStack(alignment: .top, spacing: 12) {
                ForEach(articles) { article in
                    NavigationLink {
                        ArticleReaderView(article: article)
                    } label: {
                        PerspectiveCard(article: article)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }
}

struct PerspectiveCard: View {
    @Environment(\.merillLanguage) private var language
    let article: Article

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                PublisherAvatar(publisher: article.publisher)
                    .frame(width: 32, height: 32)
                Text(article.publisher.name)
                    .font(.caption.weight(.semibold))
                Spacer()
                Text(article.publishedDate, style: .relative)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            TranslatedText(
                original: article.originalHeadline,
                sourceLanguage: article.language,
                existingTranslation: article.translatedHeadline
            )
                .font(.headline)
                .foregroundStyle(.primary)
                .multilineTextAlignment(.leading)
            if !article.snippet.isEmpty {
                TranslatedText(
                    original: article.snippet,
                    sourceLanguage: article.language
                )
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(4)
                    .multilineTextAlignment(.leading)
            }
            Label(L10n.text(language, "Read article", "Aqra l-artiklu"), systemImage: "arrow.right")
                .font(.footnote.weight(.bold))
                .foregroundStyle(.tint)
        }
        .padding()
        .frame(width: 300, alignment: .leading)
        .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
}

struct TimelineView: View {
    let articles: [Article]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(articles.sorted { $0.publishedDate < $1.publishedDate }) { article in
                HStack(alignment: .top, spacing: 10) {
                    Circle()
                        .fill(article.publisher.biasCategory.color)
                        .frame(width: 9, height: 9)
                        .padding(.top, 4)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(article.publisher.name)
                            .font(.subheadline.weight(.semibold))
                        Text(article.publishedDate, style: .relative)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
    }
}

struct ReviewGroupingSheet: View {
    @Environment(\.dismiss) private var dismiss
    @EnvironmentObject private var model: MerillAppModel
    @Environment(\.merillLanguage) private var language
    let cluster: StoryCluster
    @State private var errorMessage: String?

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Label(confidence, systemImage: "rectangle.3.group")
                } header: {
                    Text(L10n.text(language, "Grouping confidence", "Kunfidenza fil-grupp"))
                } footer: {
                    Text(L10n.text(language, "Remove a source when it does not describe the same event.", "Neħħi sors meta ma jiddeskrivix l-istess ġrajja."))
                }
                Section(L10n.text(language, "Sources", "Sorsi")) {
                    ForEach(cluster.articles) { article in
                        VStack(alignment: .leading, spacing: 5) {
                            Text(article.publisher.name)
                                .font(.caption.weight(.semibold))
                            TranslatedText(
                                original: article.originalHeadline,
                                sourceLanguage: article.language,
                                existingTranslation: article.translatedHeadline
                            )
                                .font(.subheadline)
                        }
                        .swipeActions {
                            Button(role: .destructive) {
                                Task {
                                    do { try await model.split(article) }
                                    catch { errorMessage = error.localizedDescription }
                                }
                            } label: {
                                Label(L10n.text(language, "Remove", "Neħħi"), systemImage: "trash")
                            }
                        }
                    }
                }
            }
            .navigationTitle(L10n.text(language, "Review Grouping", "Ivverifika l-Grupp"))
            .merillInlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button(L10n.text(language, "Done", "Lest")) { dismiss() }
                }
            }
            .alert(L10n.text(language, "Could not update grouping", "Il-grupp ma setax jiġi aġġornat"), isPresented: Binding(
                get: { errorMessage != nil },
                set: { if !$0 { errorMessage = nil } }
            )) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(errorMessage ?? "")
            }
        }
        .presentationDetents([.medium, .large])
    }

    private var confidence: String {
        if cluster.articles.count <= 1 {
            return L10n.text(language, "Single source", "Sors wieħed")
        }
        let sharedWords = cluster.articles
            .map { Set($0.displayHeadline.lowercased().split(separator: " ").map(String.init)) }
            .reduce(nil as Set<String>?) { partial, words in partial.map { $0.intersection(words) } ?? words }
            .map(\.count) ?? 0
        if sharedWords >= 3 {
            return L10n.text(language, "Strong match", "Tqabbil qawwi")
        }
        if sharedWords >= 1 {
            return L10n.text(language, "Related coverage", "Kopertura relatata")
        }
        return L10n.text(language, "Review suggested", "Huwa ssuġġerit li tivverifika")
    }
}

struct ArticleReaderView: View {
    @EnvironmentObject private var model: MerillAppModel
    @Environment(\.openURL) private var openURL
    @Environment(\.merillLanguage) private var language
    let article: Article
    @State private var bodyText = ""
    @State private var localizedBody = ""
    @State private var loading = true
    @State private var translating = false
    @State private var errorMessage: String?

    var body: some View {
        GeometryReader { viewport in
            let contentWidth = max(0, viewport.size.width - 32)
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    if !article.imageUrl.isEmpty {
                        HeroImage(url: article.imageUrl, category: article.category)
                            .frame(width: contentWidth, height: min(220, contentWidth * 9 / 16))
                            .clipped()
                            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                    }
                    HStack {
                        PublisherAvatar(publisher: article.publisher)
                            .frame(width: 34, height: 34)
                        Text(article.publisher.name)
                            .font(.subheadline.weight(.semibold))
                        Spacer()
                        Text(article.publishedDate, style: .relative)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    TranslatedText(
                        original: article.originalHeadline,
                        sourceLanguage: article.language,
                        existingTranslation: article.translatedHeadline
                    )
                        .font(.title.weight(.bold))
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(width: contentWidth, alignment: .leading)
                    if loading || translating {
                        ProgressView(L10n.text(language, "Loading article", "Qed jitgħabba l-artiklu"))
                    } else if let errorMessage {
                        MerillEmptyState(
                            title: L10n.text(language, "Article text unavailable", "It-test tal-artiklu mhux disponibbli"),
                            symbol: "doc.text.magnifyingglass",
                            description: errorMessage
                        )
                    } else {
                        Text(localizedBody.isEmpty ? (bodyText.isEmpty ? article.snippet : bodyText) : localizedBody)
                            .font(model.readerScale.font)
                            .lineSpacing(5)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(width: contentWidth, alignment: .leading)
                            .textSelection(.enabled)
                    }
                }
                .frame(width: contentWidth, alignment: .leading)
                .padding(.leading, 16)
                .padding(.trailing, 16)
                .padding(.vertical, 16)
            }
            .frame(width: viewport.size.width)
        }
        .navigationTitle(article.publisher.name)
        .merillInlineNavigationTitle()
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button {
                    model.readerScale = model.readerScale == .small ? .small : (model.readerScale == .medium ? .small : .medium)
                } label: {
                    Image(systemName: "textformat.size.smaller")
                }
                .accessibilityLabel(L10n.text(language, "Decrease text size", "Naqqas id-daqs tat-test"))
                Button {
                    model.readerScale = model.readerScale == .large ? .large : (model.readerScale == .medium ? .large : .medium)
                } label: {
                    Image(systemName: "textformat.size.larger")
                }
                .accessibilityLabel(L10n.text(language, "Increase text size", "Kabbar id-daqs tat-test"))
            }
            if #available(iOS 26.0, macOS 26.0, *) {
                ToolbarSpacer(.fixed, placement: .primaryAction)
            }
            ToolbarItem(placement: .primaryAction) {
                Button {
                    if let url = URL(string: article.originalUrl) { openURL(url) }
                } label: {
                    Image(systemName: "safari")
                }
                .accessibilityLabel(L10n.text(language, "Open original source", "Iftaħ is-sors oriġinali"))
            }
        }
        .task {
            do {
                let result = try await model.fetchBody(for: article)
                bodyText = result.bodyText
            } catch {
                errorMessage = error.localizedDescription
            }
            loading = false
        }
        .task(id: "\(language)|\(loading)|\(bodyText)") {
            guard !loading else { return }
            let source = bodyText.isEmpty ? article.snippet : bodyText
            guard !source.isEmpty else {
                localizedBody = ""
                return
            }
            guard language != article.language else {
                localizedBody = source
                return
            }
            translating = true
            defer { translating = false }
            do {
                localizedBody = try await model.translate(source, from: article.language, to: language)
            } catch {
                localizedBody = source
            }
        }
    }
}
