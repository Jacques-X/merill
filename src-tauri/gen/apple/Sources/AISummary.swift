import Foundation
import CoreML

// T5 tokenisation via HuggingFace swift-transformers (SPM dependency).
// Handles SentencePiece vocabularies used by flan-t5-small.
import Tokenizers

// ── Constants matching convert_model.py ──────────────────────────────────────
private let kMaxInput:  Int = 256
private let kMaxOutput: Int = 80

// ── C bridge (unchanged Rust interface) ──────────────────────────────────────
//
// Called from Rust ios_ai::generate() via extern "C".
// Input  JSON: {"headlines":["…"],"snippets":["…"]}
// Output JSON: {"headline":"…","summary":"…"}

@_cdecl("merill_generate_summary")
public func merillGenerateSummary(
    inputJson: UnsafePointer<Int8>,
    outputBuf: UnsafeMutablePointer<Int8>,
    bufLen: Int32
) -> Bool {
    guard let input = decodeInput(inputJson) else { return false }

    let fallback = SummaryOutput(
        headline: input.headlines.first ?? "",
        summary:  input.snippets.first(where: { !$0.isEmpty }) ?? ""
    )

    guard let engine = T5SummaryEngine.shared else {
        return write(fallback, to: outputBuf, maxLen: Int(bufLen))
    }

    let context = buildContext(input)

    let headline = (try? engine.generate(
        prompt:    "Write a clear 10-word news headline: \(context)",
        maxTokens: 22
    )).map(clean) ?? fallback.headline

    let summary = (try? engine.generate(
        prompt:    "Summarize in two sentences: \(context)",
        maxTokens: 72
    )).map(clean) ?? fallback.summary

    return write(SummaryOutput(headline: headline, summary: summary),
                 to: outputBuf, maxLen: Int(bufLen))
}

// ── T5 inference engine ───────────────────────────────────────────────────────

private final class T5SummaryEngine {

    // Lazily initialised once; nil if model files are missing (dev build without models).
    static let shared: T5SummaryEngine? = try? T5SummaryEngine()

    private let encoder:   MLModel
    private let decoder:   MLModel
    private let tokenizer: any TextTokenizer
    private let eosId:     Int
    private let padId:     Int

    init() throws {
        let cfg = MLModelConfiguration()
        cfg.computeUnits = .all   // Neural Engine + GPU + CPU

        // Xcode compiles .mlpackage → .mlmodelc at build time; look for the compiled form.
        guard
            let encURL = modelURL("SummaryEncoder"),
            let decURL = modelURL("SummaryDecoder"),
            let tokURL = Bundle.main.url(forResource: "SummaryTokenizer", withExtension: nil)
        else { throw SummaryError.filesNotFound }

        encoder   = try MLModel(contentsOf: encURL, configuration: cfg)
        decoder   = try MLModel(contentsOf: decURL, configuration: cfg)

        // Load T5 SentencePiece tokenizer from the bundled SummaryTokenizer/ folder.
        // swift-transformers reads tokenizer_config.json + tokenizer.json from the folder.
        tokenizer = try AutoTokenizer.from(
            tokenizerConfig: config(from: tokURL, file: "tokenizer_config.json"),
            tokenizerData:   config(from: tokURL, file: "tokenizer.json")
        )
        eosId = tokenizer.eosTokenId ?? 1
        padId = tokenizer.padTokenId ?? 0
    }

    // ── Generate text for a single prompt ────────────────────────────────────

