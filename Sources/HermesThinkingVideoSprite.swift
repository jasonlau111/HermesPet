import AVFoundation
import SwiftUI

struct HermesThinkingVideoSprite: View {
    let height: CGFloat
    var isWorking: Bool = false
    var animated: Bool = true

    private var width: CGFloat { height * 0.72 }

    var body: some View {
        LoopingResourceVideoView(
            resourceName: "thinking-light",
            resourceExtension: "mp4",
            isPlaying: animated
        )
        .frame(width: width, height: height)
        .clipShape(RoundedRectangle(cornerRadius: max(4, height * 0.16), style: .continuous))
        .shadow(color: Color.green.opacity(isWorking ? 0.38 : 0.18), radius: isWorking ? 5 : 2, y: 1)
        .overlay(
            RoundedRectangle(cornerRadius: max(4, height * 0.16), style: .continuous)
                .stroke(Color.white.opacity(0.20), lineWidth: max(0.5, height * 0.025))
        )
    }
}

private struct LoopingResourceVideoView: NSViewRepresentable {
    let resourceName: String
    let resourceExtension: String
    let isPlaying: Bool

    func makeNSView(context: Context) -> LoopingVideoNSView {
        let url = Bundle.module.url(forResource: resourceName, withExtension: resourceExtension)
        return LoopingVideoNSView(url: url)
    }

    func updateNSView(_ nsView: LoopingVideoNSView, context: Context) {
        nsView.setPlaying(isPlaying)
    }
}

private final class LoopingVideoNSView: NSView {
    private let player = AVQueuePlayer()
    private let playerLayer = AVPlayerLayer()
    private var looper: AVPlayerLooper?

    init(url: URL?) {
        super.init(frame: .zero)
        wantsLayer = true
        layer = CALayer()
        layer?.masksToBounds = true

        player.isMuted = true
        player.actionAtItemEnd = .none
        playerLayer.player = player
        playerLayer.videoGravity = .resizeAspectFill
        layer?.addSublayer(playerLayer)

        if let url {
            let item = AVPlayerItem(url: url)
            looper = AVPlayerLooper(player: player, templateItem: item)
            player.play()
        }
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func layout() {
        super.layout()
        playerLayer.frame = bounds
        layer?.cornerRadius = min(bounds.width, bounds.height) * 0.16
    }

    func setPlaying(_ playing: Bool) {
        if playing {
            player.play()
        } else {
            player.pause()
        }
    }
}
