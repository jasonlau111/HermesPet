import Foundation
import Darwin

/// Optional Rust accelerator for stream-event parsing and large-message indexing.
/// The app keeps Swift fallbacks so debug builds and machines without Cargo still run.
final class HermesRustCore: @unchecked Sendable {
    static let shared = HermesRustCore()

    private typealias ParseOpenCodeSSEFn = @convention(c) (
        UnsafePointer<UInt8>?, Int,
        UnsafePointer<UInt8>?, Int
    ) -> UnsafeMutablePointer<CChar>?
    private typealias IndexMessagesFn = @convention(c) (
        UnsafePointer<UInt8>?, Int,
        Int
    ) -> UnsafeMutablePointer<CChar>?
    private typealias OneBufferFn = @convention(c) (
        UnsafePointer<UInt8>?, Int
    ) -> UnsafeMutablePointer<CChar>?
    private typealias TwoBufferFn = @convention(c) (
        UnsafePointer<UInt8>?, Int,
        UnsafePointer<UInt8>?, Int
    ) -> UnsafeMutablePointer<CChar>?
    private typealias ThreeBufferFn = @convention(c) (
        UnsafePointer<UInt8>?, Int,
        UnsafePointer<UInt8>?, Int,
        UnsafePointer<UInt8>?, Int
    ) -> UnsafeMutablePointer<CChar>?
    private typealias FreeStringFn = @convention(c) (UnsafeMutablePointer<CChar>?) -> Void

    private let handle: UnsafeMutableRawPointer?
    private let parseOpenCodeSSE: ParseOpenCodeSSEFn?
    private let indexMessages: IndexMessagesFn?
    private let ttsReadableText: OneBufferFn?
    private let buildMimoTTSRequestFn: OneBufferFn?
    private let cacheMimoAudioFromResponseFn: ThreeBufferFn?
    private let validateText: TwoBufferFn?
    private let normalizePermissionPayload: OneBufferFn?
    private let parseOpenCodeListeningPort: OneBufferFn?
    private let parseOpenCodeHealthJSON: OneBufferFn?
    private let periodicReviewPrepareFn: OneBufferFn?
    private let growthTimelineLoadFn: OneBufferFn?
    private let growthTimelineSaveFn: TwoBufferFn?
    private let growthTimelineMarkSyncedFn: OneBufferFn?
    private let growthTimelineClearFn: OneBufferFn?
    private let freeString: FreeStringFn?

    var isAvailable: Bool {
        parseOpenCodeSSE != nil && freeString != nil
    }

    var hasTTSCore: Bool {
        buildMimoTTSRequestFn != nil && cacheMimoAudioFromResponseFn != nil && freeString != nil
    }

    private init() {
        let url = Bundle.main.url(forResource: "libhermes_chat_core", withExtension: "dylib")
        let loaded = url.flatMap { dlopen($0.path, RTLD_NOW | RTLD_LOCAL) }
        self.handle = loaded

        if let loaded {
            self.parseOpenCodeSSE = Self.loadSymbol(
                loaded,
                name: "hermes_parse_opencode_sse_event",
                as: ParseOpenCodeSSEFn.self
            )
            self.indexMessages = Self.loadSymbol(
                loaded,
                name: "hermes_index_messages_json",
                as: IndexMessagesFn.self
            )
            self.ttsReadableText = Self.loadSymbol(
                loaded,
                name: "hermes_tts_readable_text",
                as: OneBufferFn.self
            )
            self.buildMimoTTSRequestFn = Self.loadSymbol(
                loaded,
                name: "hermes_build_mimo_tts_request_json",
                as: OneBufferFn.self
            )
            self.cacheMimoAudioFromResponseFn = Self.loadSymbol(
                loaded,
                name: "hermes_mimo_cache_audio_from_response",
                as: ThreeBufferFn.self
            )
            self.validateText = Self.loadSymbol(
                loaded,
                name: "hermes_validate_text",
                as: TwoBufferFn.self
            )
            self.normalizePermissionPayload = Self.loadSymbol(
                loaded,
                name: "hermes_normalize_permission_payload",
                as: OneBufferFn.self
            )
            self.parseOpenCodeListeningPort = Self.loadSymbol(
                loaded,
                name: "hermes_parse_opencode_listening_port",
                as: OneBufferFn.self
            )
            self.parseOpenCodeHealthJSON = Self.loadSymbol(
                loaded,
                name: "hermes_parse_opencode_health_json",
                as: OneBufferFn.self
            )
            self.periodicReviewPrepareFn = Self.loadSymbol(
                loaded,
                name: "hermes_periodic_review_prepare",
                as: OneBufferFn.self
            )
            self.growthTimelineLoadFn = Self.loadSymbol(
                loaded,
                name: "hermes_growth_timeline_load",
                as: OneBufferFn.self
            )
            self.growthTimelineSaveFn = Self.loadSymbol(
                loaded,
                name: "hermes_growth_timeline_save",
                as: TwoBufferFn.self
            )
            self.growthTimelineMarkSyncedFn = Self.loadSymbol(
                loaded,
                name: "hermes_growth_timeline_mark_synced",
                as: OneBufferFn.self
            )
            self.growthTimelineClearFn = Self.loadSymbol(
                loaded,
                name: "hermes_growth_timeline_clear",
                as: OneBufferFn.self
            )
            self.freeString = Self.loadSymbol(
                loaded,
                name: "hermes_rust_free_string",
                as: FreeStringFn.self
            )
        } else {
            self.parseOpenCodeSSE = nil
            self.indexMessages = nil
            self.ttsReadableText = nil
            self.buildMimoTTSRequestFn = nil
            self.cacheMimoAudioFromResponseFn = nil
            self.validateText = nil
            self.normalizePermissionPayload = nil
            self.parseOpenCodeListeningPort = nil
            self.parseOpenCodeHealthJSON = nil
            self.periodicReviewPrepareFn = nil
            self.growthTimelineLoadFn = nil
            self.growthTimelineSaveFn = nil
            self.growthTimelineMarkSyncedFn = nil
            self.growthTimelineClearFn = nil
            self.freeString = nil
        }
    }

