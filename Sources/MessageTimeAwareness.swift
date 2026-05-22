import Foundation

/// Builds API-only time context for model prompts.
///
/// ChatMessage.content stays unchanged for UI, persistence, search, and resume.
enum MessageTimeAwareness {
    static let systemInstruction = """

    【时间感知】
    使用者消息前可能带有 HermesPet 加入的真实发送时间前缀，例如：
    [2026-05-23 01:30 | 距上次用户消息 2h6m | 距上次回复 2h5m]

    把时间和间隔当作理解对话状态的线索，让语气、承接方式和话题选择更自然，但不要刻意报时。不要输出“距离上次聊天已经 3 小时 42 分钟”这类表演式信息，也不要编造自己在间隔期间做了什么。
    """

    struct RenderedMessage {
        let message: ChatMessage
        let content: String
    }

    static func render(messages: [ChatMessage]) -> [RenderedMessage] {
        var previousUserTimestamp: Date?
        var previousAssistantTimestamp: Date?

        return messages.map { message in
            let content = modelContent(
                for: message,
                previousUserTimestamp: previousUserTimestamp,
                previousAssistantTimestamp: previousAssistantTimestamp
            )

            switch message.role {
            case .user:
                previousUserTimestamp = message.timestamp
            case .assistant:
                previousAssistantTimestamp = message.timestamp
            case .system:
                break
            }

            return RenderedMessage(message: message, content: content)
        }
    }

    static func latestUserContent(in messages: [ChatMessage]) -> String? {
        var previousUserTimestamp: Date?
        var previousAssistantTimestamp: Date?
        var latestUserContent: String?

        for message in messages {
            let content = modelContent(
                for: message,
                previousUserTimestamp: previousUserTimestamp,
                previousAssistantTimestamp: previousAssistantTimestamp
            )

            switch message.role {
            case .user:
                latestUserContent = content
                previousUserTimestamp = message.timestamp
            case .assistant:
                previousAssistantTimestamp = message.timestamp
            case .system:
                break
            }
        }

        return latestUserContent
    }

    static func modelContent(
        for message: ChatMessage,
        previousUserTimestamp: Date?,
        previousAssistantTimestamp: Date?
    ) -> String {
        guard message.role == .user else { return message.content }
        return prefixedUserContent(
            message.content,
            timestamp: message.timestamp,
            previousUserTimestamp: previousUserTimestamp,
            previousAssistantTimestamp: previousAssistantTimestamp
        )
    }

    private static func prefixedUserContent(
        _ content: String,
        timestamp: Date,
        previousUserTimestamp: Date?,
        previousAssistantTimestamp: Date?
    ) -> String {
        var segments = [formatAbsolute(timestamp)]
        if let previousUserTimestamp {
            segments.append("距上次用户消息 \(formatElapsed(timestamp.timeIntervalSince(previousUserTimestamp)))")
        }
        if let previousAssistantTimestamp {
            segments.append("距上次回复 \(formatElapsed(timestamp.timeIntervalSince(previousAssistantTimestamp)))")
        }
        return "[\(segments.joined(separator: " | "))]\n\(content)"
    }

    private static func formatAbsolute(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "zh_CN")
        formatter.timeZone = .current
        formatter.dateFormat = "yyyy-MM-dd HH:mm"
        return formatter.string(from: date)
    }

    private static func formatElapsed(_ interval: TimeInterval) -> String {
        let seconds = max(0, Int(interval.rounded()))
        if seconds < 60 { return "\(seconds)s" }

        let minutes = seconds / 60
        if minutes < 60 { return "\(minutes)m" }

        let hours = minutes / 60
        let remainingMinutes = minutes % 60
        if hours < 24 {
            return remainingMinutes == 0 ? "\(hours)h" : "\(hours)h\(remainingMinutes)m"
        }

        let days = hours / 24
        let remainingHours = hours % 24
        return remainingHours == 0 ? "\(days)d" : "\(days)d\(remainingHours)h"
    }
}
