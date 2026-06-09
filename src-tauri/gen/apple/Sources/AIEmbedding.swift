import Foundation
import CoreML
import Hub
import Tokenizers

private let embeddingInputLength = 256
private let embeddingDimensions = 384

@_cdecl("merill_generate_embeddings")
public func merillGenerateEmbeddings(
    inputJson: UnsafePointer<Int8>,
    outputBuf: UnsafeMutablePointer<Int8>,
    bufLen: Int32
) -> Bool {
    guard
        let data = String(cString: inputJson).data(using: .utf8),
        let texts = try? JSONDecoder().decode([String].self, from: data),
        let engine = E5EmbeddingEngine.shared,
        let vectors = try? texts.map({ try engine.embed("passage: \($0)") }),
        let output = try? JSONEncoder().encode(vectors),
        output.count < Int(bufLen)
    else {
        return false
    }

    output.copyBytes(to: UnsafeMutableRawBufferPointer(
        start: outputBuf,
        count: output.count
    ))
    outputBuf[output.count] = 0
    return true
}

private final class E5EmbeddingEngine {
    static let shared: E5EmbeddingEngine? = try? E5EmbeddingEngine()

    private let model: MLModel
    private let tokenizer: any Tokenizer
    private let padId: Int

    init() throws {
        let configuration = MLModelConfiguration()
        configuration.computeUnits = .all
        guard
            let modelURL = Bundle.main.url(
                forResource: "EmbeddingModel",
                withExtension: "mlmodelc"
            ) ?? Bundle.main.url(
                forResource: "EmbeddingModel",
                withExtension: "mlpackage"
            ),
            let tokenizerURL = Bundle.main.url(
                forResource: "EmbeddingTokenizer",
                withExtension: nil
            )
        else {
            throw EmbeddingError.filesNotFound
        }
        model = try MLModel(contentsOf: modelURL, configuration: configuration)
        tokenizer = try AutoTokenizer.from(
            tokenizerConfig: embeddingConfig(
                from: tokenizerURL,
                file: "tokenizer_config.json"
            ),
            tokenizerData: embeddingConfig(
                from: tokenizerURL,
                file: "tokenizer.json"
            )
        )
        padId = tokenizer.convertTokenToId("<pad>") ?? 1
    }

    func embed(_ text: String) throws -> [Float] {
        let rawIds = tokenizer(text)
        let count = min(rawIds.count, embeddingInputLength)
        var ids = [Int32](repeating: Int32(padId), count: embeddingInputLength)
        var mask = [Int32](repeating: 0, count: embeddingInputLength)
        for index in 0..<count {
            ids[index] = Int32(rawIds[index])
            mask[index] = 1
        }
        let prediction = try model.prediction(from: MLDictionaryFeatureProvider(
            dictionary: [
                "input_ids": try embeddingIntArray(
                    ids,
                    shape: [1, embeddingInputLength]
                ),
                "attention_mask": try embeddingIntArray(
                    mask,
                    shape: [1, embeddingInputLength]
                ),
            ]
        ))
        guard
            let hidden = prediction.featureValue(
                for: "last_hidden_state"
            )?.multiArrayValue
        else {
            throw EmbeddingError.invalidOutput
        }

        var vector = [Float](repeating: 0, count: embeddingDimensions)
        for token in 0..<count {
            for dimension in 0..<embeddingDimensions {
                let offset = token * embeddingDimensions + dimension
                vector[dimension] += hidden[offset].floatValue
            }
        }
        let divisor = Float(max(count, 1))
        vector = vector.map { $0 / divisor }
        let norm = sqrt(vector.reduce(0) { $0 + $1 * $1 })
        return norm > 0 ? vector.map { $0 / norm } : vector
    }
}

private enum EmbeddingError: Error {
    case filesNotFound
    case invalidOutput
}

private func embeddingConfig(from folder: URL, file: String) throws -> Config {
    let data = try Data(contentsOf: folder.appendingPathComponent(file))
    return try JSONDecoder().decode(Config.self, from: data)
}

private func embeddingIntArray(
    _ values: [Int32],
    shape: [Int]
) throws -> MLMultiArray {
    let array = try MLMultiArray(
        shape: shape.map { NSNumber(value: $0) },
        dataType: .int32
    )
    for (index, value) in values.enumerated() {
        array[index] = NSNumber(value: value)
    }
    return array
}
