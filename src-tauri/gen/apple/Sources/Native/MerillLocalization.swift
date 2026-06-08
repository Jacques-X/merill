import SwiftUI

private struct MerillLanguageKey: EnvironmentKey {
    static let defaultValue = "en"
}

extension EnvironmentValues {
    var merillLanguage: String {
        get { self[MerillLanguageKey.self] }
        set { self[MerillLanguageKey.self] = newValue }
    }
}

enum L10n {
    static func text(_ language: String, _ english: String, _ maltese: String) -> String {
        language == "mt" ? maltese : english
    }

    static func count(
        _ language: String,
        _ value: Int,
        english: String,
        maltese: String
    ) -> String {
        String(format: text(language, english, maltese), value)
    }
}

struct TranslatedText: View {
    @EnvironmentObject private var model: MerillAppModel
    @Environment(\.merillLanguage) private var language

    let original: String
    let sourceLanguage: String
    var existingTranslation: String = ""

    @State private var value = ""

    var body: some View {
        Text(value.isEmpty ? initialValue : value)
            .task(id: taskID) {
                value = initialValue
                guard language != sourceLanguage else { return }
                if !existingTranslation.isEmpty, existingTranslation != original {
                    value = existingTranslation
                    return
                }
                do {
                    value = try await model.translate(original, from: sourceLanguage, to: language)
                } catch {
                    value = original
                }
            }
    }

    private var initialValue: String {
        if language == sourceLanguage {
            return original
        }
        if !existingTranslation.isEmpty, existingTranslation != original {
            return existingTranslation
        }
        return original
    }

    private var taskID: String {
        "\(language)|\(sourceLanguage)|\(original)|\(existingTranslation)"
    }
}
