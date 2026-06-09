import SwiftUI

@main
struct MerillApp: App {
    @StateObject private var model = MerillAppModel()
    @AppStorage("merill.native.colorScheme") private var colorScheme = "system"

    var body: some Scene {
        WindowGroup {
            Group {
                if model.onboardingComplete {
                    MerillRootView()
                } else {
                    OnboardingView()
                }
            }
                .environmentObject(model)
                .environment(\.merillLanguage, model.language)
                .environment(\.locale, Locale(identifier: model.language == "mt" ? "mt_MT" : "en_MT"))
                .preferredColorScheme(preferredColorScheme)
                .task { await model.start() }
        }
        #if os(macOS)
        .defaultSize(width: 920, height: 760)
        #endif
    }

    private var preferredColorScheme: ColorScheme? {
        switch colorScheme {
        case "light": return .light
        case "dark": return .dark
        default: return nil
        }
    }
}

struct OnboardingView: View {
    @EnvironmentObject private var model: MerillAppModel
    @Environment(\.merillLanguage) private var language
    @State private var step = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            HStack(spacing: 6) {
                ForEach(0..<3) { index in
                    Capsule()
                        .fill(index <= step ? Color.accentColor : Color.secondary.opacity(0.2))
                        .frame(height: 4)
                }
            }
            .padding(.top, 28)

            Group {
                if step == 0 {
                    VStack(alignment: .leading, spacing: 14) {
                        Text("Merill").font(.caption.weight(.bold)).foregroundStyle(.tint)
                        Text("News across perspectives").font(.largeTitle.weight(.bold))
                        Text("Choose the language Merill should use for headlines, summaries, and controls.")
                            .foregroundStyle(.secondary)
                        Picker("Language", selection: $model.language) {
                            Text("English").tag("en")
                            Text("Malti").tag("mt")
                        }
                        .pickerStyle(.segmented)
                        .padding(.top, 8)
                    }
                } else if step == 1 {
                    VStack(alignment: .leading, spacing: 14) {
                        Text(L10n.text(language, "Sources", "Sorsi")).font(.caption.weight(.bold)).foregroundStyle(.tint)
                        Text(L10n.text(language, "Build your Malta feed", "Ibni l-feed Malti tiegħek")).font(.largeTitle.weight(.bold))
                        Text(L10n.text(language, "Select publishers. You can change this later in Settings.", "Agħżel pubblikaturi. Tista' tibdel dan aktar tard fis-Settings."))
                            .foregroundStyle(.secondary)
                        ScrollView {
                            VStack(spacing: 8) {
                                ForEach(model.publishers.filter { !$0.isGlobal }) { publisher in
                                    Button {
                                        model.togglePublisher(publisher)
                                    } label: {
                                        HStack {
                                            SourceLabel(publisher: publisher)
                                            Spacer()
                                            if model.isPublisherEnabled(publisher) {
                                                Image(systemName: "checkmark.circle.fill")
                                            }
                                        }
                                        .padding(12)
                                        .background(Color.secondary.opacity(0.08), in: RoundedRectangle(cornerRadius: 14))
                                    }
                                    .buttonStyle(.plain)
                                }
                            }
                        }
                    }
                } else {
                    VStack(alignment: .leading, spacing: 14) {
                        Text(L10n.text(language, "How Merill reads coverage", "Kif Merill jaqra l-kopertura")).font(.caption.weight(.bold)).foregroundStyle(.tint)
                        Text(L10n.text(language, "Ownership is context", "Is-sjieda hija kuntest")).font(.largeTitle.weight(.bold))
                        Text(L10n.text(language, "Colors describe publisher ownership, not whether an article is true. A blindspot means no independent publisher was found in that story group.", "Il-kuluri jiddeskrivu s-sjieda tal-pubblikatur, mhux jekk artiklu huwiex veru. Punt mudlam ifisser li ma nstabx pubblikatur indipendenti f'dak il-grupp."))
                            .foregroundStyle(.secondary)
                    }
                }
            }
            Spacer()
            Button {
                if step < 2 { step += 1 }
                else { model.onboardingComplete = true }
            } label: {
                Label(step < 2 ? L10n.text(language, "Continue", "Kompli") : L10n.text(language, "Open Merill", "Iftaħ Merill"), systemImage: "chevron.right")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
        }
        .padding()
        .task { if model.publishers.isEmpty { try? await model.reload() } }
    }
}

struct MerillRootView: View {
    @Environment(\.merillLanguage) private var language
    @State private var selection: RootTab = .feed

    var body: some View {
        TabView(selection: $selection) {
            FeedView(tab: .feed)
                .tabItem { Label(RootTab.feed.title(language), systemImage: RootTab.feed.symbol) }
                .tag(RootTab.feed)
            FeedView(tab: .blindspots)
                .tabItem { Label(RootTab.blindspots.title(language), systemImage: RootTab.blindspots.symbol) }
                .tag(RootTab.blindspots)
            FeedView(tab: .saved)
                .tabItem { Label(RootTab.saved.title(language), systemImage: RootTab.saved.symbol) }
                .tag(RootTab.saved)
            SettingsView()
                .tabItem { Label(RootTab.settings.title(language), systemImage: RootTab.settings.symbol) }
                .tag(RootTab.settings)
        }
        .merillTabBarBehavior()
    }
}
