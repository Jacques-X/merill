import SwiftUI

@main
struct MerillApp: App {
    @StateObject private var model = MerillAppModel()
    @AppStorage("merill.native.colorScheme") private var colorScheme = "system"

    var body: some Scene {
        WindowGroup {
            MerillRootView()
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