    func generate(prompt: String, maxTokens: Int) throws -> String {
        // 1. Tokenise + pad to kMaxInput
        let rawIds  = tokenizer(prompt).inputIds
        let clipLen = min(rawIds.count, kMaxInput)

        var inputIds = [Int32](repeating: Int32(padId), count: kMaxInput)
        var attnMask = [Int32](repeating: 0,            count: kMaxInput)
        for i in 0..<clipLen {
            inputIds[i] = Int32(rawIds[i])
            attnMask[i] = 1
        }

        // 2. Encode
        let encFeatures = try MLDictionaryFeatureProvider(dictionary: [
            "input_ids":      try int32Array(inputIds, shape: [1, kMaxInput]),
            "attention_mask": try int32Array(attnMask, shape: [1, kMaxInput]),
        ])
        let encOutput    = try encoder.prediction(from: encFeatures)
        let hiddenStates = encOutput.featureValue(for: "encoder_hidden_states")!.multiArrayValue!

        // 3. Greedy decode
        //    decoderIds[0] = pad_token (T5 uses <pad> as the decoder start token)
        //    pos_mask is one-hot at step index so the model returns logits for that position only.
        var decoderIds = [Int32](repeating: Int32(padId), count: kMaxOutput)
        var generatedCount = 0

        let limit = min(maxTokens, kMaxOutput - 1)
        for step in 0..<limit {
            var posMask = [Float32](repeating: 0, count: kMaxOutput)
            posMask[step] = 1.0

            let decFeatures = try MLDictionaryFeatureProvider(dictionary: [
                "decoder_input_ids":      try int32Array(decoderIds,    shape: [1, kMaxOutput]),
                "encoder_hidden_states":  hiddenStates,
                "encoder_attention_mask": try int32Array(attnMask,      shape: [1, kMaxInput]),
                "pos_mask":               try float32Array(posMask,     shape: [1, kMaxOutput]),
            ])
            let decOutput = try decoder.prediction(from: decFeatures)
            let logits    = decOutput.featureValue(for: "logits")!.multiArrayValue!
            let nextTok   = argmax(logits)

            if nextTok == eosId { break }
            if step + 1 < kMaxOutput {
                decoderIds[step + 1] = Int32(nextTok)
            }
            generatedCount = step + 1
        }

        // 4. Decode tokens → text (skip the leading pad start token)
        let outputTokens = decoderIds[1...generatedCount].map { Int($0) }
        return tokenizer.decode(tokens: outputTokens)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

private enum SummaryError: Error { case filesNotFound }

private struct SummaryInput:  Codable { let headlines: [String]; let snippets: [String] }
private struct SummaryOutput: Codable { let headline:  String;   let summary:  String   }

private func buildContext(_ input: SummaryInput) -> String {
    let hl  = input.headlines.prefix(5).joined(separator: ". ")
    let snp = input.snippets.filter { !$0.isEmpty }.prefix(2).joined(separator: " ")
    return String((hl + ". " + snp).prefix(700))
}

private func clean(_ text: String) -> String {
    text.trimmingCharacters(in: .whitespacesAndNewlines)
        .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
}

// Try .mlmodelc first (Xcode-compiled), fall back to .mlpackage (direct load, slower).
private func modelURL(_ name: String) -> URL? {
    Bundle.main.url(forResource: name, withExtension: "mlmodelc")
    ?? Bundle.main.url(forResource: name, withExtension: "mlpackage")
}

// Load a JSON file from a bundle directory into a swift-transformers Config.
private func config(from folder: URL, file: String) throws -> Config {
    let data = try Data(contentsOf: folder.appendingPathComponent(file))
    let json = try JSONSerialization.jsonObject(with: data) as! [String: Any]
    return Config(json)
}

private func argmax(_ arr: MLMultiArray) -> Int {
    var best = 0
    var bestVal = arr[0].floatValue
    for i in 1..<arr.count {
        let v = arr[i].floatValue
        if v > bestVal { bestVal = v; best = i }
    }
    return best
}

private func int32Array(_ values: [Int32], shape: [Int]) throws -> MLMultiArray {
    let arr = try MLMultiArray(shape: shape.map { NSNumber(value: $0) }, dataType: .int32)
    for (i, v) in values.enumerated() { arr[i] = NSNumber(value: v) }
    return arr
}

private func float32Array(_ values: [Float32], shape: [Int]) throws -> MLMultiArray {
    let arr = try MLMultiArray(shape: shape.map { NSNumber(value: $0) }, dataType: .float32)
    for (i, v) in values.enumerated() { arr[i] = NSNumber(value: v) }
    return arr
}

private func decodeInput(_ ptr: UnsafePointer<Int8>) -> SummaryInput? {
    let s = String(cString: ptr)
    guard let d = s.data(using: .utf8) else { return nil }
    return try? JSONDecoder().decode(SummaryInput.self, from: d)
}

private func write(_ output: SummaryOutput,
                   to buf: UnsafeMutablePointer<Int8>,
                   maxLen: Int) -> Bool {
    guard let data = try? JSONEncoder().encode(output),
          let s    = String(data: data, encoding: .utf8) else { return false }
    let utf8 = Array(s.utf8)
    let n    = min(utf8.count, maxLen - 1)
    for i in 0..<n { buf[i] = Int8(bitPattern: utf8[i]) }
    buf[n] = 0
    return true
}
