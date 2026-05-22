import AVFoundation
import Foundation
import Observation

enum TTSPlaybackSettings {
    struct ModelPreset: Identifiable, Hashable {
        let id: String
        let label: String
        let description: String
    }

    struct VoicePreset: Identifiable, Hashable {
        let id: String
        let label: String
    }

    static let autoPlayKey = "ttsAutoPlayEnabled"
    static let mimoApiKeyKey = "ttsMimoApiKey"
    static let mimoBaseURLKey = "ttsMimoBaseURL"
    static let mimoModelKey = "ttsMimoModel"
    static let mimoVoiceKey = "ttsMimoVoice"
    static let mimoVoiceDesignDescKey = "ttsMimoVoiceDesignDesc"
    static let mimoStylePromptKey = "ttsMimoStylePrompt"

    static let defaultBaseURL = "https://token-plan-sgp.xiaomimimo.com/v1"
    static let defaultModel = "mimo-v2.5-tts"
    static let defaultVoice = "冰糖"

    static let presetModels: [ModelPreset] = [
        ModelPreset(
            id: "mimo-v2.5-tts",
            label: "预置音色",
            description: "从 MiMo 内置音色中选择"
        ),
        ModelPreset(
            id: "mimo-v2.5-tts-voicedesign",
            label: "音色设计",
            description: "用自然语言描述想要的音色"
        ),
        ModelPreset(
            id: "mimo-v2.5-tts-voiceclone",
            label: "音色复刻",
            description: "使用已配置的复刻音色"
        )
    ]

    static let presetVoices: [VoicePreset] = [
        VoicePreset(id: "冰糖", label: "冰糖（中文·女）"),
        VoicePreset(id: "茉莉", label: "茉莉（中文·女）"),
        VoicePreset(id: "苏打", label: "苏打（中文·男）"),
        VoicePreset(id: "白桦", label: "白桦（中文·男）"),
        VoicePreset(id: "Mia", label: "Mia (English-Female)"),
        VoicePreset(id: "Chloe", label: "Chloe (English-Female)"),
        VoicePreset(id: "Milo", label: "Milo (English-Male)"),
        VoicePreset(id: "Rose", label: "Rose (English-Female)"),
        VoicePreset(id: "Ethan", label: "Ethan (English-Male)"),
        VoicePreset(id: "Noah", label: "Noah (English-Male)")
    ]

    static func modelLabel(for id: String) -> String {
        presetModels.first { $0.id == id }?.label ?? id
    }

    static func modelDescription(for id: String) -> String {
        presetModels.first { $0.id == id }?.description ?? "自定义模型"
    }

    static func voiceLabel(for id: String) -> String {
        presetVoices.first { $0.id == id }?.label ?? (id.isEmpty ? defaultVoice : id)
    }

    static var autoPlayEnabled: Bool {
        UserDefaults.standard.bool(forKey: autoPlayKey)
    }

    static var mimoAPIKey: String {
        UserDefaults.standard.string(forKey: mimoApiKeyKey) ?? ""
    }

    static var mimoBaseURL: String {
        let raw = UserDefaults.standard.string(forKey: mimoBaseURLKey) ?? ""
        return raw.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? defaultBaseURL : raw
    }

    static var mimoModel: String {
        let raw = UserDefaults.standard.string(forKey: mimoModelKey) ?? ""
        return raw.isEmpty ? defaultModel : raw
    }

    static var mimoVoice: String {
        let raw = UserDefaults.standard.string(forKey: mimoVoiceKey) ?? ""
        return raw.isEmpty ? defaultVoice : raw
    }

    static var mimoVoiceDesignDesc: String {
        UserDefaults.standard.string(forKey: mimoVoiceDesignDescKey) ?? ""
    }

    static var mimoStylePrompt: String {
        UserDefaults.standard.string(forKey: mimoStylePromptKey) ?? ""
    }
}

@MainActor
@Observable
final class TTSPlaybackController {
    static let shared = TTSPlaybackController()

    var currentMessageID: String?
    var isLoading = false
    var isPlaying = false
    var isPaused = false
    var lastError: String?

    @ObservationIgnored private var player: AVAudioPlayer?
    @ObservationIgnored private var playbackTask: Task<Void, Never>?
    @ObservationIgnored private var requestTask: Task<Void, Never>?

