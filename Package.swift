// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "HermesPet",
    platforms: [
        .macOS(.v14)
    ],
    targets: [
        .executableTarget(
            name: "HermesPet",
            linkerSettings: [
                .linkedFramework("Carbon")
            ]
        )
    ]
)