    deinit {
        if let handle {
            dlclose(handle)
        }
    }

    func parseOpenCodeSSEEvent(json: String, targetSessionID: String) -> [String: Any]? {
        guard let parseOpenCodeSSE, let freeString else { return nil }
        let jsonBytes = Array(json.utf8)
        let sessionBytes = Array(targetSessionID.utf8)
        let resultPtr = jsonBytes.withUnsafeBufferPointer { jsonBuffer in
            sessionBytes.withUnsafeBufferPointer { sessionBuffer in
                parseOpenCodeSSE(
                    jsonBuffer.baseAddress,
                    jsonBuffer.count,
                    sessionBuffer.baseAddress,
                    sessionBuffer.count
                )
            }
        }
        guard let resultPtr else { return nil }
        defer { freeString(resultPtr) }
        let result = String(cString: resultPtr)
        guard let data = result.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }
        return object
    }

    func indexMessages(_ messages: [ChatMessage], visibleLimit: Int) -> [String: Any]? {
        guard let indexMessages, let freeString else { return nil }
        let payload = messages.map {
            [
                "id": $0.id,
                "role": $0.role.rawValue,
                "content": $0.content,
                "isStreaming": $0.isStreaming
            ] as [String: Any]
        }
        guard let data = try? JSONSerialization.data(withJSONObject: payload) else { return nil }
        let resultPtr = data.withUnsafeBytes { rawBuffer in
            indexMessages(
                rawBuffer.bindMemory(to: UInt8.self).baseAddress,
                data.count,
                visibleLimit
            )
        }
        guard let resultPtr else { return nil }
        defer { freeString(resultPtr) }
        let result = String(cString: resultPtr)
        guard let resultData = result.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: resultData) as? [String: Any] else {
            return nil
        }
        return object
    }

    func readableTTS(text: String) -> String? {
        guard let ttsReadableText,
              let object = callJSON(ttsReadableText, text),
              let text = object["text"] as? String else {
            return nil
        }
        return text
    }

    func buildMimoTTSRequest(options: [String: Any]) -> [String: Any]? {
        guard let buildMimoTTSRequestFn,
              let data = try? JSONSerialization.data(withJSONObject: options),
              let raw = String(data: data, encoding: .utf8) else {
            return nil
        }
        return callJSON(buildMimoTTSRequestFn, raw)
    }

    func cacheMimoAudio(responseData: Data, cacheDir: URL, cacheKey: String) -> [String: Any]? {
        guard let cacheMimoAudioFromResponseFn, let freeString else { return nil }
        let responseBytes = Array(responseData)
        let dirBytes = Array(cacheDir.path.utf8)
        let keyBytes = Array(cacheKey.utf8)
        let resultPtr = responseBytes.withUnsafeBufferPointer { responseBuffer in
            dirBytes.withUnsafeBufferPointer { dirBuffer in
                keyBytes.withUnsafeBufferPointer { keyBuffer in
                    cacheMimoAudioFromResponseFn(
                        responseBuffer.baseAddress,
                        responseBuffer.count,
                        dirBuffer.baseAddress,
                        dirBuffer.count,
                        keyBuffer.baseAddress,
                        keyBuffer.count
                    )
                }
            }
        }
        guard let resultPtr else { return nil }
        defer { freeString(resultPtr) }
        return Self.decodeJSONObject(resultPtr)
    }

    func validate(kind: String, text: String) -> (ok: Bool, error: String?)? {
        guard let validateText,
              let object = callJSON(validateText, kind, text),
              let ok = object["ok"] as? Bool else {
            return nil
        }
        return (ok, object["error"] as? String)
    }

    func normalizePermissionPayload(_ data: Data) -> [String: Any]? {
        guard let normalizePermissionPayload,
              let raw = String(data: data, encoding: .utf8) else {
            return nil
        }
        return callJSON(normalizePermissionPayload, raw)
    }

    func parseOpenCodePort(from line: String) -> Int? {
        guard let parseOpenCodeListeningPort,
              let object = callJSON(parseOpenCodeListeningPort, line),
              object["ok"] as? Bool == true else {
            return nil
        }
        return object["port"] as? Int
    }

    func parseOpenCodeHealth(_ data: Data) -> Bool? {
        guard let parseOpenCodeHealthJSON,
              let raw = String(data: data, encoding: .utf8),
              let object = callJSON(parseOpenCodeHealthJSON, raw),
              object["ok"] as? Bool == true else {
            return nil
        }
        return object["healthy"] as? Bool
    }

    func preparePeriodicReview(options: [String: Any]) -> [String: Any]? {
        guard let periodicReviewPrepareFn,
              let raw = Self.encodeJSONObject(options) else {
            return nil
        }
        return callJSON(periodicReviewPrepareFn, raw)
    }

    func loadGrowthTimeline(options: [String: Any]) -> [String: Any]? {
        guard let growthTimelineLoadFn,
              let raw = Self.encodeJSONObject(options) else {
            return nil
        }
        return callJSON(growthTimelineLoadFn, raw)
    }

    func saveGrowthTimelineEntry(options: [String: Any], review: String) -> [String: Any]? {
        guard let growthTimelineSaveFn,
              let raw = Self.encodeJSONObject(options) else {
            return nil
        }
        return callJSON(growthTimelineSaveFn, raw, review)
    }

    func markGrowthTimelineSynced(options: [String: Any]) -> [String: Any]? {
        guard let growthTimelineMarkSyncedFn,
              let raw = Self.encodeJSONObject(options) else {
            return nil
        }
        return callJSON(growthTimelineMarkSyncedFn, raw)
    }

    func clearGrowthTimeline(options: [String: Any]) -> [String: Any]? {
        guard let growthTimelineClearFn,
              let raw = Self.encodeJSONObject(options) else {
            return nil
        }
        return callJSON(growthTimelineClearFn, raw)
    }

    private static func loadSymbol<T>(
        _ handle: UnsafeMutableRawPointer,
        name: String,
        as type: T.Type
    ) -> T? {
        guard let symbol = dlsym(handle, name) else { return nil }
        return unsafeBitCast(symbol, to: T.self)
    }

    private func callJSON(_ fn: OneBufferFn, _ text: String) -> [String: Any]? {
        guard let freeString else { return nil }
        let bytes = Array(text.utf8)
        let resultPtr = bytes.withUnsafeBufferPointer { buffer in
            fn(buffer.baseAddress, buffer.count)
        }
        guard let resultPtr else { return nil }
        defer { freeString(resultPtr) }
        return Self.decodeJSONObject(resultPtr)
    }

    private func callJSON(_ fn: TwoBufferFn, _ first: String, _ second: String) -> [String: Any]? {
        guard let freeString else { return nil }
        let firstBytes = Array(first.utf8)
        let secondBytes = Array(second.utf8)
        let resultPtr = firstBytes.withUnsafeBufferPointer { firstBuffer in
            secondBytes.withUnsafeBufferPointer { secondBuffer in
                fn(
                    firstBuffer.baseAddress,
                    firstBuffer.count,
                    secondBuffer.baseAddress,
                    secondBuffer.count
                )
            }
        }
        guard let resultPtr else { return nil }
        defer { freeString(resultPtr) }
        return Self.decodeJSONObject(resultPtr)
    }

    private static func encodeJSONObject(_ object: [String: Any]) -> String? {
        guard let data = try? JSONSerialization.data(withJSONObject: object) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    private static func decodeJSONObject(_ ptr: UnsafeMutablePointer<CChar>) -> [String: Any]? {
        let result = String(cString: ptr)
        guard let data = result.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }
        return object
    }
}
