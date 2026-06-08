import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var model: MerillAppModel
    @Environment(\.merillLanguage) private var language
    @AppStorage("merill.native.colorScheme") private var colorScheme = "system"
    @State private var addSourceKind: AddSourceKind?
    @State private var confirmWipe = false
    @State private var working = false

    var body: some View {
        NavigationStack {
            Form {
                Section(L10n.text(language, "Appearance", "Dehra")) {
                    Picker(L10n.text(language, "Theme", "Tema"), selection: $colorScheme) {
                        Text(L10n.text(language, "System", "Sistema")).tag("system")
                        Text(L10n.text(language, "Light", "Ċar")).tag("light")
                        Text(L10n.text(language, "Dark", "Skur")).tag("dark")
                    }
                    Picker(L10n.text(language, "Feed language", "Lingwa tal-aħbarijiet"), selection: $model.language) {
                        Text("English").tag("en")
                        Text("Malti").tag("mt")
                    }
                }

                Section(L10n.text(language, "Local sources", "Sorsi lokali")) {
                    ForEach(model.publishers.filter { !$0.isGlobal }) { publisher in
                        Toggle(isOn: Binding(
                            get: { model.isPublisherEnabled(publisher) },
                            set: { _ in model.togglePublisher(publisher) }
                        )) {
                            SourceLabel(publisher: publisher)
                        }
                    }
                    Button {
                        addSourceKind = .local
                    } label: {
                        Label(L10n.text(language, "Add Malta source", "Żid sors Malti"), systemImage: "plus")
                    }
                }

                Section(L10n.text(language, "Global sources", "Sorsi globali")) {
                    ForEach(model.publishers.filter(\.isGlobal)) { publisher in
                        HStack {
                            SourceLabel(publisher: publisher)
                            Spacer()
                            Button(role: .destructive) {
                                Task { await remove(publisher) }
                            } label: {
                                Image(systemName: "trash")
                            }
                            .buttonStyle(.borderless)
                            .accessibilityLabel("\(L10n.text(language, "Remove", "Neħħi")) \(publisher.name)")
                        }
                    }
                    Button {
                        addSourceKind = .global
                    } label: {
                        Label(L10n.text(language, "Add international source", "Żid sors internazzjonali"), systemImage: "plus")
                    }
                }

                Section(L10n.text(language, "Advanced", "Avvanzat")) {
                    Button {
                        Task { await recluster() }
                    } label: {
                        Label(L10n.text(language, "Re-cluster stories", "Erġa' aggruppa l-istejjer"), systemImage: "rectangle.3.group")
                    }
                    .disabled(working)
                    Button(role: .destructive) {
                        confirmWipe = true
                    } label: {
                        Label(L10n.text(language, "Clear all local news data", "Ħassar id-dejta lokali kollha"), systemImage: "trash")
                    }
                    .disabled(working)
                }

                Section(L10n.text(language, "About", "Dwar")) {
                    LabeledContent(L10n.text(language, "App", "App"), value: "Merill")
                    LabeledContent(L10n.text(language, "Version", "Verżjoni"), value: "0.1.0")
                    Text(L10n.text(language, "A native Malta news reader with local, on-device clustering and perspective comparison.", "Qarrej nattiv tal-aħbarijiet Maltin bi gruppi lokali fuq l-apparat u tqabbil tal-perspettivi."))
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle(L10n.text(language, "Settings", "Settings"))
            .sheet(item: $addSourceKind) { kind in
                AddSourceView(kind: kind)
            }
            .alert(L10n.text(language, "Clear all local news data?", "Tħassar id-dejta lokali kollha?"), isPresented: $confirmWipe) {
                Button(L10n.text(language, "Cancel", "Ikkanċella"), role: .cancel) {}
                Button(L10n.text(language, "Clear Data", "Ħassar id-Dejta"), role: .destructive) {
                    Task { await wipe() }
                }
            } message: {
                Text(L10n.text(language, "Articles and groups will be removed from this device. Your source list remains available.", "L-artikli u l-gruppi jitneħħew minn dan l-apparat. Il-lista tas-sorsi tibqa' disponibbli."))
            }
            .alert("Merill", isPresented: Binding(
                get: { model.errorMessage != nil || model.successMessage != nil },
                set: { visible in
                    if !visible {
                        model.errorMessage = nil
                        model.successMessage = nil
                    }
                }
            )) {
                Button(L10n.text(language, "OK", "OK"), role: .cancel) {}
            } message: {
                Text(model.errorMessage ?? model.successMessage ?? "")
            }
        }
    }

    private func remove(_ publisher: Publisher) async {
        working = true
        defer { working = false }
        do { try await model.removePublisher(publisher) }
        catch { model.errorMessage = error.localizedDescription }
    }

    private func recluster() async {
        working = true
        defer { working = false }
        do { try await model.forceRecluster() }
        catch { model.errorMessage = error.localizedDescription }
    }

    private func wipe() async {
        working = true
        defer { working = false }
        do { try await model.wipe() }
        catch { model.errorMessage = error.localizedDescription }
    }
}

struct SourceLabel: View {
    let publisher: Publisher

    var body: some View {
        HStack(spacing: 10) {
            PublisherAvatar(publisher: publisher)
                .frame(width: 30, height: 30)
            VStack(alignment: .leading) {
                Text(publisher.name)
                Text(publisher.biasCategory.rawValue.replacingOccurrences(of: "_", with: " ").capitalized)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

enum AddSourceKind: String, Identifiable {
    case local
    case global
    var id: Self { self }
}

struct AddSourceView: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.merillLanguage) private var language
    @EnvironmentObject private var model: MerillAppModel
    let kind: AddSourceKind
    @State private var url = ""
    @State private var name = ""
    @State private var adding = false
    @State private var errorMessage: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField(L10n.text(language, "Publisher name (optional)", "Isem tal-pubblikatur (mhux obbligatorju)"), text: $name)
                    TextField(L10n.text(language, "Website or feed URL", "Sit jew URL tal-feed"), text: $url)
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        .keyboardType(.URL)
                        #endif
                } footer: {
                    Text(L10n.text(language, "Merill will look for an RSS feed, sitemap, or recognizable news page automatically.", "Merill ifittex awtomatikament RSS feed, sitemap, jew paġna tal-aħbarijiet magħrufa."))
                }
                if let errorMessage {
                    Section {
                        Label(errorMessage, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle(
                kind == .local
                    ? L10n.text(language, "Add Malta Source", "Żid Sors Malti")
                    : L10n.text(language, "Add Global Source", "Żid Sors Globali")
            )
            .merillInlineNavigationTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(L10n.text(language, "Cancel", "Ikkanċella")) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(
                        adding
                            ? L10n.text(language, "Adding...", "Qed jiżdied...")
                            : L10n.text(language, "Add", "Żid")
                    ) {
                        Task { await add() }
                    }
                    .disabled(url.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || adding)
                }
            }
        }
        .presentationDetents([.medium])
    }

    private func add() async {
        adding = true
        defer { adding = false }
        do {
            try await model.addPublisher(url: url, name: name, isGlobal: kind == .global)
            dismiss()
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
