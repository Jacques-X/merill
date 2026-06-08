import Foundation

private struct BridgeEnvelope<T: Decodable>: Decodable {
    let ok: Bool
    let data: T?
    let error: String?
}

private struct EmptyResponse: Decodable {}

private final class BridgeContinuation {
    let continuation: CheckedContinuation<Data, Error>

    init(_ continuation: CheckedContinuation<Data, Error>) {
        self.continuation = continuation
    }
}

private let rustBridgeCallback: @convention(c) (UnsafePointer<CChar>?, UnsafeMutableRawPointer?) -> Void = {
    response, context in
    guard let context else { return }
    let box = Unmanaged<BridgeContinuation>.fromOpaque(context).takeRetainedValue()
    guard let response else {
        box.continuation.resume(throwing: RustClientError.bridge("Rust returned an empty response"))
        return
    }
    let data = Data(String(cString: response).utf8)
    merill_free_string(UnsafeMutablePointer(mutating: response))
    box.continuation.resume(returning: data)
}

enum RustClientError: LocalizedError {
    case bridge(String)
    case initialization

    var errorDescription: String? {
        switch self {
        case .bridge(let message): return message
        case .initialization: return "Merill could not initialize its local news database."
        }
    }
}

actor RustClient {
    static let shared = RustClient()

    private var initialized = false

    func initialize() throws {
        guard !initialized else { return }
        let fileManager = FileManager.default
        let root = try fileManager.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        ).appendingPathComponent("MerillNative", isDirectory: true)
        try fileManager.createDirectory(at: root, withIntermediateDirectories: true)
        let ok = root.path.withCString { merill_initialize($0) }
        guard ok else { throw RustClientError.initialization }
        initialized = true
    }

    func call<T: Decodable>(_ command: String, payload: [String: Any] = [:]) async throws -> T {
        try initialize()
        let request = try JSONSerialization.data(withJSONObject: [
            "command": command,
            "payload": payload,
        ])
        let response = try await invoke(request)
        let envelope = try JSONDecoder.merill.decode(BridgeEnvelope<T>.self, from: response)
        guard envelope.ok, let data = envelope.data else {
            throw RustClientError.bridge(envelope.error ?? "The Rust news engine returned an unknown error.")
        }
        return data
    }

    func callVoid(_ command: String, payload: [String: Any] = [:]) async throws {
        try initialize()
        let request = try JSONSerialization.data(withJSONObject: [
            "command": command,
            "payload": payload,
        ])
        let response = try await invoke(request)
        let object = try JSONSerialization.jsonObject(with: response) as? [String: Any]
        guard object?["ok"] as? Bool == true else {
            throw RustClientError.bridge(object?["error"] as? String ?? "The Rust news engine returned an unknown error.")
        }
    }

    private func invoke(_ request: Data) async throws -> Data {
        guard let requestString = String(data: request, encoding: .utf8) else {
            throw RustClientError.bridge("Could not encode the Rust request.")
        }
        return try await withCheckedThrowingContinuation { continuation in
            let box = Unmanaged.passRetained(BridgeContinuation(continuation))
            requestString.withCString { pointer in
                merill_call_async(pointer, rustBridgeCallback, box.toOpaque())
            }
        }
    }
}
