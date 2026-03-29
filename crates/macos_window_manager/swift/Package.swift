// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "MacosWindowManager",
    platforms: [
        .macOS(.v13),
    ],
    products: [
        .library(name: "MacosWindowManagerFFI", type: .static, targets: ["MacosWindowManagerFFI"]),
    ],
    targets: [
        .target(name: "MacosWindowManagerCore"),
        .target(
            name: "MacosWindowManagerFFI",
            dependencies: ["MacosWindowManagerCore"]
        ),
        .testTarget(
            name: "MacosWindowManagerCoreTests",
            dependencies: ["MacosWindowManagerCore"]
        ),
    ]
)
