import Foundation

struct GrowthTimelineEntry: Identifiable, Codable, Equatable, Sendable {
    let id: String
    let period: String
    let date: String
    let title: String
    let review: String
    let syncSummary: String
    let createdAt: Double
    let syncedAt: Double?
    let honchoConclusionID: String?
}

@MainActor
final class PeriodicReviewService {
    static let shared = PeriodicReviewService()

    private let enabledKey = "periodicReviewEnabled"
    private let lastReviewRunDateKey = "periodicReviewLastRunDate"
    private static let dateFormatter: DateFormatter = {
        let df = DateFormatter()
        df.dateFormat = "yyyy-MM-dd"
        df.timeZone = TimeZone.current
        df.locale = Locale(identifier: "en_US_POSIX")
        return df
    }()

    private var isGenerating = false
    private var isSyncing = false

    private init() {}

    func generateIfNeeded(viewModel: ChatViewModel) {
        let enabled = (UserDefaults.standard.object(forKey: enabledKey) as? Bool) ?? true
        guard enabled else { return }
        let today = Self.dateFormatter.string(from: Date())
        guard UserDefaults.standard.string(forKey: lastReviewRunDateKey) != today else { return }

        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 12_000_000_000)
            do {
                _ = try await generateNow(viewModel: viewModel, preferYesterday: true)
                UserDefaults.standard.set(today, forKey: lastReviewRunDateKey)
            } catch {
                NSLog("[PeriodicReview] 自动生成失败：%@", "\(error)")
            }
        }
    }

    func generateNow(viewModel: ChatViewModel, preferYesterday: Bool = false) async throws -> GrowthTimelineEntry {
        guard !isGenerating else { throw PeriodicReviewError.busy }
        isGenerating = true
        defer { isGenerating = false }

        let options = try prepareOptions(preferYesterday: preferYesterday)
        let prepared = try prepareReview(options: options)
        guard (prepared["hasData"] as? Bool) == true,
              let prompt = prepared["prompt"] as? String,
              !prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw PeriodicReviewError.noData
        }

        var review = ""
        do {
            for try await chunk in viewModel.streamOneShotAsk(
                prompt: prompt,
                modeOverride: viewModel.periodicReviewBackend,
                recordToActivity: false
            ) {
                review += chunk
            }
        } catch {
            throw PeriodicReviewError.generationFailed(error.localizedDescription)
        }

        let trimmed = review.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { throw PeriodicReviewError.emptyResponse }
        let saved = try saveReview(options: options, review: trimmed)
        UserDefaults.standard.set(Self.dateFormatter.string(from: Date()), forKey: lastReviewRunDateKey)
        return saved
    }

    func loadTimeline() -> [GrowthTimelineEntry] {
        let options = ["timelinePath": Self.timelineURL.path]
        guard let object = HermesRustCore.shared.loadGrowthTimeline(options: options),
              object["ok"] as? Bool == true,
              let rawEntries = object["entries"] as? [[String: Any]] else {
            return []
        }
        return Self.decodeEntries(rawEntries)
    }

    func clearTimeline() {
        let options = ["timelinePath": Self.timelineURL.path]
        _ = HermesRustCore.shared.clearGrowthTimeline(options: options)
    }

    func syncToHermes(_ entry: GrowthTimelineEntry) async throws -> [GrowthTimelineEntry] {
        guard !isSyncing else { throw PeriodicReviewError.busy }
        isSyncing = true
        defer { isSyncing = false }

        let content = entry.syncSummary.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !content.isEmpty else { throw PeriodicReviewError.emptySyncSummary }
        let conclusionID = try await postHonchoConclusion(content: content)
        var options: [String: Any] = [
            "timelinePath": Self.timelineURL.path,
            "entryID": entry.id,
            "syncedAt": Date().timeIntervalSince1970
        ]
        if let conclusionID {
            options["conclusionID"] = conclusionID
        }
        guard let object = HermesRustCore.shared.markGrowthTimelineSynced(options: options),
              object["ok"] as? Bool == true,
              let rawEntries = object["entries"] as? [[String: Any]] else {
            throw PeriodicReviewError.timelineWriteFailed
        }
        return Self.decodeEntries(rawEntries)
    }

    private func prepareOptions(preferYesterday: Bool) throws -> [String: Any] {
        let selectedDate = selectReviewDate(preferYesterday: preferYesterday)
        ActivityRecorder.shared.queryStore.aggregateDailyStats(for: selectedDate)

        let start = Calendar.current.startOfDay(for: selectedDate)
        let end = start.addingTimeInterval(86400)
        return [
            "activityDbPath": ActivityStore.defaultURL.path,
            "timelinePath": Self.timelineURL.path,
            "period": "daily",
            "date": Self.dateFormatter.string(from: selectedDate),
            "startTimestamp": start.timeIntervalSince1970,
            "endTimestamp": end.timeIntervalSince1970,
            "maxQuestions": 24,
            "maxIntents": 30
        ]
    }

    private func selectReviewDate(preferYesterday: Bool) -> Date {
        if preferYesterday,
           let yesterday = Calendar.current.date(byAdding: .day, value: -1, to: Date()) {
            ActivityRecorder.shared.queryStore.aggregateDailyStats(for: yesterday)
            let stats = ActivityRecorder.shared.queryStore.dailyStats(for: yesterday)
            let questions = ActivityRecorder.shared.queryStore.recentUserQuestions(withinMinutes: 48 * 60, limit: 200)
            let start = Calendar.current.startOfDay(for: yesterday)
            let end = start.addingTimeInterval(86400)
            if !stats.isEmpty || questions.contains(where: { $0.timestamp >= start && $0.timestamp < end }) {
                return yesterday
            }
        }
        return Date()
    }

    private func prepareReview(options: [String: Any]) throws -> [String: Any] {
        guard let object = HermesRustCore.shared.preparePeriodicReview(options: options) else {
            throw PeriodicReviewError.rustUnavailable
        }
        if object["ok"] as? Bool == true {
            return object
        }
        throw PeriodicReviewError.rustFailed(object["error"] as? String ?? "unknown error")
    }

    private func saveReview(options: [String: Any], review: String) throws -> GrowthTimelineEntry {
        guard let object = HermesRustCore.shared.saveGrowthTimelineEntry(options: options, review: review) else {
            throw PeriodicReviewError.rustUnavailable
        }
        guard object["ok"] as? Bool == true,
              let raw = object["entry"] as? [String: Any],
              let entry = Self.decodeEntry(raw) else {
            throw PeriodicReviewError.timelineWriteFailed
        }
        return entry
    }

    private func postHonchoConclusion(content: String) async throws -> String? {
        let baseURL = Self.honchoBaseURL().trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard let url = URL(string: "\(baseURL)/v3/workspaces/hermes/conclusions") else {
            throw PeriodicReviewError.honchoFailed("Honcho URL 无效")
        }
        let payload: [String: Any] = [
            "conclusions": [[
                "observer_id": "hermes",
                "observed_id": "sinian",
                "session_id": "hermes-agent",
                "content": content
            ]]
        ]

        do {
            let data = try await postJSON(payload, to: url)
            return Self.firstConclusionID(in: data)
        } catch PeriodicReviewError.honchoSessionMissing {
            try await ensureHonchoSession()
            let data = try await postJSON(payload, to: url)
            return Self.firstConclusionID(in: data)
        }
    }

    private func ensureHonchoSession() async throws {
        let baseURL = Self.honchoBaseURL().trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard let url = URL(string: "\(baseURL)/v3/workspaces/hermes/sessions") else {
            throw PeriodicReviewError.honchoFailed("Honcho URL 无效")
        }
        _ = try await postJSON(["id": "hermes-agent"], to: url)
    }

    private func postJSON(_ payload: [String: Any], to url: URL) async throws -> Data {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 30
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: payload)
        let (data, response) = try await URLSession.shared.data(for: request)
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            let detail = String(data: data, encoding: .utf8) ?? ""
            if http.statusCode == 404 && detail.contains("Session") {
                throw PeriodicReviewError.honchoSessionMissing
            }
            throw PeriodicReviewError.honchoFailed("HTTP \(http.statusCode): \(detail)")
        }
        return data
    }

    private static func honchoBaseURL() -> String {
        let configURL = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".hermes/honcho.json")
        guard let data = try? Data(contentsOf: configURL),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let raw = object["baseUrl"] as? String,
              !raw.isEmpty else {
            return "http://192.168.50.2:8017"
        }
        return raw
    }

    private static var timelineURL: URL {
        let dir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".hermespet", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("growth-timeline.json")
    }

    private static func decodeEntries(_ rawEntries: [[String: Any]]) -> [GrowthTimelineEntry] {
        rawEntries.compactMap(decodeEntry)
    }

    private static func decodeEntry(_ raw: [String: Any]) -> GrowthTimelineEntry? {
        guard JSONSerialization.isValidJSONObject(raw),
              let data = try? JSONSerialization.data(withJSONObject: raw) else {
            return nil
        }
        return try? JSONDecoder().decode(GrowthTimelineEntry.self, from: data)
    }

    private static func firstConclusionID(in data: Data) -> String? {
        guard let object = try? JSONSerialization.jsonObject(with: data) else { return nil }
        return findID(in: object)
    }

    private static func findID(in object: Any) -> String? {
        if let dict = object as? [String: Any] {
            if let id = dict["id"] as? String { return id }
            for value in dict.values {
                if let id = findID(in: value) { return id }
            }
        } else if let array = object as? [Any] {
            for value in array {
                if let id = findID(in: value) { return id }
            }
        }
        return nil
    }
}

enum PeriodicReviewError: LocalizedError {
    case busy
    case noData
    case rustUnavailable
    case rustFailed(String)
    case generationFailed(String)
    case emptyResponse
    case emptySyncSummary
    case timelineWriteFailed
    case honchoSessionMissing
    case honchoFailed(String)

    var errorDescription: String? {
        switch self {
        case .busy:
            return "周期回顾正在处理中"
        case .noData:
            return "还没有足够的活动数据，先用一会儿再生成"
        case .rustUnavailable:
            return "Rust 后端未加载，无法生成周期回顾"
        case .rustFailed(let detail):
            return "Rust 后端处理失败：\(detail)"
        case .generationFailed(let detail):
            return "周期回顾生成失败：\(detail)"
        case .emptyResponse:
            return "AI 没有返回周期回顾内容"
        case .emptySyncSummary:
            return "没有可同步到 Hermes 的精选摘要"
        case .timelineWriteFailed:
            return "成长时间线写入失败"
        case .honchoSessionMissing:
            return "Honcho session 不存在"
        case .honchoFailed(let detail):
            return "同步到 Hermes 失败：\(detail)"
        }
    }
}
