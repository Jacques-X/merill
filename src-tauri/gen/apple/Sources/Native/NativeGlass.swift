import SwiftUI

extension View {
    @ViewBuilder
    func merillInlineNavigationTitle() -> some View {
        #if os(iOS)
        self.navigationBarTitleDisplayMode(.inline)
        #else
        self
        #endif
    }

    @ViewBuilder
    func merillTabBarBehavior() -> some View {
        #if os(iOS)
        if #available(iOS 26.0, *) {
            self.tabBarMinimizeBehavior(.onScrollDown)
        } else {
            self
        }
        #else
        self
        #endif
    }

    @ViewBuilder
    func merillInteractiveGlass<S: Shape>(in shape: S) -> some View {
        if #available(iOS 26.0, macOS 26.0, *) {
            self.glassEffect(.regular.interactive(), in: shape)
        } else {
            self.background(.regularMaterial, in: shape)
        }
    }
}

struct MerillEmptyState<Actions: View>: View {
    let title: String
    let symbol: String
    let description: String
    @ViewBuilder let actions: () -> Actions

    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: symbol)
                .font(.system(size: 34))
                .foregroundStyle(.secondary)
            Text(title)
                .font(.headline)
            Text(description)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 340)
            actions()
        }
        .padding(28)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

extension MerillEmptyState where Actions == EmptyView {
    init(title: String, symbol: String, description: String) {
        self.init(title: title, symbol: symbol, description: description) { EmptyView() }
    }
}

struct FeedFilterControl: View {
    @Environment(\.merillLanguage) private var language
    @Binding var topic: String?
    @Namespace private var glassNamespace
    @State private var expanded = false

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
        if #available(iOS 26.0, macOS 26.0, *) {
            GlassEffectContainer(spacing: 10) {
                HStack(spacing: 10) {
                    if expanded {
                        ForEach(topics, id: \.0) { title, value in
                            Button(title) {
                                topic = value
                                withAnimation(.easeOut(duration: 0.2)) { expanded = false }
                            }
                            .font(.caption.weight(.semibold))
                            .padding(.horizontal, 12)
                            .frame(minHeight: 44)
                            .glassEffect(.regular.interactive(), in: Capsule())
                            .glassEffectID(title, in: glassNamespace)
                        }
                    } else {
                        Button {
                            withAnimation(.easeOut(duration: 0.2)) { expanded = true }
                        } label: {
                            Image(systemName: topic == nil ? "line.3.horizontal.decrease" : "line.3.horizontal.decrease.circle.fill")
                                .frame(width: 44, height: 44)
                        }
                        .accessibilityLabel(L10n.text(language, "Filter stories", "Iffiltra l-istejjer"))
                        .glassEffect(.regular.interactive(), in: Circle())
                        .glassEffectID("filter", in: glassNamespace)
                    }
                }
            }
        } else {
            Menu {
                ForEach(topics, id: \.0) { title, value in
                    Button {
                        topic = value
                    } label: {
                        if topic == value {
                            Label(title, systemImage: "checkmark")
                        } else {
                            Text(title)
                        }
                    }
                }
            } label: {
                Image(systemName: topic == nil ? "line.3.horizontal.decrease" : "line.3.horizontal.decrease.circle.fill")
                    .frame(width: 44, height: 44)
            }
            .accessibilityLabel(L10n.text(language, "Filter stories", "Iffiltra l-istejjer"))
            .merillInteractiveGlass(in: Circle())
        }
    }
}