    private init() {}

    func canPlay(content: String, isUser: Bool, isStreaming: Bool) -> Bool {
        !isUser
        && !isStreaming
        && !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        && !TTSPlaybackSettings.mimoAPIKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    func toggle(messageID: String, content: String) {
        if currentMessageID == messageID && (isPlaying || isLoading) {
            if isLoading {
                stop()
            } else if isPaused {
                resume()
            } else {
                pause()
            }
            return
        }

        requestTask?.cancel()
        requestTask = Task { [weak self] in
            await self?.play(messageID: messageID, content: content, automatic: false)
        }
    }

    func autoPlayIfEnabled(messageID: String, content: String) {
        guard TTSPlaybackSettings.autoPlayEnabled,
              !TTSPlaybackSettings.mimoAPIKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return
        }
        requestTask?.cancel()
        requestTask = Task { [weak self] in
            await self?.play(messageID: messageID, content: content, automatic: true)
        }
    }

    func stop() {
        stop(cancelRequest: true)
    }

    private func stop(cancelRequest: Bool) {
        if cancelRequest {
            requestTask?.cancel()
            requestTask = nil
        }
        playbackTask?.cancel()
        playbackTask = nil
        player?.stop()
        player = nil
        currentMessageID = nil
        isLoading = false
        isPlaying = false
        isPaused = false
    }

    private func pause() {
        player?.pause()
        isPaused = true
    }

    private func resume() {
        guard player?.play() == true else { return }
        isPaused = false
    }

    private func play(messageID: String, content: String, automatic: Bool) async {
        stop(cancelRequest: false)
        currentMessageID = messageID
        isLoading = true
        lastError = nil

        do {
            let audioURL = try await synthesize(messageID: messageID, content: content)
            try Task.checkCancellation()
            try playAudioFile(audioURL, messageID: messageID)
        } catch is CancellationError {
            stop()
        } catch {
            isLoading = false
            isPlaying = false
            isPaused = false
            if currentMessageID == messageID {
                currentMessageID = nil
            }
            lastError = automatic
                ? "自动朗读失败：\(error.localizedDescription)"
                : "朗读失败：\(error.localizedDescription)"
            SoundManager.play(.error)
        }
    }

    private func synthesize(messageID: String, content: String) async throws -> URL {
        let readable = HermesRustCore.shared.readableTTS(text: content) ?? fallbackReadableText(content)
        let trimmed = readable.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { throw TTSPlaybackError.emptyText }

        let options: [String: Any] = [
            "text": trimmed,
            "baseUrl": TTSPlaybackSettings.mimoBaseURL,
            "model": TTSPlaybackSettings.mimoModel,
            "voice": TTSPlaybackSettings.mimoVoice,
            "voiceDesignDesc": TTSPlaybackSettings.mimoVoiceDesignDesc,
            "stylePrompt": TTSPlaybackSettings.mimoStylePrompt
        ]

        let requestSpec = HermesRustCore.shared.buildMimoTTSRequest(options: options)
            ?? Self.fallbackMimoRequest(options: options)
        guard requestSpec["ok"] as? Bool == true,
              let urlString = requestSpec["url"] as? String,
              let url = URL(string: urlString),
              let body = requestSpec["body"] as? [String: Any] else {
            throw TTSPlaybackError.invalidRequest
        }

        var request = URLRequest(url: url, timeoutInterval: 60)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue(TTSPlaybackSettings.mimoAPIKey, forHTTPHeaderField: "api-key")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)

