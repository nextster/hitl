// swift-tools-version: 6.3
import PackageDescription

// The checklist of a hand check on a shared iPhone.
//
// ConveyorCheck is the exchange with the host, the tool on the Mac that
// installed the build: the checklist it passes to the launch, the report it
// reads back, and the store that keeps both between launches. Plain
// Foundation, so it builds and tests on a Mac.
//
// ConveyorCheckUI is the panel that floats over the app with the checklist.
// UIKit; an app can show a checklist of its own in the same panel.
let upcomingFeatures: [SwiftSetting] = [
    .enableUpcomingFeature("NonisolatedNonsendingByDefault"),
    .enableUpcomingFeature("InferIsolatedConformances"),
]
// Everything in the panel draws, so its declarations are main-actor isolated
// unless they say otherwise.
let mainActorByDefault: [SwiftSetting] = upcomingFeatures + [.defaultIsolation(MainActor.self)]

let package = Package(
    name: "ConveyorCheck",
    platforms: [.iOS(.v26), .macOS(.v26)],
    products: [
        .library(name: "ConveyorCheck", targets: ["ConveyorCheck"]),
        .library(name: "ConveyorCheckUI", targets: ["ConveyorCheckUI"]),
    ],
    targets: [
        .target(name: "ConveyorCheck", swiftSettings: upcomingFeatures),
        .target(name: "ConveyorCheckUI", dependencies: ["ConveyorCheck"], swiftSettings: mainActorByDefault),
        .testTarget(name: "ConveyorCheckTests", dependencies: ["ConveyorCheck"], swiftSettings: upcomingFeatures),
    ],
    swiftLanguageModes: [.v6]
)