        let (data, response) = try await URLSession.shared.data(for: request)
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            let bodyText = String(data: data, encoding: .utf8)
                ?? HTTPURLResponse.localizedString(forStatusCode: http.statusCode)
            throw TTSPlaybackError.http(status: http.statusCode, body: String(bodyText.prefix(200)))
        }

        let cacheDir = Self.audioCacheDirectory
        if let cached = HermesRustCore.shared.cacheMimoAudio(
            responseData: data,
            cacheDir: cacheDir,
            cacheKey: messageID
        ),
           cached["ok"] as? Bool == true,
           let path = cached["path"] as? String {
            return URL(fileURLWithPath: path)
        }
        return try Self.fallbackCacheAudio(responseData: data, cacheDir: cacheDir, cacheKey: messageID)
    }

    private func playAudioFile(_ url: URL, messageID: String) throws {
        let p = try AVAudioPlayer(contentsOf: url)
        p.prepareToPlay()
        guard p.play() else { throw TTSPlaybackError.playbackFailed }
        player = p
        isLoading = false
        isPlaying = true
        isPaused = false

        playbackTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 250_000_000)
                let didFinish = await MainActor.run {
                    guard let self, self.currentMessageID == messageID, self.player === p else {
                        return true
                    }
                    if self.isPaused || p.isPlaying {
                        return false
                    }
                    self.currentMessageID = nil
                    self.isPlaying = false
                    self.isPaused = false
                    self.player = nil
                    return true
                }
                if didFinish { return }
            }
        }
    }

    private static var audioCacheDirectory: URL {
        URL(fileURLWithPath: NSHomeDirectory())
            .appendingPathComponent(".hermespet/audio-cache/mimo", isDirectory: true)
    }

    private static func fallbackMimoRequest(options: [String: Any]) -> [String: Any] {
        let text = options["text"] as? String ?? ""
        let baseURL = ((options["baseUrl"] as? String) ?? TTSPlaybackSettings.defaultBaseURL)
            .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let model = (options["model"] as? String) ?? TTSPlaybackSettings.defaultModel
        let voice = (options["voice"] as? String) ?? TTSPlaybackSettings.defaultVoice
        let stylePrompt = (options["stylePrompt"] as? String) ?? ""
        let voiceDesignDesc = (options["voiceDesignDesc"] as? String) ?? ""

        let userContent: String
        if model == "mimo-v2.5-tts-voicedesign" {
            let designDesc = voiceDesignDesc.trimmingCharacters(in: .whitespacesAndNewlines)
            let designBase = designDesc.isEmpty ? "默认音色" : designDesc
            userContent = stylePrompt.isEmpty
                ? designBase
                : "\(designBase)\n风格指令：\(stylePrompt)"
        } else {
            userContent = stylePrompt
        }

        var audio: [String: Any] = ["format": "wav"]
        if model != "mimo-v2.5-tts-voicedesign" {
            audio["voice"] = voice
        }
        return [
            "ok": true,
            "url": "\(baseURL)/chat/completions",
            "body": [
                "model": model,
                "messages": [
                    ["role": "user", "content": userContent],
                    ["role": "assistant", "content": text]
                ],
                "audio": audio
            ]
        ]
    }

    private static func fallbackCacheAudio(responseData: Data, cacheDir: URL, cacheKey: String) throws -> URL {
        guard let json = try JSONSerialization.jsonObject(with: responseData) as? [String: Any],
              let choices = json["choices"] as? [[String: Any]],
              let message = choices.first?["message"] as? [String: Any],
              let audio = message["audio"] as? [String: Any],
              let b64 = audio["data"] as? String,
              let audioData = Data(base64Encoded: b64) else {
            throw TTSPlaybackError.audioNotFound
        }
        try FileManager.default.createDirectory(at: cacheDir, withIntermediateDirectories: true)
        let safeKey = cacheKey.filter { $0.isLetter || $0.isNumber || $0 == "-" || $0 == "_" }
        let url = cacheDir.appendingPathComponent("mimo-\(Int(Date().timeIntervalSince1970 * 1000))-\(safeKey).wav")
        try audioData.write(to: url, options: .atomic)
        return url
    }

    private func fallbackReadableText(_ raw: String) -> String {
        raw
            .components(separatedBy: .newlines)
            .filter { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("```") }
            .joined(separator: "\n")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

enum TTSPlaybackError: LocalizedError {
    case emptyText
    case invalidRequest
    case audioNotFound
    case playbackFailed
    case http(status: Int, body: String)

    var errorDescription: String? {
        switch self {
        case .emptyText:
            return "消息没有可朗读文本"
        case .invalidRequest:
            return "MiMo TTS 请求构造失败"
        case .audioNotFound:
            return "MiMo TTS 响应中没有音频"
        case .playbackFailed:
            return "音频播放器启动失败"
        case .http(let status, let body):
            return "MiMo TTS 返回 \(status)：\(body)"
        }
    }
}
